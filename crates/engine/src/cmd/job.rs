use lightfoundry_device::{SimulatedGrbl, StreamOptions, stream_program};
use lightfoundry_doc::{Anchor, StartFrom};
use lightfoundry_gcode::Placement;
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
    CommandSpec {
        id: "job.set_origin",
        label: "Start From / Job Origin",
        menu: "Laser",
        shortcut: None,
        params_doc: "{start_from?: absolute|user_origin|current_position, anchor?: top_left|top|top_right|left|center|right|bottom_left|bottom|bottom_right}",
        mutates: true,
        run: set_origin,
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
    Ok(lightfoundry_gcode::generate(&job, &s.doc.machine, placement(s)?)?)
}

/// Machine placement for the document's "start from" setting.
fn placement(s: &Session) -> Result<Placement, EngineError> {
    let origin = s.doc.job_origin;
    if origin.start_from == StartFrom::Absolute {
        return Ok(Placement::Absolute);
    }
    let bounds = lightfoundry_toolpath::job_bounds(&s.doc).ok_or_else(|| EngineError::NotFound("there is nothing to output".into()))?;
    Ok(Placement::Relative {
        anchor: origin.anchor.point(bounds),
        extent: (bounds.width(), bounds.height()),
        set_zero: origin.start_from == StartFrom::CurrentPosition,
    })
}

#[derive(Deserialize)]
struct SetOriginParams {
    start_from: Option<StartFrom>,
    anchor: Option<Anchor>,
}

fn set_origin(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: SetOriginParams = parse(params)?;
    let o = &mut s.doc.job_origin;
    o.start_from = p.start_from.unwrap_or(o.start_from);
    o.anchor = p.anchor.unwrap_or(o.anchor);
    Ok(json!({ "start_from": o.start_from, "anchor": o.anchor }))
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
    let program = lightfoundry_gcode::frame(bounds, &s.doc.machine, placement(s)?, p.speed, p.power)?;
    Ok(json!({ "gcode": program, "bounds": [bounds.x0, bounds.y0, bounds.x1, bounds.y1] }))
}

fn simulate(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    let program = generate(s)?;
    let mut sim = SimulatedGrbl::new();
    let report = stream_program(&mut sim, &program, &StreamOptions::default(), &std::sync::atomic::AtomicBool::new(false), |_| {})?;
    Ok(json!({ "lines_streamed": report.lines, "peak_rx_bytes": sim.max_buffer_used(), "overran_buffer": sim.overran_buffer() }))
}
