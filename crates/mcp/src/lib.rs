//! A Model Context Protocol server over stdio, so an agent can design and export jobs through the
//! same commands the UI uses (`Session::execute`). Messages are newline-delimited JSON-RPC 2.0.
//!
//! Protocol reference: <https://modelcontextprotocol.io/specification>. Every engine command is a
//! tool named like its id with `.` replaced by `_` (`shape.rectangle` becomes `shape_rectangle`),
//! plus `document_summary`, a read-only overview of the drawing.

use std::io::{self, BufRead, Write};

use lightfoundry_engine::{Session, commands};
use serde_json::{Value, json};

/// Protocol version answered when the client does not name one we can echo.
const DEFAULT_PROTOCOL: &str = "2024-11-05";
/// Largest single message accepted, in bytes (a design with an embedded picture can be big).
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const SUMMARY_TOOL: &str = "document_summary";
/// Shapes listed by the summary before it says there are more.
const SUMMARY_SHAPES: usize = 200;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Reads messages from `input` until it ends, answering on `output`.
pub fn serve(session: &mut Session, input: &mut impl BufRead, output: &mut impl Write) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match read_capped_line(input, &mut buf, MAX_MESSAGE_BYTES)? {
            Line::End => return Ok(()),
            Line::TooLong => {
                write_message(output, &error_response(&Value::Null, INVALID_REQUEST, "message too large"))?;
            }
            Line::Ok => {
                let text = String::from_utf8_lossy(&buf);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                if let Some(reply) = handle_line(session, text) {
                    write_message(output, &reply)?;
                }
            }
        }
    }
}

/// Handles one message; `None` for notifications, which get no reply.
pub fn handle_line(session: &mut Session, line: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(error_response(&Value::Null, PARSE_ERROR, "invalid JSON"));
    };
    let Some(obj) = msg.as_object() else {
        return Some(error_response(&Value::Null, INVALID_REQUEST, "expected a JSON-RPC request object"));
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response (or junk) from the client: nothing to answer.
        return id.map(|id| error_response(&id, INVALID_REQUEST, "missing method"));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    let result = dispatch(session, method, &params);
    let id = id?; // notifications carry no id and get no reply
    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err((code, message)) => error_response(&id, code, &message),
    })
}

fn dispatch(session: &mut Session, method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "lightfoundry", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Design and export laser jobs. Units are mm, y points down from the bed's top-left corner. Call document_summary to see the drawing; shape_* create shapes, layer_set changes cut settings, job_gcode generates the program (include=true to get the text).",
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => call_tool(session, params),
        m if m.starts_with("notifications/") => Ok(Value::Null),
        _ => Err((METHOD_NOT_FOUND, format!("unknown method {method}"))),
    }
}

fn tool_list() -> Vec<Value> {
    let mut tools: Vec<Value> = commands()
        .map(|c| {
            json!({
                "name": tool_name(c.id),
                "title": c.label,
                "description": format!("{} Parameters: {}", c.label, c.params_doc),
                "inputSchema": { "type": "object", "additionalProperties": true },
                "annotations": { "readOnlyHint": !c.mutates },
            })
        })
        .collect();
    tools.push(json!({
        "name": SUMMARY_TOOL,
        "title": "Document summary",
        "description": "Read-only overview: machine, layers in use, selection and the first shapes with their bounds.",
        "inputSchema": { "type": "object", "properties": {} },
        "annotations": { "readOnlyHint": true },
    }));
    tools
}

fn call_tool(session: &mut Session, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_string()))?;
    if name == SUMMARY_TOOL {
        return Ok(tool_result(false, &summary(session)));
    }
    let spec = commands().find(|c| tool_name(c.id) == name).ok_or((INVALID_PARAMS, format!("unknown tool {name}")))?;
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Value::Null,
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Err((INVALID_PARAMS, "arguments must be an object".into())),
    };
    Ok(match session.execute(spec.id, &args) {
        Ok(v) => tool_result(false, &v),
        Err(e) => tool_result(true, &Value::String(e.to_string())),
    })
}

fn tool_result(is_error: bool, value: &Value) -> Value {
    let text = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn summary(s: &Session) -> Value {
    let m = &s.doc.machine;
    let shapes: Vec<Value> = s
        .doc
        .shapes
        .iter()
        .take(SUMMARY_SHAPES)
        .map(|sh| {
            let b = sh.world_bounds().map(|b| json!([b.x0, b.y0, b.x1, b.y1]));
            json!({ "id": sh.id, "name": sh.name, "layer": sh.layer, "bounds": b })
        })
        .collect();
    let used: std::collections::BTreeSet<u32> = s.doc.shapes.iter().map(|sh| sh.layer).collect();
    let layers: Vec<Value> = used
        .iter()
        .filter_map(|id| s.doc.layer(*id))
        .map(|l| json!({ "id": l.id, "name": l.name, "mode": l.mode, "speed": l.speed, "max_power": l.max_power, "passes": l.passes }))
        .collect();
    json!({
        "machine": { "name": m.name, "bed": [m.bed_width, m.bed_height], "origin": m.origin, "dialect": m.dialect },
        "shape_count": s.doc.shapes.len(),
        "shapes": shapes,
        "layers_in_use": layers,
        "selection": s.selection,
        "unsaved_changes": s.dirty,
    })
}

fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_message(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let text = serde_json::to_string(msg).map_err(io::Error::other)?;
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

enum Line {
    Ok,
    End,
    TooLong,
}

/// Reads up to a newline into `buf`, never holding more than `cap` bytes. A line over the cap is
/// skipped to its end and reported, so one huge message cannot exhaust memory.
fn read_capped_line(input: &mut impl BufRead, buf: &mut Vec<u8>, cap: usize) -> io::Result<Line> {
    let mut too_long = false;
    let mut any = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if any { if too_long { Line::TooLong } else { Line::Ok } } else { Line::End });
        }
        any = true;
        let (take, found) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > cap {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(chunk.get(..take).unwrap_or_default());
            }
        }
        input.consume(take);
        if found {
            return Ok(if too_long { Line::TooLong } else { Line::Ok });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(s: &mut Session, msg: Value) -> Value {
        handle_line(s, &msg.to_string()).unwrap()
    }

    fn call(s: &mut Session, name: &str, args: Value) -> Value {
        ask(s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}))
    }

    mod handshake_tests {
        use super::*;

        #[test]
        fn initialize_names_the_server_and_echoes_the_protocol_version() {
            let mut s = Session::new();
            let r = ask(&mut s, json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}));
            assert_eq!(r["id"], json!(7));
            assert_eq!(r["result"]["protocolVersion"], json!("2025-03-26"));
            assert_eq!(r["result"]["serverInfo"]["name"], json!("lightfoundry"));
            assert!(r["result"]["capabilities"]["tools"].is_object());
        }

        #[test]
        fn notifications_get_no_reply_and_ping_answers() {
            let mut s = Session::new();
            assert!(handle_line(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
            assert_eq!(ask(&mut s, json!({"jsonrpc": "2.0", "id": "a", "method": "ping"}))["result"], json!({}));
        }

        #[test]
        fn every_command_is_a_tool_with_a_valid_name() {
            let mut s = Session::new();
            let r = ask(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
            let tools = r["result"]["tools"].as_array().unwrap();
            assert_eq!(tools.len(), commands().count() + 1);
            for t in tools {
                let name = t["name"].as_str().unwrap();
                assert!(name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'), "{name}");
            }
            assert!(tools.iter().any(|t| t["name"] == "shape_rectangle"));
            assert!(tools.iter().any(|t| t["name"] == "document_summary"));
        }
    }

    mod tool_call_tests {
        use super::*;

        #[test]
        fn tools_run_engine_commands_and_the_summary_sees_the_result() {
            let mut s = Session::new();
            let r = call(&mut s, "shape_rectangle", json!({"x": 10, "y": 20, "width": 30, "height": 40}));
            assert_eq!(r["result"]["isError"], json!(false), "{r}");
            let sum = call(&mut s, "document_summary", Value::Null);
            let text = sum["result"]["content"][0]["text"].as_str().unwrap();
            let v: Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["shape_count"], json!(1));
            assert_eq!(v["shapes"][0]["bounds"], json!([10.0, 20.0, 40.0, 60.0]));
            let g = call(&mut s, "job_gcode", json!({"include": true}));
            assert!(g["result"]["content"][0]["text"].as_str().unwrap().contains("G21"));
        }

        #[test]
        fn engine_errors_are_tool_errors_not_protocol_errors() {
            let mut s = Session::new();
            let r = call(&mut s, "shape_rectangle", json!({"width": "wide"}));
            assert_eq!(r["result"]["isError"], json!(true));
            assert!(r.get("error").is_none());
            let out_of_bed = call(&mut s, "job_gcode", json!({}));
            assert_eq!(out_of_bed["result"]["isError"], json!(false), "an empty job is fine");
        }

        #[test]
        fn mutations_over_mcp_are_undoable_like_any_other() {
            let mut s = Session::new();
            call(&mut s, "shape_rectangle", json!({"width": 5, "height": 5}));
            call(&mut s, "edit_undo", Value::Null);
            assert!(s.doc.shapes.is_empty());
        }
    }

    mod hostile_input_tests {
        use super::*;

        #[test]
        fn malformed_messages_get_protocol_errors() {
            let mut s = Session::new();
            for (line, code) in [("not json", PARSE_ERROR), ("[1,2]", INVALID_REQUEST), ("42", INVALID_REQUEST), ("\"x\"", INVALID_REQUEST)] {
                assert_eq!(handle_line(&mut s, line).unwrap()["error"]["code"], json!(code), "{line}");
            }
            let unknown = ask(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "nope"}));
            assert_eq!(unknown["error"]["code"], json!(METHOD_NOT_FOUND));
            for params in
                [json!({}), json!({"name": 5}), json!({"name": "no_such_tool"}), json!({"name": "shape_rectangle", "arguments": [1]}), json!(null)]
            {
                let r = ask(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params}));
                assert_eq!(r["error"]["code"], json!(INVALID_PARAMS), "{params}");
            }
        }

        #[test]
        fn a_response_shaped_message_without_a_method_is_ignored_or_rejected_politely() {
            let mut s = Session::new();
            assert!(handle_line(&mut s, r#"{"jsonrpc":"2.0","result":{}}"#).is_none());
            assert!(handle_line(&mut s, r#"{"jsonrpc":"2.0","id":3,"result":{}}"#).is_some());
        }

        #[test]
        fn every_command_survives_junk_arguments_through_the_protocol() {
            let mut s = Session::new();
            let names: Vec<String> = commands().map(|c| tool_name(c.id)).filter(|n| !n.starts_with("file_")).collect();
            for name in names {
                for args in [json!({}), json!({"id": -1, "x": "a", "width": null}), json!({"ids": [1e300], "text": 5})] {
                    let r = call(&mut s, &name, args);
                    assert!(r.get("result").is_some() || r.get("error").is_some(), "{name}");
                }
            }
        }
    }

    mod stdio_tests {
        use std::io::Cursor;

        use super::*;

        #[test]
        fn the_serve_loop_answers_each_line_and_stops_at_the_end() {
            let mut s = Session::new();
            let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\nbroken\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}";
            let mut out = Vec::new();
            serve(&mut s, &mut Cursor::new(input), &mut out).unwrap();
            let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
            assert_eq!(lines.len(), 3, "{lines:?}");
            assert_eq!(lines[0]["id"], json!(1));
            assert_eq!(lines[1]["error"]["code"], json!(PARSE_ERROR));
            assert_eq!(lines[2]["id"], json!(2), "the last line needs no newline");
        }

        #[test]
        fn oversized_lines_are_skipped_without_being_held_in_memory() {
            let mut input = Cursor::new(format!("{}\n{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}}\n", "x".repeat(5000)));
            let mut buf = Vec::new();
            assert!(matches!(read_capped_line(&mut input, &mut buf, 100).unwrap(), Line::TooLong));
            assert!(buf.is_empty());
            assert!(matches!(read_capped_line(&mut input, &mut buf, 100).unwrap(), Line::Ok));
            assert!(buf.starts_with(b"{\"jsonrpc"));
            assert!(matches!(read_capped_line(&mut input, &mut buf, 100).unwrap(), Line::End));
        }
    }
}
