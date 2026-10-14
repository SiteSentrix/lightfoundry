//! The controller settings editor: the `$$` table the machine reports, editable in place, plus
//! checks of those values against the document's machine setup.

use std::collections::BTreeMap;

use lightfoundry_device::settings::{Kind, describe};
use lightfoundry_doc::Machine;
use serde_json::json;

use crate::LightFoundryApp;

/// Differences between what the controller reports and what the document assumes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Advice {
    /// `$32=0`: without laser mode GRBL does not scale power during moves and `M4` is not dynamic.
    LaserModeOff,
    /// `$30` differs from the document's S max, so power percentages would come out wrong.
    SMaxDiffers(u32),
    /// `$130`/`$131` differ from the document's bed size.
    BedDiffers(f64, f64),
}

pub fn advice(settings: &BTreeMap<u32, String>, machine: &Machine) -> Vec<Advice> {
    let num = |id: u32| settings.get(&id).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    let mut out = Vec::new();
    if num(32).is_some_and(|v| v == 0.0) {
        out.push(Advice::LaserModeOff);
    }
    if let Some(s) = num(30)
        && s >= 1.0
        && s <= f64::from(u32::MAX)
        && (s - f64::from(machine.max_s)).abs() > 0.5
    {
        out.push(Advice::SMaxDiffers(s.round() as u32));
    }
    if let (Some(w), Some(h)) = (num(130), num(131))
        && w >= 1.0
        && h >= 1.0
        && ((w - machine.bed_width).abs() > 0.05 || (h - machine.bed_height).abs() > 0.05)
    {
        out.push(Advice::BedDiffers(w, h));
    }
    out
}

pub fn section(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.collapsing("Controller settings ($$)", |ui| {
        let busy = app.job_active();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Read from machine")).clicked() {
                app.link_do(|l| l.request_settings());
            }
            if busy {
                ui.weak("not while a job runs");
            }
        });
        let settings = app.snap.as_ref().map(|s| s.settings.clone()).unwrap_or_default();
        if settings.is_empty() {
            ui.weak("Press the button to read the controller's settings.");
            return;
        }
        for a in advice(&settings, &app.session.doc.machine) {
            advice_row(app, ui, a, busy);
        }
        let mut write = None;
        egui::Grid::new("controller_settings").num_columns(3).spacing([8.0, 3.0]).show(ui, |ui| {
            for (id, value) in &settings {
                let info = describe(*id);
                ui.label(format!("${id}")).on_hover_text(info.map_or("Not a documented GRBL 1.1 setting", |i| i.name));
                ui.label(info.map_or("(unknown)", |i| i.name));
                ui.add_enabled_ui(!busy, |ui| {
                    if let Some(v) = edit_value(ui, info.map_or(Kind::Number, |i| i.kind), value, info.map_or("", |i| i.unit)) {
                        write = Some((*id, v));
                    }
                });
                ui.end_row();
            }
        });
        if let Some((id, v)) = write {
            app.link_do(|l| l.write_setting(id, v));
        }
    });
}

fn advice_row(app: &mut LightFoundryApp, ui: &mut egui::Ui, a: Advice, busy: bool) {
    ui.horizontal_wrapped(|ui| match a {
        Advice::LaserModeOff => {
            ui.colored_label(app.tokens.error, "Laser mode ($32) is off.");
            if ui.add_enabled(!busy, egui::Button::new("Turn on")).clicked() {
                app.link_do(|l| l.write_setting(32, 1.0));
            }
        }
        Advice::SMaxDiffers(s) => {
            ui.label(format!("Controller S max is {s}; the document uses {}.", app.session.doc.machine.max_s));
            if ui.button("Use controller's").clicked() {
                app.run("machine.set", json!({ "max_s": s }));
            }
        }
        Advice::BedDiffers(w, h) => {
            ui.label(format!("Controller travel is {w} × {h} mm."));
            if ui.button("Use as bed size").clicked() {
                app.run("machine.set", json!({ "bed_width": w, "bed_height": h }));
                app.view = None;
            }
        }
    });
}

/// An editor for one value; returns the new value once the person finishes changing it.
fn edit_value(ui: &mut egui::Ui, kind: Kind, value: &str, unit: &str) -> Option<f64> {
    let current = value.parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
    match kind {
        Kind::Flag => {
            let mut on = current != 0.0;
            ui.checkbox(&mut on, "").changed().then(|| f64::from(u8::from(on)))
        }
        Kind::Mask | Kind::Number => {
            let mut v = current;
            let mut drag = egui::DragValue::new(&mut v).speed(0.1).max_decimals(4).range(0.0..=1.0e7);
            if kind == Kind::Mask {
                drag = drag.speed(1.0).max_decimals(0);
            } else if !unit.is_empty() && unit != "mask" {
                drag = drag.suffix(format!(" {unit}"));
            }
            let r = ui.add(drag);
            (crate::panels::committed(&r) && (v - current).abs() > 1e-9).then_some(v)
        }
    }
}
