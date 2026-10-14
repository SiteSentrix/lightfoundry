//! Headless LightFoundry: batch conversion and a JSON command interface.
//!
//! ```text
//! lightfoundry-cli gcode design.svg -o job.gcode [--layer 0:speed=900,max_power=40] [--bed 300x200] [--origin top_left]
//! lightfoundry-cli info design.svg
//! lightfoundry-cli commands
//! lightfoundry-cli mcp                        # Model Context Protocol server on stdin/stdout
//! lightfoundry-cli run script.json            # [{"id": "...", "params": {...}}, ...] then prints the last result
//! ```

use std::path::Path;
use std::process::ExitCode;

use lightfoundry_engine::{Session, commands};
use serde_json::{Value, json};

const USAGE: &str = "\
usage: lightfoundry-cli <command>

  gcode <input.svg|dxf|png|jpg|lightfoundry> -o <out.gcode> [--bed WxH] [--origin top_left|bottom_left]
                       [--start-from absolute|user_origin|current_position] [--anchor center|top_left|...]
                       [--set LAYER:key=value,key=value]...
  info <input>         shape count, size, estimated time
  commands             list every engine command with its parameters
  mcp                  serve the engine's commands as MCP tools on stdin/stdout (for AI agents)
  run <script.json>    run a JSON array of {\"id\", \"params\"} commands and print the last result
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("commands") => Ok(commands().map(|c| format!("{:<22} {:<28} {}", c.id, c.label, c.params_doc)).collect::<Vec<_>>().join("\n")),
        Some("info") => info(args.get(1).ok_or(USAGE)?),
        Some("gcode") => gcode(&args[1..]),
        Some("run") => script(args.get(1).ok_or(USAGE)?),
        Some("mcp") => mcp(),
        _ => Err(USAGE.into()),
    }
}

fn load(path: &str) -> Result<Session, String> {
    let mut s = Session::new();
    let id = match Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("svg") => "file.import_svg",
        Some("dxf") => "file.import_dxf",
        Some("png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp") => "file.import_image",
        _ => "file.open",
    };
    s.execute(id, &json!({ "path": path })).map_err(|e| format!("{path}: {e}"))?;
    Ok(s)
}

fn info(path: &str) -> Result<String, String> {
    let mut s = load(path)?;
    let est = s.execute("job.estimate", &Value::Null).map_err(|e| e.to_string())?;
    let b = s.doc.bounds_of(&s.doc.shapes.iter().map(|sh| sh.id).collect::<Vec<_>>());
    Ok(format!(
        "shapes: {}\nsize: {}\nestimated time: {:.0} s (cut {:.0} mm, travel {:.0} mm)",
        s.doc.shapes.len(),
        b.map_or("empty".into(), |b| format!("{:.2} × {:.2} mm", b.width(), b.height())),
        est["seconds"].as_f64().unwrap_or(0.0),
        est["cut_mm"].as_f64().unwrap_or(0.0),
        est["travel_mm"].as_f64().unwrap_or(0.0),
    ))
}

fn gcode(args: &[String]) -> Result<String, String> {
    let input = args.first().ok_or(USAGE)?;
    let mut s = load(input)?;
    let mut out_path = None;
    let mut i = 1;
    while let Some(flag) = args.get(i) {
        let value = args.get(i + 1).ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "-o" => out_path = Some(value.clone()),
            "--bed" => {
                let (w, h) = value.split_once('x').ok_or("--bed expects WIDTHxHEIGHT, e.g. 300x200")?;
                let (w, h): (f64, f64) = (w.parse().map_err(|_| "bad bed width")?, h.parse().map_err(|_| "bad bed height")?);
                s.execute("machine.set", &json!({ "bed_width": w, "bed_height": h })).map_err(|e| e.to_string())?;
            }
            "--origin" => {
                s.execute("machine.set", &json!({ "origin": value })).map_err(|e| e.to_string())?;
            }
            "--start-from" => {
                s.execute("job.set_origin", &json!({ "start_from": value })).map_err(|e| e.to_string())?;
            }
            "--anchor" => {
                s.execute("job.set_origin", &json!({ "anchor": value })).map_err(|e| e.to_string())?;
            }
            "--set" => apply_layer_settings(&mut s, value)?,
            other => return Err(format!("unknown option {other}")),
        }
        i += 2;
    }
    let out_path = out_path.ok_or("missing -o <out.gcode>")?;
    s.execute("file.export_gcode", &json!({ "path": out_path })).map_err(|e| e.to_string())?;
    let est = s.execute("job.estimate", &Value::Null).map_err(|e| e.to_string())?;
    Ok(format!("wrote {out_path} (estimated {:.0} s)", est["seconds"].as_f64().unwrap_or(0.0)))
}

/// `3:speed=900,max_power=40,mode=fill`
fn apply_layer_settings(s: &mut Session, spec: &str) -> Result<(), String> {
    let (layer, rest) = spec.split_once(':').ok_or("--set expects LAYER:key=value,...")?;
    let mut params = serde_json::Map::new();
    params.insert("id".into(), json!(layer.parse::<u32>().map_err(|_| "layer must be a number")?));
    for pair in rest.split(',') {
        let (k, v) = pair.split_once('=').ok_or("--set expects key=value pairs")?;
        let value = v.parse::<f64>().map(|n| json!(n)).or_else(|_| v.parse::<bool>().map(|b| json!(b))).unwrap_or_else(|_| json!(v));
        params.insert(k.into(), value);
    }
    s.execute("layer.set", &Value::Object(params)).map(|_| ()).map_err(|e| e.to_string())
}

fn script(path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let steps: Vec<Value> = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let mut s = Session::new();
    let mut last = Value::Null;
    for (n, step) in steps.iter().enumerate() {
        let id = step["id"].as_str().ok_or_else(|| format!("step {}: missing \"id\"", n + 1))?;
        last = s.execute(id, &step["params"]).map_err(|e| format!("step {} ({id}): {e}", n + 1))?;
    }
    serde_json::to_string_pretty(&last).map_err(|e| e.to_string())
}

/// Speaks MCP on stdin/stdout until the client closes the pipe. Nothing else may print to stdout.
fn mcp() -> Result<String, String> {
    let mut s = Session::new();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout().lock();
    lightfoundry_mcp::serve(&mut s, &mut input, &mut output).map_err(|e| e.to_string())?;
    Ok(String::new())
}
