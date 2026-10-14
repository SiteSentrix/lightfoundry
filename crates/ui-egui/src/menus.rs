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
                ui.checkbox(&mut app.show_rulers, "Rulers");
                ui.checkbox(&mut app.snapping.objects, "Snap to objects and guides");
                let mut grid_on = app.snapping.grid.is_some();
                if ui.checkbox(&mut grid_on, "Snap to grid").changed() {
                    app.snapping.grid = grid_on.then_some(app.snapping.grid.unwrap_or(5.0));
                }
                if let Some(step) = app.snapping.grid.as_mut() {
                    ui.horizontal(|ui| {
                        ui.label("Grid step");
                        egui::ComboBox::from_id_salt("grid_step").selected_text(format!("{step} mm")).show_ui(ui, |ui| {
                            for s in [0.5, 1.0, 2.0, 5.0, 10.0] {
                                ui.selectable_value(step, s, format!("{s} mm"));
                            }
                        });
                    });
                }
                if ui.add_enabled(!app.guides.is_empty(), egui::Button::new("Clear guides")).clicked() {
                    app.guides.clear();
                    ui.close();
                }
                ui.separator();
                ui.checkbox(&mut app.show_preview, "Toolpath preview");
                ui.checkbox(&mut app.show_travel, "Show travel moves");
                ui.menu_button("Theme", |ui| {
                    for choice in crate::theme::ThemeChoice::ALL {
                        if ui.radio_value(&mut app.theme, choice, choice.label()).clicked() {
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Preview colours", |ui| {
                    for (mode, label) in [
                        (crate::timeline_ui::PreviewColors::Layer, "By layer"),
                        (crate::timeline_ui::PreviewColors::Power, "By power"),
                        (crate::timeline_ui::PreviewColors::Speed, "By speed"),
                    ] {
                        if ui.radio_value(&mut app.preview_colors, mode, label).clicked() {
                            ui.close();
                        }
                    }
                });
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
    recent_menu(app, ui);
    template_menu(app, ui);
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
        if app.platform_browser_files() {
            if let Some(v) = app.run("job.gcode", json!({ "include": true }))
                && let Some(text) = v["gcode"].as_str()
            {
                app.download("job.gcode", text.as_bytes(), "text/plain");
            }
        } else if let Some(path) = app.pick_save("Save G-code", "job.gcode", &["gcode", "nc"])
            && app.run("file.export_gcode", json!({ "path": path })).is_some()
        {
            app.say("G-code saved");
        }
        ui.close();
    }
}

fn template_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    if app.templates_available() {
        let templates = app.templates();
        let mut chosen = None;
        ui.menu_button("New from Template", |ui| {
            if templates.is_empty() {
                ui.weak("No templates yet");
            }
            for (name, path) in &templates {
                if ui.button(name).clicked() {
                    chosen = Some(path.clone());
                    ui.close();
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.template_name).hint_text("Template name").desired_width(120.0));
                if ui.add_enabled(!app.template_name.trim().is_empty(), egui::Button::new("Save current")).clicked() {
                    let name = std::mem::take(&mut app.template_name);
                    app.save_template(&name);
                    ui.close();
                }
            });
        });
        if let Some(path) = chosen {
            app.request(PendingAction::FromTemplate(path));
            ui.close();
        }
    }
}

fn recent_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let mut chosen = None;
    let mut clear = false;
    ui.add_enabled_ui(!app.recent.files.is_empty(), |ui| {
        ui.menu_button("Open Recent", |ui| {
            for path in &app.recent.files {
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                    chosen = Some(path.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Clear list").clicked() {
                clear = true;
                ui.close();
            }
        });
    });
    if clear {
        app.recent.files.clear();
        app.save_recent();
    }
    if let Some(path) = chosen {
        app.request(PendingAction::OpenPath(path));
        ui.close();
    }
}

fn edit_menu(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let (can_undo, can_redo) = (app.session.can_undo(), app.session.can_redo());
    use crate::shortcuts::Action;
    if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text(app.shortcuts.text(Action::Undo))).clicked() {
        app.run("edit.undo", Value::Null);
        ui.close();
    }
    if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text(app.shortcuts.text(Action::Redo))).clicked() {
        app.run("edit.redo", Value::Null);
        ui.close();
    }
    ui.separator();
    for (label, id, action) in [
        ("Select All", "edit.select_all", Some(Action::SelectAll)),
        ("Duplicate", "edit.duplicate", Some(Action::Duplicate)),
        ("Delete", "edit.delete", None),
    ] {
        let shortcut = action.map_or("Del".to_string(), |a| app.shortcuts.text(a));
        if ui.add_enabled(id == "edit.select_all" || !app.session.selection.is_empty(), egui::Button::new(label).shortcut_text(shortcut)).clicked() {
            app.run(id, Value::Null);
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Keyboard shortcuts…").clicked() {
        app.shortcuts_open = true;
        ui.close();
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
        ui.add_enabled_ui(app.session.selection.len() >= 3, |ui| {
            for (label, axis) in [("Distribute Horizontally", "horizontal"), ("Distribute Vertically", "vertical")] {
                if ui.button(label).clicked() {
                    app.run("edit.distribute", json!({ "axis": axis }));
                    ui.close();
                }
            }
        });
        ui.add_enabled_ui(app.session.selection.len() == 2, |ui| {
            if ui.button("Text on Path").on_hover_text("Select a text and the shape to bend it along").clicked() {
                let is_text = |id: &u64| app.session.doc.shape(*id).is_some_and(|s| matches!(s.kind, lightfoundry_doc::ShapeKind::Text { .. }));
                if let [a, b] = app.session.selection[..] {
                    let (text, path) = if is_text(&a) { (a, b) } else { (b, a) };
                    app.run("text.on_path", json!({ "text": text, "path": path }));
                }
                ui.close();
            }
        });
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
        if ui.add(egui::Button::new("Group").shortcut_text(app.shortcuts.text(crate::shortcuts::Action::Group))).clicked() {
            app.run("edit.group", json!({}));
            ui.close();
        }
        if ui.add(egui::Button::new("Ungroup").shortcut_text(app.shortcuts.text(crate::shortcuts::Action::Ungroup))).clicked() {
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
    if app.platform_browser_files() {
        let Some(v) = app.run("file.project_data", Value::Null) else { return false };
        let bytes = v["data_base64"].as_str().and_then(lightfoundry_doc::base64::decode).unwrap_or_default();
        let name =
            app.session.path.as_ref().and_then(|p| p.file_name()).map_or("project.lightfoundry".to_string(), |n| n.to_string_lossy().into_owned());
        app.download(&name, &bytes, "application/json");
        return true;
    }
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
    if app.platform_browser_files() {
        let kind = match command {
            "file.import_svg" => crate::UploadKind::Svg,
            "file.import_dxf" => crate::UploadKind::Dxf,
            _ => crate::UploadKind::Image,
        };
        app.begin_upload(kind);
        return;
    }
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
