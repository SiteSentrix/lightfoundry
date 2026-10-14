use std::sync::Arc;

use lightfoundry_doc::{ShapeId, ShapeKind};
use lightfoundry_geom::{Affine, Vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "image.adjust",
        label: "Adjust Image",
        menu: "Edit",
        shortcut: None,
        params_doc: "{id? (default: the selected image), brightness? (-1..1), contrast? (-1..1), gamma? (0.1..5), negative?: bool, reset?: bool}  — tone is applied before dithering; the picture itself is never changed",
        mutates: true,
        run: adjust,
    },
    CommandSpec {
        id: "image.crop",
        label: "Crop Image",
        menu: "Edit",
        shortcut: None,
        params_doc: "{id?, left?, top?, right?, bottom?}  — fractions (0..0.95) of the picture to cut off each side; the rest stays where it is",
        mutates: true,
        run: crop,
    },
    CommandSpec {
        id: "image.sharpen",
        label: "Sharpen Image",
        menu: "Edit",
        shortcut: None,
        params_doc: "{id?, amount (0..5)}  — unsharp mask applied to the stored picture",
        mutates: true,
        run: sharpen,
    },
];

#[derive(Deserialize)]
struct CropParams {
    id: Option<ShapeId>,
    #[serde(default)]
    left: f64,
    #[serde(default)]
    top: f64,
    #[serde(default)]
    right: f64,
    #[serde(default)]
    bottom: f64,
}

/// Fraction of a side that may be cut off; the opposite sides together must leave something.
const MAX_CROP: f64 = 0.95;

fn crop(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: CropParams = parse(params)?;
    let id = target_image(s, p.id)?;
    let sides = [p.left, p.top, p.right, p.bottom];
    if sides.iter().any(|v| !(v.is_finite() && (0.0..=MAX_CROP).contains(v))) || p.left + p.right >= 1.0 || p.top + p.bottom >= 1.0 {
        return Err(EngineError::BadParams(format!("crop fractions must be between 0 and {MAX_CROP}, and opposite sides must leave some picture")));
    }
    let shape = s.doc.shape_mut(id).ok_or_else(|| EngineError::NotFound(format!("shape {id} does not exist")))?;
    let ShapeKind::Image { image, width_mm, height_mm, .. } = &mut shape.kind else { return Err(not_an_image(id)) };
    let (w, h) = (image.width, image.height);
    let px = |frac: f64, len: u32| (frac * f64::from(len)).round() as u32;
    let (x0, y0) = (px(p.left, w), px(p.top, h));
    let (x1, y1) = (w.saturating_sub(px(p.right, w)), h.saturating_sub(px(p.bottom, h)));
    let cropped = image.crop(x0, y0, x1, y1).ok_or_else(|| EngineError::BadParams("that crop leaves no pixels".into()))?;
    let (mm_per_x, mm_per_y) = (*width_mm / f64::from(w), *height_mm / f64::from(h));
    let (new_w, new_h) = (f64::from(cropped.width) * mm_per_x, f64::from(cropped.height) * mm_per_y);
    let shift = Vec2::new(f64::from(x0) * mm_per_x, f64::from(y0) * mm_per_y);
    *image = Arc::new(cropped);
    *width_mm = new_w;
    *height_mm = new_h;
    // What remains stays exactly where it was on the bed.
    shape.transform *= Affine::translate(shift);
    Ok(json!({ "width_mm": new_w, "height_mm": new_h }))
}

#[derive(Deserialize)]
struct SharpenParams {
    id: Option<ShapeId>,
    amount: f64,
}

fn sharpen(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: SharpenParams = parse(params)?;
    let id = target_image(s, p.id)?;
    let amount = in_range("amount", p.amount, 0.0, 5.0)?;
    let shape = s.doc.shape_mut(id).ok_or_else(|| EngineError::NotFound(format!("shape {id} does not exist")))?;
    let ShapeKind::Image { image, .. } = &mut shape.kind else { return Err(not_an_image(id)) };
    *image = Arc::new(image.sharpened(amount));
    Ok(json!({ "amount": amount }))
}

/// The image a command acts on: the given id, else the only selected shape.
fn target_image(s: &Session, id: Option<ShapeId>) -> Result<ShapeId, EngineError> {
    match id {
        Some(id) => Ok(id),
        None => match s.selection.as_slice() {
            [only] => Ok(*only),
            _ => Err(EngineError::BadParams("select exactly one image, or pass an id".into())),
        },
    }
}

fn not_an_image(id: ShapeId) -> EngineError {
    EngineError::BadParams(format!("shape {id} is not an image"))
}

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
