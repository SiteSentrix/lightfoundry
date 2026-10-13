use lightfoundry_doc::{Dither, LayerMode, Origin};
use serde_json::{Value, json};

use crate::menus::format_duration;
use crate::theme::{on_dark, parse_hex};
use crate::{LightFoundryApp, Target, Tool};

pub fn toolbar(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    for (tool, label) in
        [(Tool::Select, "Select"), (Tool::Rectangle, "Rectangle"), (Tool::Ellipse, "Ellipse"), (Tool::Line, "Line"), (Tool::Polygon, "Polygon")]
    {
        if ui.add_sized([72.0, 28.0], egui::Button::new(label).selected(app.tool == tool)).clicked() {
            app.tool = tool;
        }
    }
    if app.tool == Tool::Polygon {
        ui.add_space(6.0);
        ui.label("Sides");
        ui.add(egui::DragValue::new(&mut app.polygon_sides).range(3..=100));
    }
}

pub fn inspector(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::CollapsingHeader::new("Cuts / Layers").default_open(true).show(ui, |ui| layers(app, ui));
        egui::CollapsingHeader::new("Selection").default_open(true).show(ui, |ui| properties(app, ui));
        egui::CollapsingHeader::new("Laser").default_open(true).show(ui, |ui| laser(app, ui));
    });
}

fn layers(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let used: Vec<u32> = app.session.doc.shapes.iter().map(|s| s.layer).collect();
    let layer_list = app.session.doc.layers.clone();
    // Chips are laid out in rows of eight, like a palette.
    for row in layer_list.chunks(8) {
        ui.horizontal(|ui| {
            for layer in row {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 24.0), egui::Sense::click());
                let color = on_dark(parse_hex(&layer.color));
                let fill = if layer.output { color } else { color.gamma_multiply(0.25) };
                ui.painter().rect_filled(rect, 3.0, fill);
                if app.active_layer == layer.id {
                    ui.painter().rect_stroke(rect.expand(2.0), 4.0, egui::Stroke::new(2.0, app.tokens.selection), egui::StrokeKind::Outside);
                }
                if used.contains(&layer.id) {
                    ui.painter().circle_filled(rect.right_top() + egui::vec2(-5.0, 5.0), 2.5, egui::Color32::BLACK);
                }
                if resp.on_hover_text(format!("{}{}", layer.name, if layer.output { "" } else { " (output off)" })).clicked() {
                    app.active_layer = layer.id;
                }
            }
        });
    }
    let Some(layer) = app.session.doc.layer(app.active_layer).cloned() else { return };
    ui.add_space(4.0);
    ui.label(egui::RichText::new(&layer.name).strong());
    let mut output = layer.output;
    if ui.checkbox(&mut output, "Output").changed() {
        app.run("layer.set", json!({ "id": layer.id, "output": output }));
    }
    egui::Grid::new("layer_settings").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Mode");
        let mut mode = layer.mode;
        egui::ComboBox::from_id_salt("mode")
            .selected_text(match mode {
                LayerMode::Line => "Line",
                LayerMode::Fill => "Fill",
                LayerMode::FillAndLine => "Fill + Line",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, LayerMode::Line, "Line");
                ui.selectable_value(&mut mode, LayerMode::Fill, "Fill");
                ui.selectable_value(&mut mode, LayerMode::FillAndLine, "Fill + Line");
            });
        if mode != layer.mode {
            app.run("layer.set", json!({ "id": layer.id, "mode": mode }));
        }
        ui.end_row();

        for (label, key, value, range, suffix) in [
            ("Speed", "speed", layer.speed, 1.0..=100_000.0, " mm/min"),
            ("Max power", "max_power", layer.max_power, 0.0..=100.0, " %"),
            ("Min power", "min_power", layer.min_power, 0.0..=100.0, " %"),
            ("Interval", "interval", layer.interval, 0.005..=10.0, " mm"),
            ("Scan angle", "scan_angle", layer.scan_angle, -360.0..=360.0, "°"),
        ] {
            ui.label(label);
            if let Some(v) = number(ui, value, range, suffix) {
                app.run("layer.set", json!({ "id": layer.id, key: v }));
            }
            ui.end_row();
        }

        ui.label("Passes");
        let mut passes = layer.passes;
        let r = ui.add(egui::DragValue::new(&mut passes).range(1..=1000));
        if committed(&r) && passes != layer.passes {
            app.run("layer.set", json!({ "id": layer.id, "passes": passes }));
        }
        ui.end_row();

        ui.label("Image");
        let mut dither = layer.dither;
        egui::ComboBox::from_id_salt("dither")
            .selected_text(match dither {
                Dither::Threshold => "Threshold",
                Dither::FloydSteinberg => "Floyd–Steinberg",
                Dither::Grayscale => "Grayscale",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut dither, Dither::Threshold, "Threshold");
                ui.selectable_value(&mut dither, Dither::FloydSteinberg, "Floyd–Steinberg");
                ui.selectable_value(&mut dither, Dither::Grayscale, "Grayscale");
            });
        if dither != layer.dither {
            app.run("layer.set", json!({ "id": layer.id, "dither": dither }));
        }
        ui.end_row();
    });
    let mut bidi = layer.bidirectional;
    if ui.checkbox(&mut bidi, "Bi-directional fill").changed() {
        app.run("layer.set", json!({ "id": layer.id, "bidirectional": bidi }));
    }
    let mut air = layer.air_assist;
    if ui.checkbox(&mut air, "Air assist").changed() {
        app.run("layer.set", json!({ "id": layer.id, "air_assist": air }));
    }
}

fn properties(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let sel = app.session.selection.clone();
    let Some(b) = app.session.doc.bounds_of(&sel) else {
        ui.weak("Nothing selected");
        return;
    };
    ui.label(format!("{} object{}", sel.len(), if sel.len() == 1 { "" } else { "s" }));
    egui::Grid::new("props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("X");
        if let Some(x) = number(ui, b.x0, -1e6..=1e6, " mm") {
            app.run("edit.move_to", json!({ "x": x, "y": b.y0 }));
        }
        ui.end_row();
        ui.label("Y");
        if let Some(y) = number(ui, b.y0, -1e6..=1e6, " mm") {
            app.run("edit.move_to", json!({ "x": b.x0, "y": y }));
        }
        ui.end_row();
        ui.label("Width");
        if let Some(w) = number(ui, b.width(), 0.01..=1e5, " mm") {
            app.run("edit.resize", json!({ "width": w }));
        }
        ui.end_row();
        ui.label("Height");
        if let Some(h) = number(ui, b.height(), 0.01..=1e5, " mm") {
            app.run("edit.resize", json!({ "height": h }));
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.button("Rotate 90°").clicked() {
            app.run("edit.rotate", json!({ "degrees": 90 }));
        }
        if ui.button("Flip H").clicked() {
            app.run("edit.flip", json!({ "axis": "horizontal" }));
        }
        if ui.button("Flip V").clicked() {
            app.run("edit.flip", json!({ "axis": "vertical" }));
        }
    });
    if ui.button(format!("Move to layer {}", app.active_layer)).clicked() {
        app.run("edit.set_layer", json!({ "layer": app.active_layer }));
    }
}

fn laser(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let m = app.session.doc.machine.clone();
    ui.collapsing("Device", |ui| {
        egui::Grid::new("device").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Bed width");
            if let Some(v) = number(ui, m.bed_width, 1.0..=100_000.0, " mm") {
                app.run("machine.set", json!({ "bed_width": v }));
                app.view = None;
            }
            ui.end_row();
            ui.label("Bed height");
            if let Some(v) = number(ui, m.bed_height, 1.0..=100_000.0, " mm") {
                app.run("machine.set", json!({ "bed_height": v }));
                app.view = None;
            }
            ui.end_row();
            ui.label("Origin");
            let mut origin = m.origin;
            egui::ComboBox::from_id_salt("origin").selected_text(if origin == Origin::BottomLeft { "Bottom-left" } else { "Top-left" }).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut origin, Origin::BottomLeft, "Bottom-left");
                    ui.selectable_value(&mut origin, Origin::TopLeft, "Top-left");
                },
            );
            if origin != m.origin {
                app.run("machine.set", json!({ "origin": origin }));
            }
            ui.end_row();
            ui.label("Max S ($30)");
            let mut max_s = m.max_s;
            if committed(&ui.add(egui::DragValue::new(&mut max_s).range(1..=100_000))) && max_s != m.max_s {
                app.run("machine.set", json!({ "max_s": max_s }));
            }
            ui.end_row();
        });
        let mut dynamic = m.dynamic_power;
        if ui.checkbox(&mut dynamic, "Dynamic power (M4)").changed() {
            app.run("machine.set", json!({ "dynamic_power": dynamic }));
        }
    });

    ui.horizontal(|ui| {
        ui.label("Send to");
        let current = match &app.target {
            Target::Simulator => "Simulator".to_string(),
            Target::Serial(p) => p.clone(),
        };
        egui::ComboBox::from_id_salt("target").selected_text(current).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.target, Target::Simulator, "Simulator");
            for p in app.ports.clone() {
                ui.selectable_value(&mut app.target, Target::Serial(p.clone()), p);
            }
        });
        if ui.small_button("↻").on_hover_text("Rescan serial ports").clicked() {
            app.ports = app.platform.serial_ports();
        }
    });

    let running = app.runner.as_ref().is_some_and(|r| r.is_running());
    ui.horizontal(|ui| {
        if ui.button("Generate").clicked() {
            generate(app);
        }
        if ui.add_enabled(!running, egui::Button::new("Frame")).clicked()
            && let Some(v) = app.run("job.frame", json!({ "power": app.frame_power }))
            && let Some(g) = v["gcode"].as_str()
        {
            app.start_job(g.to_string());
        }
        if ui.add_enabled(!running, egui::Button::new("Start")).clicked()
            && let Some(v) = app.run("job.gcode", json!({ "include": true }))
            && let Some(g) = v["gcode"].as_str()
        {
            app.start_job(g.to_string());
        }
        if ui.add_enabled(running, egui::Button::new("Stop")).clicked()
            && let Some(r) = &app.runner
        {
            r.cancel();
        }
    });
    ui.horizontal(|ui| {
        ui.label("Frame power");
        ui.add(egui::DragValue::new(&mut app.frame_power).range(0.0..=100.0).suffix(" %"));
    });
    if let Some(r) = &app.runner {
        let p = r.progress();
        if p.total > 0 {
            ui.add(egui::ProgressBar::new(p.acknowledged as f32 / p.total as f32).text(format!("{} / {}", p.acknowledged, p.total)));
        }
    }
    if let Some(Err(e)) = &app.run_result {
        ui.colored_label(app.tokens.error, e);
    }
    if let Some(gcode) = app.session.last_gcode.clone() {
        ui.collapsing("G-code", |ui| {
            let lines = gcode.lines().count();
            ui.weak(format!("{lines} lines"));
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                // Show the head only: a raster job can be millions of lines.
                let head: String = gcode.lines().take(300).collect::<Vec<_>>().join("\n");
                ui.add(egui::Label::new(egui::RichText::new(head).monospace().small()).wrap_mode(egui::TextWrapMode::Extend));
            });
        });
    }
}

fn generate(app: &mut LightFoundryApp) {
    if app.run("job.gcode", Value::Null).is_none() {
        return;
    }
    if let Some(v) = app.run("job.estimate", Value::Null) {
        let est = format_duration(v["seconds"].as_f64().unwrap_or(0.0));
        let lines = app.session.last_gcode.as_ref().map_or(0, |g| g.lines().count());
        app.say(format!("{lines} lines of G-code · estimated {est}"));
    }
}

/// A numeric field that reports a value only when the user finishes editing it, so dragging does
/// not create an undo step per frame.
fn number(ui: &mut egui::Ui, value: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) -> Option<f64> {
    let mut v = value;
    let r = ui.add(egui::DragValue::new(&mut v).range(range).suffix(suffix).speed(0.1).max_decimals(3));
    (committed(&r) && (v - value).abs() > 1e-9).then_some(v)
}

fn committed(r: &egui::Response) -> bool {
    r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus())
}
