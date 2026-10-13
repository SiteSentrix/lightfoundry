use lightfoundry_doc::{LayerId, ShapeId};
use lightfoundry_geom::{Affine, Rect};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, id_list, parse, shapes::finite};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.select",
        label: "Select",
        menu: "Edit",
        shortcut: None,
        params_doc: "{ids: [id]}  (empty clears)",
        mutates: false,
        run: select,
    },
    CommandSpec {
        id: "edit.select_all",
        label: "Select All",
        menu: "Edit",
        shortcut: Some("Ctrl+A"),
        params_doc: "{}",
        mutates: false,
        run: select_all,
    },
    CommandSpec {
        id: "edit.delete",
        label: "Delete",
        menu: "Edit",
        shortcut: Some("Delete"),
        params_doc: "{ids?}  (default: selection)",
        mutates: true,
        run: delete,
    },
    CommandSpec {
        id: "edit.duplicate",
        label: "Duplicate",
        menu: "Edit",
        shortcut: Some("Ctrl+D"),
        params_doc: "{ids?, dx?, dy?}",
        mutates: true,
        run: duplicate,
    },
    CommandSpec { id: "edit.move", label: "Move", menu: "Arrange", shortcut: None, params_doc: "{ids?, dx, dy}  (mm)", mutates: true, run: move_by },
    CommandSpec {
        id: "edit.move_to",
        label: "Move To",
        menu: "Arrange",
        shortcut: None,
        params_doc: "{ids?, x, y}  (top-left of selection bounds, mm)",
        mutates: true,
        run: move_to,
    },
    CommandSpec {
        id: "edit.resize",
        label: "Resize",
        menu: "Arrange",
        shortcut: None,
        params_doc: "{ids?, width?, height?, lock_aspect?}  (mm, about the top-left)",
        mutates: true,
        run: resize,
    },
    CommandSpec {
        id: "edit.rotate",
        label: "Rotate",
        menu: "Arrange",
        shortcut: None,
        params_doc: "{ids?, degrees}  (about the selection centre)",
        mutates: true,
        run: rotate,
    },
    CommandSpec {
        id: "edit.flip",
        label: "Flip",
        menu: "Arrange",
        shortcut: None,
        params_doc: "{ids?, axis: \"horizontal\"|\"vertical\"}",
        mutates: true,
        run: flip,
    },
    CommandSpec {
        id: "edit.align",
        label: "Align",
        menu: "Arrange/Align",
        shortcut: None,
        params_doc: "{ids?, to: left|right|top|bottom|center_x|center_y}",
        mutates: true,
        run: align,
    },
    CommandSpec {
        id: "edit.set_layer",
        label: "Move to Layer",
        menu: "Arrange",
        shortcut: None,
        params_doc: "{ids?, layer}",
        mutates: true,
        run: set_layer,
    },
    CommandSpec { id: "edit.undo", label: "Undo", menu: "Edit", shortcut: Some("Ctrl+Z"), params_doc: "{}", mutates: false, run: undo },
    CommandSpec { id: "edit.redo", label: "Redo", menu: "Edit", shortcut: Some("Ctrl+Shift+Z"), params_doc: "{}", mutates: false, run: redo },
    CommandSpec {
        id: "command.batch",
        label: "Batch",
        menu: "",
        shortcut: None,
        params_doc: "{commands: [{id, params}]}  — all or nothing, one undo step",
        mutates: true,
        run: batch,
    },
];

#[derive(Deserialize, Default)]
struct Ids {
    ids: Option<Vec<ShapeId>>,
}

fn select(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: Ids = parse(params)?;
    let ids = p.ids.unwrap_or_default();
    s.selection = ids.into_iter().filter(|id| s.doc.shape(*id).is_some()).collect();
    Ok(json!({ "selected": s.selection }))
}

fn select_all(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    s.selection = s.doc.shapes.iter().map(|sh| sh.id).collect();
    Ok(json!({ "selected": s.selection }))
}

fn delete(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let ids = targets(s, &parse::<Ids>(params)?)?;
    s.doc.remove_shapes(&ids);
    s.selection.retain(|id| !ids.contains(id));
    Ok(json!({ "deleted": ids.len() }))
}

#[derive(Deserialize)]
struct DuplicateParams {
    #[serde(flatten)]
    ids: Ids,
    #[serde(default = "default_offset")]
    dx: f64,
    #[serde(default = "default_offset")]
    dy: f64,
}

fn default_offset() -> f64 {
    5.0
}

fn duplicate(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: DuplicateParams = parse(params)?;
    finite(&[p.dx, p.dy])?;
    let ids = targets(s, &p.ids)?;
    let mut created = Vec::new();
    for id in ids {
        if let Some(mut copy) = s.doc.shape(id).cloned() {
            copy.transform = Affine::translate((p.dx, p.dy)) * copy.transform;
            created.push(s.doc.add_shape(copy));
        }
    }
    s.selection = created.clone();
    Ok(json!({ "ids": created }))
}

#[derive(Deserialize)]
struct MoveParams {
    #[serde(flatten)]
    ids: Ids,
    dx: f64,
    dy: f64,
}

fn move_by(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: MoveParams = parse(params)?;
    finite(&[p.dx, p.dy])?;
    let ids = targets(s, &p.ids)?;
    transform_all(s, &ids, Affine::translate((p.dx, p.dy)));
    Ok(json!({ "moved": ids.len() }))
}

#[derive(Deserialize)]
struct MoveToParams {
    #[serde(flatten)]
    ids: Ids,
    x: f64,
    y: f64,
}

fn move_to(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: MoveToParams = parse(params)?;
    finite(&[p.x, p.y])?;
    let ids = targets(s, &p.ids)?;
    let b = bounds(s, &ids)?;
    transform_all(s, &ids, Affine::translate((p.x - b.x0, p.y - b.y0)));
    Ok(json!({ "moved": ids.len() }))
}

#[derive(Deserialize)]
struct ResizeParams {
    #[serde(flatten)]
    ids: Ids,
    width: Option<f64>,
    height: Option<f64>,
    #[serde(default)]
    lock_aspect: bool,
}

fn resize(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: ResizeParams = parse(params)?;
    let ids = targets(s, &p.ids)?;
    let b = bounds(s, &ids)?;
    let (w, h) = (b.width(), b.height());
    let ratio = |new: Option<f64>, old: f64| match new {
        Some(n) if n.is_finite() && n > 0.0 && n < 1e6 && old > 1e-9 => Ok(Some(n / old)),
        Some(_) => Err(EngineError::BadParams("sizes must be positive, finite and the shape must have extent in that direction".into())),
        None => Ok(None),
    };
    let (rx, ry) = (ratio(p.width, w)?, ratio(p.height, h)?);
    let (sx, sy) = match (rx, ry, p.lock_aspect) {
        (Some(x), None, true) => (x, x),
        (None, Some(y), true) => (y, y),
        (x, y, _) => (x.unwrap_or(1.0), y.unwrap_or(1.0)),
    };
    let m = Affine::translate((b.x0, b.y0)) * Affine::scale_non_uniform(sx, sy) * Affine::translate((-b.x0, -b.y0));
    transform_all(s, &ids, m);
    Ok(json!({ "width": w * sx, "height": h * sy }))
}

#[derive(Deserialize)]
struct RotateParams {
    #[serde(flatten)]
    ids: Ids,
    degrees: f64,
}

fn rotate(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: RotateParams = parse(params)?;
    finite(&[p.degrees])?;
    let ids = targets(s, &p.ids)?;
    let c = bounds(s, &ids)?.center();
    transform_all(s, &ids, Affine::rotate_about(p.degrees.to_radians(), c));
    Ok(json!({ "rotated": ids.len() }))
}

#[derive(Deserialize)]
struct FlipParams {
    #[serde(flatten)]
    ids: Ids,
    axis: Axis,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Axis {
    Horizontal,
    Vertical,
}

fn flip(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: FlipParams = parse(params)?;
    let ids = targets(s, &p.ids)?;
    let c = bounds(s, &ids)?.center();
    let (sx, sy) = match p.axis {
        Axis::Horizontal => (-1.0, 1.0),
        Axis::Vertical => (1.0, -1.0),
    };
    transform_all(s, &ids, Affine::translate(c.to_vec2()) * Affine::scale_non_uniform(sx, sy) * Affine::translate(-c.to_vec2()));
    Ok(json!({ "flipped": ids.len() }))
}

#[derive(Deserialize)]
struct AlignParams {
    #[serde(flatten)]
    ids: Ids,
    to: Align,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Align {
    Left,
    Right,
    Top,
    Bottom,
    CenterX,
    CenterY,
}

/// Aligns each shape to the bounds of the whole selection.
fn align(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: AlignParams = parse(params)?;
    let ids = targets(s, &p.ids)?;
    let all = bounds(s, &ids)?;
    for id in &ids {
        let Some(b) = s.doc.shape(*id).and_then(|sh| sh.world_bounds()) else { continue };
        let (dx, dy) = match p.to {
            Align::Left => (all.x0 - b.x0, 0.0),
            Align::Right => (all.x1 - b.x1, 0.0),
            Align::Top => (0.0, all.y0 - b.y0),
            Align::Bottom => (0.0, all.y1 - b.y1),
            Align::CenterX => (all.center().x - b.center().x, 0.0),
            Align::CenterY => (0.0, all.center().y - b.center().y),
        };
        transform_all(s, &[*id], Affine::translate((dx, dy)));
    }
    Ok(json!({ "aligned": ids.len() }))
}

#[derive(Deserialize)]
struct SetLayerParams {
    #[serde(flatten)]
    ids: Ids,
    layer: LayerId,
}

fn set_layer(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: SetLayerParams = parse(params)?;
    if s.doc.layer(p.layer).is_none() {
        return Err(EngineError::NotFound(format!("layer {} does not exist", p.layer)));
    }
    let ids = targets(s, &p.ids)?;
    for id in &ids {
        if let Some(sh) = s.doc.shape_mut(*id) {
            sh.layer = p.layer;
        }
    }
    Ok(json!({ "moved": ids.len() }))
}

fn undo(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    Ok(json!({ "undone": s.undo() }))
}

fn redo(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    Ok(json!({ "redone": s.redo() }))
}

#[derive(Deserialize)]
struct BatchParams {
    commands: Vec<BatchItem>,
}

#[derive(Deserialize)]
struct BatchItem {
    id: String,
    #[serde(default)]
    params: Value,
}

fn batch(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    const MAX_BATCH: usize = 10_000;
    let p: BatchParams = parse(params)?;
    if p.commands.len() > MAX_BATCH {
        return Err(EngineError::BadParams(format!("a batch may hold at most {MAX_BATCH} commands")));
    }
    let mut results = Vec::with_capacity(p.commands.len());
    for item in &p.commands {
        if item.id == "command.batch" {
            return Err(EngineError::BadParams("batches cannot be nested".into()));
        }
        results.push(s.run_nested(&item.id, &item.params)?);
    }
    Ok(json!({ "results": results }))
}

fn targets(s: &Session, p: &Ids) -> Result<Vec<ShapeId>, EngineError> {
    let ids: Vec<ShapeId> = id_list(&p.ids, &s.selection).into_iter().filter(|id| s.doc.shape(*id).is_some()).collect();
    if ids.is_empty() { Err(EngineError::NothingSelected) } else { Ok(ids) }
}

fn bounds(s: &Session, ids: &[ShapeId]) -> Result<Rect, EngineError> {
    s.doc.bounds_of(ids).ok_or(EngineError::NothingSelected)
}

fn transform_all(s: &mut Session, ids: &[ShapeId], m: Affine) {
    for id in ids {
        if let Some(sh) = s.doc.shape_mut(*id) {
            sh.transform = m * sh.transform;
        }
    }
}
