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
        params_doc: "{text, x, y, height? (cap height mm, default 10), letter_spacing?, line_spacing?, align?: left|center|right, layer?}  (single-line stroke font)",
        mutates: true,
        run: create,
    },
    CommandSpec {
        id: "text.edit",
        label: "Edit Text",
        menu: "Edit",
        shortcut: None,
        params_doc: "{id? (default: the selected text), text?, height?, letter_spacing?, line_spacing?, align?}",
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
    let l = layout(text, &style).map_err(|e| EngineError::BadParams(e.to_string()))?;
    Ok((l.path, l.unsupported))
}
