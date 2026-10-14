//! A Model Context Protocol server over stdio, so an agent can design and export jobs through the
//! same commands the UI uses (`Session::execute`). Messages are newline-delimited JSON-RPC 2.0.
//!
//! Protocol reference: <https://modelcontextprotocol.io/specification>. Every engine command is a
//! tool named like its id with `.` replaced by `_` (`shape.rectangle` becomes `shape_rectangle`),
//! plus `document_summary`, a read-only overview of the drawing.

use std::io::{self, BufRead, Write};

use lightfoundry_engine::{Session, commands};
use serde_json::{Value, json};

pub mod link;

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

/// Who is on the other end: a private session of the agent's own, or the person's open window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The server owns its session (`lightfoundry-cli mcp`); nothing there can be lost.
    Standalone,
    /// The agent is working in the document the person has open, so commands that would throw away
    /// unsaved work are refused.
    Live,
}

/// Commands that replace the open document; the live link refuses them while it has unsaved changes.
const REPLACES_DOCUMENT: &[&str] = &["file.new", "file.open", "file.recover"];

/// Handles one message; `None` for notifications, which get no reply.
pub fn handle_line(session: &mut Session, line: &str) -> Option<Value> {
    handle_line_in(session, line, Mode::Standalone)
}

/// [`handle_line`] for a server in the given [`Mode`].
pub fn handle_line_in(session: &mut Session, line: &str, mode: Mode) -> Option<Value> {
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
    let result = dispatch(session, method, &params, mode);
    let id = id?; // notifications carry no id and get no reply
    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err((code, message)) => error_response(&id, code, &message),
    })
}

fn dispatch(session: &mut Session, method: &str, params: &Value, mode: Mode) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": { "name": "lightfoundry", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Design and export laser jobs. Units are mm, y points down from the bed's top-left corner. Call document_summary to see the drawing; shape_* create shapes, layer_set changes cut settings, job_gcode generates the program (include=true to get the text).",
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => call_tool(session, params, mode),
        "resources/list" => Ok(json!({ "resources": resource_list() })),
        "resources/read" => read_resource(session, params),
        "prompts/list" => Ok(json!({ "prompts": prompt_list() })),
        "prompts/get" => get_prompt(params),
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

fn call_tool(session: &mut Session, params: &Value, mode: Mode) -> Result<Value, (i64, String)> {
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
    if mode == Mode::Live && session.dirty && REPLACES_DOCUMENT.contains(&spec.id) {
        return Ok(tool_result(
            true,
            &Value::String(format!("{} would discard unsaved changes in the open window; ask the person to save first", spec.id)),
        ));
    }
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

const DOCUMENT_URI: &str = "lightfoundry://document/summary";
const GCODE_URI: &str = "lightfoundry://job/gcode";
const COMMANDS_URI: &str = "lightfoundry://reference/commands";

fn resource_list() -> Vec<Value> {
    vec![
        json!({ "uri": DOCUMENT_URI, "name": "Document summary", "description": "Machine, layers in use, selection and the shapes with their bounds", "mimeType": "application/json" }),
        json!({ "uri": GCODE_URI, "name": "G-code for the current design", "description": "The program the job would run now (generated on read)", "mimeType": "text/plain" }),
        json!({ "uri": COMMANDS_URI, "name": "Command reference", "description": "Every command with its parameters", "mimeType": "text/plain" }),
    ]
}

fn read_resource(session: &mut Session, params: &Value) -> Result<Value, (i64, String)> {
    let uri = params.get("uri").and_then(Value::as_str).ok_or((INVALID_PARAMS, "resources/read needs a uri".to_string()))?;
    let (mime, text) = match uri {
        DOCUMENT_URI => ("application/json", serde_json::to_string(&summary(session)).unwrap_or_default()),
        GCODE_URI => match session.execute("job.gcode", &json!({ "include": true })) {
            Ok(v) => ("text/plain", v["gcode"].as_str().unwrap_or_default().to_string()),
            Err(e) => return Err((INVALID_PARAMS, format!("cannot generate G-code: {e}"))),
        },
        COMMANDS_URI => {
            ("text/plain", commands().map(|c| format!("{} ({}): {}", tool_name(c.id), c.label, c.params_doc)).collect::<Vec<_>>().join("\n"))
        }
        _ => return Err((INVALID_PARAMS, format!("unknown resource {uri}"))),
    };
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] }))
}

/// Prompt templates: (name, description, arguments, text with `{name}` placeholders).
const PROMPTS: &[(&str, &str, &[&str], &str)] = &[
    (
        "design_from_description",
        "Design something for the laser from a description",
        &["description"],
        "Design this for a laser engraver/cutter: {description}\n\nWork in millimetres (y points down from the bed's top-left). Check the bed size with document_summary, build the shapes with the shape_* and text_create tools, put cuts and engravings on separate layers with layer_set (cuts: Line mode; engravings: Fill), then use job_estimate to report the time and ask before generating G-code.",
    ),
    (
        "prepare_for_cutting",
        "Check a design before it is sent to a machine",
        &[],
        "Review the current design before cutting. Use document_summary and the document resources. Check: everything is inside the bed; cut layers are Line mode with sensible speed/power for the material; engraved areas are Fill; nothing is on a layer that does not output; inner shapes are cut before outer ones; duplicates are removed. Report problems and fix only what is clearly wrong. Always recommend a Frame run at 0% power and a material test grid first.",
    ),
    (
        "explain_job",
        "Explain what the job will do and how long it will take",
        &[],
        "Call job_estimate and document_summary and explain in plain language what the job will do, layer by layer, how long each part takes, and anything surprising (very slow layers, many passes, travel-heavy order).",
    ),
];

fn prompt_list() -> Vec<Value> {
    PROMPTS
        .iter()
        .map(|(name, description, args, _)| {
            let arguments: Vec<Value> = args.iter().map(|a| json!({ "name": a, "required": true })).collect();
            json!({ "name": name, "description": description, "arguments": arguments })
        })
        .collect()
}

fn get_prompt(params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "prompts/get needs a name".to_string()))?;
    let (_, description, args, template) = PROMPTS.iter().find(|(n, ..)| *n == name).ok_or((INVALID_PARAMS, format!("unknown prompt {name}")))?;
    let mut text = (*template).to_string();
    for arg in *args {
        let value =
            params.get("arguments").and_then(|a| a.get(*arg)).and_then(Value::as_str).ok_or((INVALID_PARAMS, format!("missing argument {arg}")))?;
        text = text.replace(&format!("{{{arg}}}"), value);
    }
    Ok(json!({ "description": description, "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
}

fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub(crate) fn write_message(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let text = serde_json::to_string(msg).map_err(io::Error::other)?;
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

pub(crate) enum Line {
    Ok,
    End,
    TooLong,
}

/// Reads up to a newline into `buf`, never holding more than `cap` bytes. A line over the cap is
/// skipped to its end and reported, so one huge message cannot exhaust memory.
pub(crate) fn read_capped_line(input: &mut impl BufRead, buf: &mut Vec<u8>, cap: usize) -> io::Result<Line> {
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

    mod resource_and_prompt_tests {
        use super::*;

        #[test]
        fn resources_list_and_read_the_live_design() {
            let mut s = Session::new();
            let r = ask(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "resources/list"}));
            assert_eq!(r["result"]["resources"].as_array().unwrap().len(), 3);
            call(&mut s, "shape_rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 10}));
            let read = |s: &mut Session, uri: &str| ask(s, json!({"jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": {"uri": uri}}));
            let doc = read(&mut s, DOCUMENT_URI);
            let v: Value = serde_json::from_str(doc["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(v["shape_count"], json!(1));
            let g = read(&mut s, GCODE_URI);
            assert!(g["result"]["contents"][0]["text"].as_str().unwrap().contains("G21"));
            let c = read(&mut s, COMMANDS_URI);
            assert!(c["result"]["contents"][0]["text"].as_str().unwrap().contains("shape_rectangle"));
            assert_eq!(read(&mut s, "lightfoundry://nope")["error"]["code"], json!(INVALID_PARAMS));
            assert_eq!(
                ask(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {}}))["error"]["code"],
                json!(INVALID_PARAMS)
            );
        }

        #[test]
        fn prompts_fill_in_their_arguments_and_refuse_missing_ones() {
            let mut s = Session::new();
            let list = ask(&mut s, json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/list"}));
            assert!(list["result"]["prompts"].as_array().unwrap().iter().any(|p| p["name"] == "design_from_description"));
            let got = ask(
                &mut s,
                json!({"jsonrpc": "2.0", "id": 2, "method": "prompts/get", "params": {"name": "design_from_description", "arguments": {"description": "a 40 mm round coaster"}}}),
            );
            let text = got["result"]["messages"][0]["content"]["text"].as_str().unwrap();
            assert!(text.contains("a 40 mm round coaster") && !text.contains("{description}"));
            for params in [json!({"name": "design_from_description"}), json!({"name": "nope"}), json!({})] {
                assert!(
                    ask(&mut s, json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/get", "params": params})).get("error").is_some(),
                    "{params}"
                );
            }
            assert!(
                ask(&mut s, json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": {"name": "explain_job"}})).get("result").is_some()
            );
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
