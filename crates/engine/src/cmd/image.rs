use lightfoundry_doc::{ShapeId, ShapeKind};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "image.adjust",
    label: "Adjust Image",
    menu: "Edit",
    shortcut: None,
    params_doc: "{id? (default: the selected image), brightness? (-1..1), contrast? (-1..1), gamma? (0.1..5), negative?: bool, reset?: bool}  — tone is applied before dithering; the picture itself is never changed",
    mutates: true,
    run: adjust,
}];

#[derive(Deserialize)]
struct AdjustParams {
    id: Option<ShapeId>,
    brightness: Option<f64>,
    contrast: Option<f64>,
    gamma: Option<f64>,
    negative: Option<bool>,
    #[serde(default)]
    reset: bool,
}

fn adjust(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: AdjustParams = parse(params)?;
    let id = match p.id {
        Some(id) => id,
        None => match s.selection.as_slice() {
            [only] => *only,
            _ => return Err(EngineError::BadParams("select exactly one image, or pass an id".into())),
        },
    };
    let shape = s.doc.shape_mut(id).ok_or_else(|| EngineError::NotFound(format!("shape {id} does not exist")))?;
    let ShapeKind::Image { adjust, .. } = &mut shape.kind else {
        return Err(EngineError::BadParams(format!("shape {id} is not an image")));
    };
    let mut next = if p.reset { Default::default() } else { *adjust };
    if let Some(v) = p.brightness {
        next.brightness = in_range("brightness", v, -1.0, 1.0)?;
    }
    if let Some(v) = p.contrast {
        next.contrast = in_range("contrast", v, -1.0, 1.0)?;
    }
    if let Some(v) = p.gamma {
        next.gamma = in_range("gamma", v, 0.1, 5.0)?;
    }
    next.negative = p.negative.unwrap_or(next.negative);
    *adjust = next;
    Ok(json!({ "brightness": next.brightness, "contrast": next.contrast, "gamma": next.gamma, "negative": next.negative }))
}

fn in_range(name: &str, v: f64, lo: f64, hi: f64) -> Result<f64, EngineError> {
    if v.is_finite() && (lo..=hi).contains(&v) { Ok(v) } else { Err(EngineError::BadParams(format!("{name} must be between {lo} and {hi}"))) }
}
