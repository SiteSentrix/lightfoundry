use lightfoundry_device::{SimulatedGrbl, StreamOptions, stream_program};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "job.gcode",
        label: "Generate G-code",
        menu: "Laser",
        shortcut: None,
        params_doc: "{include?: bool}  — stores the program in the session; include=true returns the text",
        mutates: false,
        run: gcode,
    },
    CommandSpec { id: "job.estimate", label: "Estimate Time", menu: "Laser", shortcut: None, params_doc: "{}", mutates: false, run: estimate },
    CommandSpec {
        id: "job.frame",
        label: "Frame",
        menu: "Laser",
        shortcut: None,
        params_doc: "{speed? (mm/min, default 3000), power? (%, default 0)}  — returns a program tracing the job bounds",
        mutates: false,
        run: frame,
    },
    CommandSpec {
        id: "job.simulate",
        label: "Simulate on Virtual Machine",
        menu: "Laser",
        shortcut: None,
        params_doc: "{}  — streams the program to a simulated GRBL controller",
        mutates: false,
        run: simulate,
    },
];

/// Plans the document and renders it as G-code.
pub(super) fn generate(s: &Session) -> Result<String, EngineError> {
    let job = lightfoundry_toolpath::plan(&s.doc)?;
    Ok(lightfoundry_gcode::generate(&job, &s.doc.machine)?)
}

#[derive(Deserialize, Default)]
struct GcodeParams {
    #[serde(default)]
    include: bool,
}

fn gcode(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: GcodeParams = parse(params)?;
    let program = generate(s)?;
    let summary = json!({ "lines": program.lines().count(), "bytes": program.len() });
    let out = if p.include { json!({ "summary": summary, "gcode": program }) } else { summary };
    s.last_gcode = Some(program);
    Ok(out)
}

fn estimate(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    let job = lightfoundry_toolpath::plan(&s.doc)?;
    let e = lightfoundry_toolpath::estimate(&job, s.doc.machine.rapid_speed);
    Ok(json!({ "seconds": e.seconds, "cut_mm": e.cut_mm, "travel_mm": e.travel_mm }))
}

#[derive(Deserialize)]
struct FrameParams {
    #[serde(default = "frame_speed")]
    speed: f64,
    #[serde(default)]
    power: f64,
}

fn frame_speed() -> f64 {
    3000.0
}

fn frame(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: FrameParams = parse(params)?;
    let bounds = lightfoundry_toolpath::job_bounds(&s.doc).ok_or_else(|| EngineError::NotFound("there is nothing to frame".into()))?;
    let program = lightfoundry_gcode::frame(bounds, &s.doc.machine, p.speed, p.power)?;
    Ok(json!({ "gcode": program, "bounds": [bounds.x0, bounds.y0, bounds.x1, bounds.y1] }))
}

fn simulate(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    let program = generate(s)?;
    let mut sim = SimulatedGrbl::new();
    let report = stream_program(&mut sim, &program, &StreamOptions::default(), &std::sync::atomic::AtomicBool::new(false), |_| {})?;
    Ok(json!({ "lines_streamed": report.lines, "peak_rx_bytes": sim.max_buffer_used(), "overran_buffer": sim.overran_buffer() }))
}
