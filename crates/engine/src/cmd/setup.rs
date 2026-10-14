use lightfoundry_doc::{Dither, LayerId, LayerMode, Origin};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "layer.set",
        label: "Cut Settings",
        menu: "Laser",
        shortcut: None,
        params_doc: "{id, name?, color? (#rrggbb), mode?: line|fill|fill_and_line, speed? (mm/min), min_power?, max_power? (%), passes?, interval? (mm), scan_angle?, kerf_offset? (mm; closed outlines are offset before cutting, + grows parts and shrinks holes), tab_count? (per outer outline), tab_width? (mm), lead_in?/lead_out? (mm, on the scrap side), bidirectional?, dither?: threshold|floyd_steinberg|grayscale, output?, air_assist?}",
        mutates: true,
        run: layer_set,
    },
    CommandSpec {
        id: "layer.move",
        label: "Reorder Layer",
        menu: "Laser",
        shortcut: None,
        params_doc: "{id, index}  — moves the layer to this position in the list; layers are cut in list order",
        mutates: true,
        run: layer_move,
    },
    CommandSpec {
        id: "machine.set",
        label: "Device Settings",
        menu: "Laser",
        shortcut: None,
        params_doc: "{name?, bed_width?, bed_height?, origin?: bottom_left|top_left, max_s?, dynamic_power?, rapid_speed?, overscan?}",
        mutates: true,
        run: machine_set,
    },
];

#[derive(Deserialize)]
struct LayerParams {
    id: LayerId,
    name: Option<String>,
    color: Option<String>,
    mode: Option<LayerMode>,
    speed: Option<f64>,
    min_power: Option<f64>,
    max_power: Option<f64>,
    passes: Option<u32>,
    interval: Option<f64>,
    scan_angle: Option<f64>,
    kerf_offset: Option<f64>,
    tab_count: Option<u32>,
    tab_width: Option<f64>,
    lead_in: Option<f64>,
    lead_out: Option<f64>,
    bidirectional: Option<bool>,
    dither: Option<Dither>,
    output: Option<bool>,
    air_assist: Option<bool>,
}

fn layer_set(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: LayerParams = parse(params)?;
    let layer = s.doc.layer_mut(p.id).ok_or_else(|| EngineError::NotFound(format!("layer {} does not exist", p.id)))?;
    if let Some(v) = p.speed {
        layer.speed = in_range("speed", v, 1.0, 100_000.0)?;
    }
    if let Some(v) = p.min_power {
        layer.min_power = in_range("min_power", v, 0.0, 100.0)?;
    }
    if let Some(v) = p.max_power {
        layer.max_power = in_range("max_power", v, 0.0, 100.0)?;
    }
    if let Some(v) = p.interval {
        layer.interval = in_range("interval", v, 0.005, 10.0)?;
    }
    if let Some(v) = p.scan_angle {
        layer.scan_angle = in_range("scan_angle", v, -360.0, 360.0)?;
    }
    if let Some(v) = p.kerf_offset {
        layer.kerf_offset = in_range("kerf_offset", v, -50.0, 50.0)?;
    }
    if let Some(v) = p.tab_width {
        layer.tab_width = in_range("tab_width", v, 0.0, 1000.0)?;
    }
    if let Some(v) = p.lead_in {
        layer.lead_in = in_range("lead_in", v, 0.0, 1000.0)?;
    }
    if let Some(v) = p.lead_out {
        layer.lead_out = in_range("lead_out", v, 0.0, 1000.0)?;
    }
    if let Some(v) = p.tab_count {
        layer.tab_count = v.min(1000);
    }
    if let Some(v) = p.passes {
        layer.passes = v.clamp(1, 1000);
    }
    if let Some(v) = p.name {
        layer.name = v.chars().take(64).collect();
    }
    if let Some(v) = p.color {
        let ok = v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !ok {
            return Err(EngineError::BadParams("color must look like #rrggbb".into()));
        }
        layer.color = v.to_ascii_lowercase();
    }
    layer.mode = p.mode.unwrap_or(layer.mode);
    layer.dither = p.dither.unwrap_or(layer.dither);
    layer.bidirectional = p.bidirectional.unwrap_or(layer.bidirectional);
    layer.output = p.output.unwrap_or(layer.output);
    layer.air_assist = p.air_assist.unwrap_or(layer.air_assist);
    if layer.min_power > layer.max_power {
        layer.min_power = layer.max_power;
    }
    Ok(json!({ "id": layer.id }))
}

#[derive(Deserialize)]
struct MoveLayerParams {
    id: LayerId,
    index: usize,
}

fn layer_move(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: MoveLayerParams = parse(params)?;
    let from = s.doc.layers.iter().position(|l| l.id == p.id).ok_or_else(|| EngineError::NotFound(format!("layer {} does not exist", p.id)))?;
    if p.index >= s.doc.layers.len() {
        return Err(EngineError::BadParams(format!("index must be below {}", s.doc.layers.len())));
    }
    let layer = s.doc.layers.remove(from);
    s.doc.layers.insert(p.index, layer);
    Ok(json!({ "index": p.index }))
}

#[derive(Deserialize)]
struct MachineParams {
    name: Option<String>,
    bed_width: Option<f64>,
    bed_height: Option<f64>,
    origin: Option<Origin>,
    max_s: Option<u32>,
    dynamic_power: Option<bool>,
    rapid_speed: Option<f64>,
    overscan: Option<f64>,
}

fn machine_set(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: MachineParams = parse(params)?;
    let m = &mut s.doc.machine;
    if let Some(v) = p.bed_width {
        m.bed_width = in_range("bed_width", v, 1.0, 100_000.0)?;
    }
    if let Some(v) = p.bed_height {
        m.bed_height = in_range("bed_height", v, 1.0, 100_000.0)?;
    }
    if let Some(v) = p.rapid_speed {
        m.rapid_speed = in_range("rapid_speed", v, 1.0, 1_000_000.0)?;
    }
    if let Some(v) = p.overscan {
        m.overscan = in_range("overscan", v, 0.0, 50.0)?;
    }
    if let Some(v) = p.max_s {
        if !(1..=100_000).contains(&v) {
            return Err(EngineError::BadParams("max_s must be between 1 and 100000".into()));
        }
        m.max_s = v;
    }
    if let Some(v) = p.name {
        m.name = v.chars().take(80).collect();
    }
    m.origin = p.origin.unwrap_or(m.origin);
    m.dynamic_power = p.dynamic_power.unwrap_or(m.dynamic_power);
    Ok(json!({ "bed": [m.bed_width, m.bed_height] }))
}

fn in_range(name: &str, v: f64, lo: f64, hi: f64) -> Result<f64, EngineError> {
    if v.is_finite() && (lo..=hi).contains(&v) { Ok(v) } else { Err(EngineError::BadParams(format!("{name} must be between {lo} and {hi}"))) }
}
