use serde_json::{Value, json};

use crate::{LightFoundryApp, PendingAction};

pub fn menu_bar(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    egui::Panel::top("menu").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| file_menu(app, ui));
            ui.menu_button("Edit", |ui| edit_menu(app, ui));
            ui.menu_button("Arrange", |ui| arrange_menu(app, ui));
            ui.menu_button("Laser", |ui| laser_menu(app, ui));
            ui.menu_button("View", |ui| {
                if ui.button("Fit bed to window").clicked() {
                    app.view = None;
                    ui.close();
                }
                ui.checkbox(&mut app.show_preview, "Toolpath preview");
                ui.checkbox(&mut app.show_travel, "Show travel moves");
            });
        });
    });
}

fn file_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    if item(ui, "New", "Ctrl+N") {
        app.request(PendingAction::New);
    }
    if item(ui, "Open…", "Ctrl+O") {
        app.request(PendingAction::Open);
    }
    let can_save = true;
    if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
        save(app, false);
        ui.close();
    }
    if ui.button("Save As…").clicked() {
        save(app, true);
        ui.close();
    }
    ui.separator();
    if ui.button("Import SVG…").clicked() {
        import(app, "file.import_svg", "Import SVG", &["svg"]);
        ui.close();
    }
    if ui.button("Import DXF…").clicked() {
        import(app, "file.import_dxf", "Import DXF", &["dxf"]);
        ui.close();
    }
    if ui.button("Import Image…").clicked() {
        import(app, "file.import_image", "Import image", &["png", "jpg", "jpeg", "bmp", "gif", "webp"]);
        ui.close();
    }
    ui.separator();
    if ui.button("Save G-code…").clicked() {
        if let Some(path) = app.pick_save("Save G-code", "job.gcode", &["gcode", "nc"])
            && app.run("file.export_gcode", json!({ "path": path })).is_some()
        {
            app.say("G-code saved");
        }
        ui.close();
    }
}

fn edit_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let (can_undo, can_redo) = (app.session.can_undo(), app.session.can_redo());
    if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
        app.run("edit.undo", Value::Null);
        ui.close();
    }
    if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
        app.run("edit.redo", Value::Null);
        ui.close();
    }
    ui.separator();
    for (label, id) in [("Select All", "edit.select_all"), ("Duplicate", "edit.duplicate"), ("Delete", "edit.delete")] {
        if ui.add_enabled(id == "edit.select_all" || !app.session.selection.is_empty(), egui::Button::new(label)).clicked() {
            app.run(id, Value::Null);
            ui.close();
        }
    }
}

fn arrange_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.add_enabled_ui(!app.session.selection.is_empty(), |ui| {
        for (label, to) in [
            ("Align Left", "left"),
            ("Align Right", "right"),
            ("Align Top", "top"),
            ("Align Bottom", "bottom"),
            ("Center Horizontally", "center_x"),
            ("Center Vertically", "center_y"),
        ] {
            if ui.button(label).clicked() {
                app.run("edit.align", json!({ "to": to }));
                ui.close();
            }
        }
        ui.separator();
        ui.add_enabled_ui(app.session.selection.len() >= 2, |ui| {
            ui.menu_button("Boolean", |ui| {
                for (label, op) in [
                    ("Union (weld)", "union"),
                    ("Subtract (first minus the rest)", "difference"),
                    ("Intersect", "intersect"),
                    ("Exclusive or", "xor"),
                ] {
                    if ui.button(label).clicked() {
                        app.run("path.boolean", json!({ "op": op }));
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        if ui.add(egui::Button::new("Group").shortcut_text("Ctrl+G")).clicked() {
            app.run("edit.group", json!({}));
            ui.close();
        }
        if ui.add(egui::Button::new("Ungroup").shortcut_text("Ctrl+Shift+G")).clicked() {
            app.run("edit.ungroup", json!({}));
            ui.close();
        }
        ui.separator();
        if ui.button("Join Paths").clicked() {
            app.run("path.join", json!({}));
            ui.close();
        }
        ui.separator();
        if ui.button("Flip Horizontal").clicked() {
            app.run("edit.flip", json!({ "axis": "horizontal" }));
            ui.close();
        }
        if ui.button("Flip Vertical").clicked() {
            app.run("edit.flip", json!({ "axis": "vertical" }));
            ui.close();
        }
        if ui.button("Rotate 90°").clicked() {
            app.run("edit.rotate", json!({ "degrees": 90 }));
            ui.close();
        }
    });
}

fn laser_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    if ui.button("Generate G-code").clicked() {
        if app.run("job.gcode", Value::Null).is_some() {
            app.say("G-code generated");
        }
        ui.close();
    }
    if ui.button("Estimate time").clicked() {
        if let Some(v) = app.run("job.estimate", Value::Null) {
            app.say(format!("Estimated {}", format_duration(v["seconds"].as_f64().unwrap_or(0.0))));
        }
        ui.close();
    }
}

fn item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
    let clicked = ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Saves (asking for a path when needed). Returns whether the document was saved.
pub(crate) fn save(app: &mut LightFoundryApp, always_ask: bool) -> bool {
    let path = if always_ask || app.session.path.is_none() { app.pick_save("Save project", "project.lightfoundry", &["lightfoundry"]) } else { None };
    let params = path.map_or(Value::Null, |p| json!({ "path": p }));
    if (always_ask && params.is_null()) || (app.session.path.is_none() && params.is_null()) {
        return false;
    }
    if app.run("file.save", params).is_some() {
        app.say("Saved");
        return true;
    }
    false
}

fn import(app: &mut LightFoundryApp, command: &str, title: &str, exts: &[&str]) {
    if let Some(path) = app.pick_open(title, exts)
        && app.run(command, json!({ "path": path })).is_some()
    {
        app.say("Imported");
    }
}

pub fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "—".into();
    }
    let s = seconds.round() as u64;
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, s) => format!("{h}h {m:02}m {s:02}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_duration(5.2), "5s");
        assert_eq!(format_duration(125.0), "2m 05s");
        assert_eq!(format_duration(3725.0), "1h 02m 05s");
        assert_eq!(format_duration(f64::NAN), "—");
        assert_eq!(format_duration(-1.0), "—");
    }
}
