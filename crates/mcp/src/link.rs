//! The live link: lets an agent work in the document of a running desktop app instead of a private
//! session.
//!
//! The app opts in (it is off by default) and listens on the loopback interface only, on a port the
//! system picks. It writes the port and a random token to a per-user *link file* readable by the
//! owner alone; a client must present that token as its first line, so another local user, or a web
//! page poking at `localhost`, cannot drive the app. Connection threads only move lines: every
//! command runs on the app's own thread through [`LinkServer::serve_pending`], once per frame, so the
//! UI and the agent never touch the session at the same time. The client side, [`proxy`], bridges
//! an MCP client's stdio to the socket (`lightfoundry-cli mcp --connect`).
//!
//! Only engine commands cross the link. Starting a job on a machine is a UI action and is not
//! reachable, and commands that would replace unsaved work are refused ([`Mode::Live`]).

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lightfoundry_engine::Session;
use serde_json::{Value, json};

use crate::{Line, MAX_MESSAGE_BYTES, Mode, handle_line_in, read_capped_line, write_message};

/// Agents connected at once.
const MAX_CLIENTS: usize = 4;
/// Longest accepted handshake line, in bytes.
const MAX_HANDSHAKE_BYTES: usize = 512;
/// How long a new connection may take to present its token.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a request may wait for the app to get to it (a stalled or closed window).
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);
/// Requests answered per frame, so a flood cannot freeze the window.
const MAX_PER_PASS: usize = 16;
/// Bytes of randomness in the token.
const TOKEN_BYTES: usize = 32;

struct Request {
    line: String,
    reply: Sender<Option<Value>>,
}

/// A running listener, owned by the app. Dropping it closes the port and removes the link file.
pub struct LinkServer {
    requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
    addr: SocketAddr,
    link_file: PathBuf,
}

impl LinkServer {
    /// Opens a loopback port, writes the link file at `link_file` and starts accepting agents.
    /// `wake` is called (from another thread) whenever a request arrives, so the app can repaint.
    pub fn start(link_file: &Path, wake: impl Fn() + Send + Sync + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let addr = listener.local_addr()?;
        let token = new_token()?;
        write_link_file(link_file, addr.port(), &token)?;

        let (tx, requests) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(AtomicUsize::new(0));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let accept_stop = stop.clone();
        thread::Builder::new().name("lightfoundry-link".into()).spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                    clients.fetch_sub(1, Ordering::AcqRel);
                    continue; // dropped: too many agents
                }
                let (tx, token, wake, count) = (tx.clone(), token.clone(), wake.clone(), clients.clone());
                let spawned = thread::Builder::new().name("lightfoundry-link-client".into()).spawn(move || {
                    if let Err(e) = client(stream, &token, &tx, wake.as_ref()) {
                        log::debug!("agent link closed: {e}");
                    }
                    count.fetch_sub(1, Ordering::AcqRel);
                });
                if spawned.is_err() {
                    clients.fetch_sub(1, Ordering::AcqRel); // the thread never ran, so it never gave its slot back
                    log::warn!("could not start a link thread");
                }
            }
        })?;
        Ok(Self { requests, stop, addr, link_file: link_file.to_path_buf() })
    }

    /// Where agents can connect.
    pub fn address(&self) -> SocketAddr {
        self.addr
    }

    /// Answers the requests that arrived since the last call (at most a frame's worth), running
    /// them on `session`. Call from the thread that owns the session. Returns how many were handled.
    pub fn serve_pending(&self, session: &mut Session) -> usize {
        let mut handled = 0;
        while handled < MAX_PER_PASS {
            let Ok(request) = self.requests.try_recv() else { break };
            let reply = handle_line_in(session, request.line.trim(), Mode::Live);
            let _ = request.reply.send(reply); // the agent may have gone
            handled += 1;
        }
        handled
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200)); // wakes the accept loop
        let _ = fs::remove_file(&self.link_file);
    }
}

/// One agent: checks the token, then forwards each line to the app and writes back the answer.
fn client(stream: TcpStream, token: &str, tx: &Sender<Request>, wake: &(dyn Fn() + Send + Sync)) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut input = BufReader::new(stream.try_clone()?);
    let mut output = stream;

    let mut buf = Vec::new();
    match read_capped_line(&mut input, &mut buf, MAX_HANDSHAKE_BYTES)? {
        Line::Ok => {}
        Line::End | Line::TooLong => return Err(io::Error::other("bad handshake")),
    }
    let presented = serde_json::from_slice::<Value>(&buf).ok().and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_owned));
    if !presented.is_some_and(|p| same(p.as_bytes(), token.as_bytes())) {
        write_message(&mut output, &json!({ "ok": false, "error": "wrong token" }))?;
        return Err(io::Error::other("wrong token"));
    }
    write_message(&mut output, &json!({ "ok": true }))?;
    output.set_read_timeout(None)?;

    loop {
        buf.clear();
        match read_capped_line(&mut input, &mut buf, MAX_MESSAGE_BYTES)? {
            Line::End => return Ok(()),
            Line::TooLong => {
                write_message(&mut output, &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": "message too large" } }))?
            }
            Line::Ok => {
                let line = String::from_utf8_lossy(&buf).into_owned();
                if line.trim().is_empty() {
                    continue;
                }
                let (reply, answer) = mpsc::channel();
                tx.send(Request { line, reply }).map_err(|_| io::Error::other("app closed"))?;
                wake();
                match answer.recv_timeout(REPLY_TIMEOUT) {
                    Ok(Some(value)) => write_message(&mut output, &value)?,
                    Ok(None) => {} // a notification
                    Err(_) => return Err(io::Error::other("the app did not answer")),
                }
            }
        }
    }
}

/// Where the running app and `lightfoundry-cli mcp --connect` agree to meet: the per-user data folder.
pub fn default_link_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_RUNTIME_DIR").or_else(|| var("XDG_DATA_HOME")).or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    }?;
    Some(base.join("LightFoundry").join("agent-link.json"))
}

/// Connects to the app named by `link_file` and relays MCP lines between `input`/`output` (an
/// agent's stdio) and it, until `input` ends.
pub fn proxy(link_file: &Path, input: impl BufRead, output: impl Write + Send + 'static) -> io::Result<()> {
    let text = fs::read_to_string(link_file)
        .map_err(|e| io::Error::new(e.kind(), format!("no running LightFoundry to connect to ({}): {e}", link_file.display())))?;
    let info: Value = serde_json::from_str(&text).map_err(|e| io::Error::other(format!("unreadable link file: {e}")))?;
    let port =
        info.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).ok_or_else(|| io::Error::other("link file has no port"))?;
    let token = info.get("token").and_then(Value::as_str).ok_or_else(|| io::Error::other("link file has no token"))?;

    let stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_secs(3))
        .map_err(|e| io::Error::new(e.kind(), format!("LightFoundry is not listening (is agent access switched on?): {e}")))?;
    stream.set_nodelay(true)?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    write_message(&mut writer, &json!({ "token": token }))?;
    let mut hello = String::new();
    reader.by_ref().take(MAX_HANDSHAKE_BYTES as u64).read_line(&mut hello)?;
    if serde_json::from_str::<Value>(&hello).ok().and_then(|v| v.get("ok").and_then(Value::as_bool)) != Some(true) {
        return Err(io::Error::other("LightFoundry refused the connection (stale link file?)"));
    }

    let output = Arc::new(Mutex::new(output));
    let relay_out = output.clone();
    let relay = thread::spawn(move || {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match read_capped_line(&mut reader, &mut buf, MAX_MESSAGE_BYTES) {
                Ok(Line::Ok) => {
                    let Ok(mut out) = relay_out.lock() else { return };
                    if out.write_all(&buf).and_then(|()| out.flush()).is_err() {
                        return;
                    }
                }
                Ok(Line::TooLong) => {}
                Ok(Line::End) | Err(_) => return,
            }
        }
    });

    let mut input = input;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match read_capped_line(&mut input, &mut buf, MAX_MESSAGE_BYTES)? {
            Line::End => break,
            Line::TooLong => continue,
            Line::Ok => {
                writer.write_all(&buf)?;
                writer.flush()?;
            }
        }
    }
    let _ = writer.shutdown(Shutdown::Write); // the app finishes pending answers, then closes
    let _ = relay.join();
    Ok(())
}

fn write_link_file(path: &Path, port: u16, token: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let _ = fs::remove_file(path); // so the owner-only mode applies to a fresh file
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(json!({ "port": port, "token": token, "pid": std::process::id() }).to_string().as_bytes())?;
    file.flush()
}

/// A random hex token. Prefers the operating system's generator; falls back to the process-random
/// hash keys the standard library seeds from it.
fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    let from_os = fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).is_ok();
    if !from_os {
        use std::hash::{BuildHasher, Hasher};
        for chunk in bytes.chunks_mut(8) {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
            let word = h.finish().to_le_bytes();
            for (dst, src) in chunk.iter_mut().zip(word) {
                *dst = src;
            }
        }
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    mod link_tests {
        use super::*;

        #[test]
        fn an_agent_edits_the_open_document_through_the_link() {
            let (file, _guard) = scratch("roundtrip");
            let server = LinkServer::start(&file, || {}).unwrap();
            let mut session = Session::new();
            let (mut agent, mut lines) = connect(&file);
            writeln!(agent, "{}", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"shape_rectangle","arguments":{"x":5,"y":5,"width":20,"height":10}}})).unwrap();
            let reply = pump(&server, &mut session, &mut lines);
            assert_eq!(reply["id"], json!(1));
            assert_eq!(reply["result"]["isError"], json!(false));
            assert_eq!(session.doc.shapes.len(), 1, "the agent's shape is in the window's document");
        }

        #[test]
        fn a_wrong_token_is_turned_away_and_changes_nothing() {
            let (file, _guard) = scratch("token");
            let server = LinkServer::start(&file, || {}).unwrap();
            let mut s = TcpStream::connect(server.address()).unwrap();
            writeln!(s, r#"{{"token":"nope"}}"#).unwrap();
            let mut answer = String::new();
            BufReader::new(s.try_clone().unwrap()).read_line(&mut answer).unwrap();
            assert!(answer.contains("false"));
            let _ = writeln!(s, r#"{{"jsonrpc":"2.0","id":1,"method":"ping"}}"#);
            assert_eq!(server.serve_pending(&mut Session::new()), 0);
        }

        #[test]
        fn hostile_handshakes_are_dropped() {
            let (file, _guard) = scratch("hostile");
            let server = LinkServer::start(&file, || {}).unwrap();
            for junk in [vec![b'x'; 4096], b"\xff\xfe\n".to_vec(), b"[]\n".to_vec(), b"{\"token\":7}\n".to_vec(), b"\n".to_vec()] {
                let mut s = TcpStream::connect(server.address()).unwrap();
                let _ = s.write_all(&junk);
                let _ = s.write_all(b"\n");
                let mut rest = String::new();
                let _ = BufReader::new(s).read_to_string(&mut rest); // ends: the server hangs up
                assert!(!rest.contains("true"));
            }
            assert_eq!(server.serve_pending(&mut Session::new()), 0);
        }

        #[test]
        fn commands_that_would_discard_unsaved_work_are_refused_but_others_run() {
            let (file, _guard) = scratch("live");
            let server = LinkServer::start(&file, || {}).unwrap();
            let mut session = Session::new();
            session.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
            assert!(session.dirty);
            let (mut agent, mut lines) = connect(&file);
            writeln!(agent, "{}", json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"file_new","arguments":{}}})).unwrap();
            let reply = pump(&server, &mut session, &mut lines);
            assert_eq!(reply["result"]["isError"], json!(true));
            assert_eq!(session.doc.shapes.len(), 1, "the person's drawing is still there");
        }

        #[test]
        fn the_link_file_is_private_and_removed_when_the_server_goes() {
            let (file, _guard) = scratch("file");
            let server = LinkServer::start(&file, || {}).unwrap();
            let info: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
            assert_eq!(info["port"], json!(server.address().port()));
            assert_eq!(info["token"].as_str().unwrap().len(), TOKEN_BYTES * 2);
            #[cfg(unix)]
            assert_eq!(
                std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&file).unwrap().permissions()) & 0o077,
                0,
                "no access for group or others"
            );
            drop(server);
            assert!(!file.exists());
        }

        #[test]
        fn the_proxy_reports_a_missing_or_stale_link_file() {
            let (file, _guard) = scratch("stale");
            let err = proxy(&file, io::empty(), io::sink()).unwrap_err();
            assert!(err.to_string().contains("no running LightFoundry"));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, "{\"port\": 1, \"token\": \"x\"}").unwrap();
            assert!(proxy(&file, io::empty(), io::sink()).is_err());
            fs::write(&file, "not json").unwrap();
            assert!(proxy(&file, io::empty(), io::sink()).is_err());
        }

        #[test]
        fn the_proxy_relays_a_session_end_to_end() {
            let (file, _guard) = scratch("proxy");
            let server = LinkServer::start(&file, || {}).unwrap();
            let done = Arc::new(AtomicBool::new(false));
            let flag = done.clone();
            let path = file.clone();
            let out = Arc::new(Mutex::new(Vec::new()));
            let sink = SharedSink(out.clone());
            let client = thread::spawn(move || {
                let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
                let r = proxy(&path, &input[..], sink);
                flag.store(true, Ordering::Release);
                r
            });
            let mut session = Session::new();
            while !done.load(Ordering::Acquire) {
                server.serve_pending(&mut session);
                thread::sleep(Duration::from_millis(5));
            }
            client.join().unwrap().unwrap();
            let text = String::from_utf8(out.lock().unwrap().clone()).unwrap();
            assert!(text.contains("\"id\":1"), "{text}");
        }
    }

    struct SharedSink(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().map_err(|_| io::Error::other("poisoned"))?.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> (PathBuf, Cleanup) {
        let dir = std::env::temp_dir().join(format!("lf-link-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        (dir.join("agent-link.json"), Cleanup(dir))
    }

    /// Connects like an agent would, presenting the token from the link file.
    fn connect(file: &Path) -> (TcpStream, BufReader<TcpStream>) {
        let info: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, info["port"].as_u64().unwrap() as u16)).unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        writeln!(writer, "{}", json!({ "token": info["token"] })).unwrap();
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        assert!(hello.contains("true"), "{hello}");
        (writer, reader)
    }

    /// Plays the app's frame loop until the agent has its answer.
    fn pump(server: &LinkServer, session: &mut Session, lines: &mut BufReader<TcpStream>) -> Value {
        lines.get_ref().set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let mut line = String::new();
        for _ in 0..500 {
            server.serve_pending(session);
            match lines.read_line(&mut line) {
                Ok(n) if n > 0 => return serde_json::from_str(&line).unwrap(),
                _ => {}
            }
        }
        panic!("no answer from the app");
    }
}
