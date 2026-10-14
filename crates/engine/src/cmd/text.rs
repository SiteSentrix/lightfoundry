use lightfoundry_doc::{LayerId, Shape, ShapeId, ShapeKind, TextAlign, TextSettings};
use lightfoundry_geom::Affine;
use lightfoundry_text::{Align, TextStyle, layout};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse, shapes::finite};
use crate::{EngineError, Session};

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "text.create",
        label: "Text",
        menu: "Create",
        shortcut: Some("T"),
        params_doc: "{text, x, y, height? (cap height mm, default 10), letter_spacing?, line_spacing?, align?: left|center|right, layer?}  (single-line stroke font), font? (path to a .ttf/.otf file: draws filled outlines; use a Fill layer to engrave them), vertical? (stack letters top to bottom, one column per line, first line rightmost), fallback_font? (path to a second .ttf/.otf for characters the font lacks)",
        mutates: true,
        run: create,
    },
    CommandSpec {
        id: "text.on_path",
        label: "Text on Path",
        menu: "Create",
        shortcut: None,
        params_doc: "{text: id of a text shape, path: id of the shape to follow, offset? (mm along the path, default 0), keep? (keep the originals, default true)}  — makes a new path shape with the letters bent along the line; it is plain geometry afterwards, not editable text",
        mutates: true,
        run: on_path,
    },
    CommandSpec {
        id: "text.edit",
        label: "Edit Text",
        menu: "Edit",
        shortcut: None,
        params_doc: "{id? (default: the selected text), text?, height?, letter_spacing?, line_spacing?, align?, font? (path, or \"\" for the built-in font), vertical?, fallback_font? (path, or \"\" for none)}",
        mutates: true,
        run: edit,
    },
];

#[derive(Deserialize)]
struct CreateParams {
    text: String,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    height: Option<f64>,
    letter_spacing: Option<f64>,
    line_spacing: Option<f64>,
    align: Option<TextAlign>,
    font: Option<String>,
    vertical: Option<bool>,
    fallback_font: Option<String>,
    #[serde(default)]
    layer: LayerId,
}

fn create(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: CreateParams = parse(params)?;
    finite(&[p.x, p.y])?;
    let defaults = TextSettings::default();
    let settings = TextSettings {
        height: p.height.unwrap_or(defaults.height),
        letter_spacing: p.letter_spacing.unwrap_or(defaults.letter_spacing),
        line_spacing: p.line_spacing.unwrap_or(defaults.line_spacing),
        align: p.align.unwrap_or(defaults.align),
        font: p.font.filter(|f| !f.is_empty()),
        vertical: p.vertical.unwrap_or(defaults.vertical),
        fallback_font: p.fallback_font.filter(|f| !f.is_empty()),
    };
    let (path, unsupported) = generate(&p.text, &settings)?;
    let name = p.text.lines().next().unwrap_or("Text").chars().take(24).collect::<String>();
    let shape = Shape {
        id: 0,
        name,
        layer: p.layer,
        transform: Affine::translate((p.x, p.y)),
        kind: ShapeKind::Text { content: p.text, style: settings, path },
        cut: None,
        group: None,
        order: 0,
    };
    let id = s.doc.add_shape(shape);
    s.selection = vec![id];
    Ok(json!({ "id": id, "unsupported": unsupported }))
}

#[derive(Deserialize)]
struct EditParams {
    id: Option<ShapeId>,
    text: Option<String>,
    height: Option<f64>,
    letter_spacing: Option<f64>,
    line_spacing: Option<f64>,
    align: Option<TextAlign>,
    font: Option<String>,
    vertical: Option<bool>,
    fallback_font: Option<String>,
}

fn edit(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: EditParams = parse(params)?;
    let id = match p.id {
        Some(id) => id,
        None => match s.selection.as_slice() {
            [only] => *only,
            _ => return Err(EngineError::BadParams("select exactly one text object, or pass an id".into())),
        },
    };
    let shape = s.doc.shape_mut(id).ok_or_else(|| EngineError::NotFound(format!("shape {id} does not exist")))?;
    let ShapeKind::Text { content, style, path } = &mut shape.kind else {
        return Err(EngineError::BadParams(format!("shape {id} is not text")));
    };
    let mut new_style = style.clone();
    new_style.height = p.height.unwrap_or(new_style.height);
    new_style.letter_spacing = p.letter_spacing.unwrap_or(new_style.letter_spacing);
    new_style.line_spacing = p.line_spacing.unwrap_or(new_style.line_spacing);
    new_style.align = p.align.unwrap_or(new_style.align);
    if let Some(f) = p.font {
        new_style.font = (!f.is_empty()).then_some(f);
    }
    new_style.vertical = p.vertical.unwrap_or(new_style.vertical);
    if let Some(f) = p.fallback_font {
        new_style.fallback_font = (!f.is_empty()).then_some(f);
    }
    let new_content = p.text.unwrap_or_else(|| content.clone());
    let (new_path, unsupported) = generate(&new_content, &new_style)?;
    *content = new_content;
    *style = new_style;
    *path = new_path;
    Ok(json!({ "id": id, "unsupported": unsupported }))
}

pub(super) fn generate(text: &str, s: &TextSettings) -> Result<(lightfoundry_geom::BezPath, Vec<char>), EngineError> {
    if text.trim().is_empty() {
        return Err(EngineError::BadParams("text is empty".into()));
    }
    let style = TextStyle {
        height: s.height,
        letter_spacing: s.letter_spacing,
        line_spacing: s.line_spacing,
        align: match s.align {
            TextAlign::Left => Align::Left,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::Right,
        },
    };
    let font = s.font.as_deref().map(read_font).transpose()?;
    let fallback = match (&font, s.fallback_font.as_deref()) {
        (Some(_), Some(path)) => Some(read_font(path)?),
        _ => None,
    };
    let one = |t: &str, st: &TextStyle| match &font {
        Some(data) => lightfoundry_text::layout_outline_with_fallback(data, fallback.as_deref(), t, st),
        None => layout(t, st),
    };
    let l = if s.vertical { lightfoundry_text::layout_vertical(text, &style, |c| one(c, &style)) } else { one(text, &style) }
        .map_err(|e| EngineError::BadParams(e.to_string()))?;
    Ok((l.path, l.unsupported))
}

/// Reads a font file, refusing anything unreasonably large before it is loaded.
fn read_font(path: &str) -> Result<Vec<u8>, EngineError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| EngineError::BadParams(format!("cannot open the font {path}: {e}")))?;
    let mut data = Vec::new();
    file.take(lightfoundry_text::MAX_FONT_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| EngineError::BadParams(format!("cannot read the font {path}: {e}")))?;
    if data.len() > lightfoundry_text::MAX_FONT_BYTES {
        return Err(EngineError::BadParams("the font file is too large".into()));
    }
    Ok(data)
}

#[derive(Deserialize)]
struct OnPathParams {
    text: ShapeId,
    path: ShapeId,
    #[serde(default)]
    offset: f64,
    #[serde(default = "yes")]
    keep: bool,
}

fn yes() -> bool {
    true
}

/// Most points the bent text may have.
const MAX_BENT_POINTS: usize = 2_000_000;

fn on_path(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: OnPathParams = parse(params)?;
    finite(&[p.offset])?;
    if p.text == p.path {
        return Err(EngineError::BadParams("the text and the path must be different shapes".into()));
    }
    let text = s.doc.shape(p.text).ok_or_else(|| EngineError::NotFound(format!("shape {} does not exist", p.text)))?;
    let ShapeKind::Text { style, path, .. } = &text.kind else {
        return Err(EngineError::BadParams(format!("shape {} is not text", p.text)));
    };
    let guide = s.doc.shape(p.path).ok_or_else(|| EngineError::NotFound(format!("shape {} does not exist", p.path)))?;
    // The longest line of the guide shape, in workspace mm.
    let line = guide
        .world_polylines()
        .into_iter()
        .max_by(|a, b| a.length().total_cmp(&b.length()))
        .ok_or_else(|| EngineError::BadParams("the path has no geometry".into()))?;
    let along = lightfoundry_geom::along::Along::new(&line).ok_or_else(|| EngineError::BadParams("the path is too short to follow".into()))?;

    // Letters keep their own size (the text shape's scale) and their baseline sits on the line.
    let m = text.transform.as_coeffs();
    let (sx, sy) = (m[0].hypot(m[1]), m[2].hypot(m[3]));
    let baseline = style.height;
    let layer = text.layer;
    let mut bent = lightfoundry_geom::BezPath::new();
    let mut count = 0usize;
    for pl in lightfoundry_geom::flatten(path, BEND_TOLERANCE_MM) {
        count += pl.points.len();
        if count > MAX_BENT_POINTS {
            return Err(EngineError::BadParams("that text is too large to bend".into()));
        }
        // Long straight edges are cut into short steps so they can follow a curve.
        let mut dense: Vec<lightfoundry_geom::Point> = Vec::with_capacity(pl.points.len());
        let n = pl.points.len();
        let edges = if pl.closed { n } else { n.saturating_sub(1) };
        for i in 0..edges {
            let (a, b) = (pl.points[i], pl.points[(i + 1) % n]);
            let steps = ((a.distance(b) * sx.max(sy) / BEND_STEP_MM).ceil() as usize).clamp(1, 10_000);
            count += steps;
            if count > MAX_BENT_POINTS {
                return Err(EngineError::BadParams("that text is too large to bend".into()));
            }
            dense.extend((0..steps).map(|k| a.lerp(b, k as f64 / steps as f64)));
        }
        if !pl.closed
            && let Some(last) = pl.points.last()
        {
            dense.push(*last);
        }
        for (i, pt) in dense.iter().enumerate() {
            let (at, dir) = along.at(p.offset + pt.x * sx);
            let up = lightfoundry_geom::Vec2::new(dir.y, -dir.x);
            let q = at + up * ((baseline - pt.y) * sy);
            if i == 0 {
                bent.move_to(q);
            } else {
                bent.line_to(q);
            }
        }
        if pl.closed {
            bent.close_path();
        }
    }
    let new_id = s.doc.add_shape(Shape::path("Text on path", layer, bent));
    if !p.keep {
        s.doc.shapes.retain(|x| x.id != p.text);
    }
    s.selection = vec![new_id];
    Ok(json!({ "id": new_id }))
}

/// Flattening tolerance used before bending, mm: curves are straightened finely enough that the
/// bent result stays smooth.
const BEND_TOLERANCE_MM: f64 = 0.02;
/// Longest straight step kept when bending, mm.
const BEND_STEP_MM: f64 = 0.5;
