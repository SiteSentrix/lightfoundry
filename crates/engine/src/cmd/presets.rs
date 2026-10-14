use lightfoundry_doc::{LayerId, MAX_PRESETS, Preset};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse};
use crate::{EngineError, Session};

/// Longest preset name kept, in characters.
const MAX_NAME: usize = 60;

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "preset.save",
        label: "Save Cut Settings as Preset",
        menu: "Laser",
        shortcut: None,
        params_doc: "{name, layer}  — stores the layer's cut settings under that name (replacing a preset of the same name)",
        mutates: true,
        run: save,
    },
    CommandSpec {
        id: "preset.apply",
        label: "Apply Preset",
        menu: "Laser",
        shortcut: None,
        params_doc: "{name, layer}  — copies the preset's cut settings onto the layer (its name, colour and output switch stay)",
        mutates: true,
        run: apply,
    },
    CommandSpec { id: "preset.delete", label: "Delete Preset", menu: "Laser", shortcut: None, params_doc: "{name}", mutates: true, run: delete },
];

#[derive(Deserialize)]
struct NamedLayer {
    name: String,
    layer: LayerId,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

fn clean(name: &str) -> Result<String, EngineError> {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    if name.is_empty() { Err(EngineError::BadParams("a preset needs a name".into())) } else { Ok(name) }
}

fn save(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: NamedLayer = parse(params)?;
    let name = clean(&p.name)?;
    let settings = s.doc.layer(p.layer).ok_or_else(|| EngineError::NotFound(format!("layer {} does not exist", p.layer)))?.clone();
    match s.doc.presets.iter_mut().find(|x| x.name == name) {
        Some(existing) => existing.settings = settings,
        None => {
            if s.doc.presets.len() >= MAX_PRESETS {
                return Err(EngineError::BadParams(format!("at most {MAX_PRESETS} presets can be kept")));
            }
            s.doc.presets.push(Preset { name: name.clone(), settings });
        }
    }
    Ok(json!({ "name": name, "count": s.doc.presets.len() }))
}

fn apply(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: NamedLayer = parse(params)?;
    let name = clean(&p.name)?;
    let preset =
        s.doc.presets.iter().find(|x| x.name == name).ok_or_else(|| EngineError::NotFound(format!("no preset called {name}")))?.settings.clone();
    let layer = s.doc.layer_mut(p.layer).ok_or_else(|| EngineError::NotFound(format!("layer {} does not exist", p.layer)))?;
    layer.apply_settings(&preset);
    Ok(json!({ "layer": p.layer }))
}

fn delete(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: Named = parse(params)?;
    let name = clean(&p.name)?;
    let before = s.doc.presets.len();
    s.doc.presets.retain(|x| x.name != name);
    if s.doc.presets.len() == before {
        return Err(EngineError::NotFound(format!("no preset called {name}")));
    }
    Ok(json!({ "count": s.doc.presets.len() }))
}
