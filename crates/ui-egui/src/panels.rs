use lightfoundry_device::{JobState, Link};
use lightfoundry_doc::{Anchor, Dither, LayerMode, Origin, ShapeKind, StartFrom, TextAlign};
use serde_json::{Value, json};

use crate::menus::format_duration;
use crate::theme::{on_dark, parse_hex};
use crate::{LightFoundryApp, Target, Tool};

pub fn toolbar(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    for (tool, label) in [
        (Tool::Select, "Select"),
        (Tool::Rectangle, "Rectangle"),
        (Tool::Ellipse, "Ellipse"),
        (Tool::Line, "Line"),
        (Tool::Polygon, "Polygon"),
        (Tool::Text, "Text"),
    ] {
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
        egui::CollapsingHeader::new("Selection").default_open(true).show(ui, |ui| {
            properties(app, ui);
            text_properties(app, ui);
        });
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

    job_origin(app, ui);
    ui.separator();
    connection(app, ui);
    job_controls(app, ui);
    if app.link.is_some() {
        jog_pad(app, ui);
        console(app, ui);
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

/// Workspace-direction step → machine-axis step (the machine's Y axis may point up or down).
pub(crate) fn machine_delta(origin: Origin, dx: f64, dy: f64) -> (f64, f64) {
    match origin {
        Origin::TopLeft => (dx, dy),
        Origin::BottomLeft => (dx, -dy),
    }
}

fn job_origin(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let o = app.session.doc.job_origin;
    ui.horizontal(|ui| {
        ui.label("Start from");
        let mut start_from = o.start_from;
        egui::ComboBox::from_id_salt("start_from")
            .selected_text(match start_from {
                StartFrom::Absolute => "Absolute coordinates",
                StartFrom::UserOrigin => "User origin",
                StartFrom::CurrentPosition => "Current position",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut start_from, StartFrom::Absolute, "Absolute coordinates");
                ui.selectable_value(&mut start_from, StartFrom::UserOrigin, "User origin");
                ui.selectable_value(&mut start_from, StartFrom::CurrentPosition, "Current position");
            });
        if start_from != o.start_from {
            app.run("job.set_origin", json!({ "start_from": start_from }));
        }
    });
    if o.start_from == StartFrom::Absolute {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Job origin");
        egui::Grid::new("anchor_grid").spacing([2.0, 2.0]).show(ui, |ui| {
            for row in Anchor::ALL.chunks(3) {
                for a in row {
                    if anchor_button(ui, o.anchor == *a).clicked() {
                        app.run("job.set_origin", json!({ "anchor": a }));
                    }
                }
                ui.end_row();
            }
        });
    });
    ui.weak(match o.start_from {
        StartFrom::UserOrigin => "Jog to the spot, then Set origin; that spot is the job origin.",
        _ => "The head's position when the job starts becomes the job origin.",
    });
}

fn connection(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let connected = app.link.is_some();
    ui.horizontal(|ui| {
        ui.label("Machine");
        let current = match &app.target {
            Target::Simulator => "Simulator".to_string(),
            Target::Serial(p) => p.clone(),
        };
        ui.add_enabled_ui(!connected, |ui| {
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
        if connected {
            if ui.button("Disconnect").clicked() {
                app.disconnect();
            }
        } else if ui.button("Connect").clicked() {
            app.connect();
        }
    });
    if let Some(s) = &app.snap {
        let state = s.status.as_ref().map_or("…".to_string(), |st| format!("{:?}", st.state));
        let pos = s.status.as_ref().and_then(|st| st.position).map_or(String::new(), |(x, y, _)| format!("   X {x:.2}  Y {y:.2}"));
        ui.label(egui::RichText::new(format!("{state}{pos}")).monospace());
    }
}

fn job_controls(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let connected = app.link.is_some();
    let active = app.job_active();
    let paused = app.snap.as_ref().is_some_and(|s| s.job == JobState::Paused);
    ui.horizontal(|ui| {
        if ui.button("Generate").clicked() {
            generate(app);
        }
        if ui.add_enabled(connected && !active, egui::Button::new("Frame")).clicked()
            && let Some(v) = app.run("job.frame", json!({ "power": app.frame_power }))
            && let Some(g) = v["gcode"].as_str()
        {
            app.start_job(g.to_string());
        }
        if ui.add_enabled(connected && !active, egui::Button::new("Start")).clicked()
            && let Some(v) = app.run("job.gcode", json!({ "include": true }))
            && let Some(g) = v["gcode"].as_str()
        {
            app.start_job(g.to_string());
        }
    });
    ui.horizontal(|ui| {
        if paused {
            if ui.button("Resume").clicked() {
                app.link_do(Link::resume);
            }
        } else if ui.add_enabled(active, egui::Button::new("Pause")).clicked() {
            app.link_do(Link::pause);
        }
        if ui.add_enabled(connected, egui::Button::new("Stop")).on_hover_text("Soft reset: stops motion and turns the laser off").clicked() {
            app.link_do(Link::stop);
        }
        ui.label("Frame power");
        ui.add(egui::DragValue::new(&mut app.frame_power).range(0.0..=100.0).suffix(" %"));
    });
    if let Some(p) = app.snap.as_ref().and_then(|s| s.progress)
        && p.total > 0
        && active
    {
        ui.add(egui::ProgressBar::new(p.acknowledged as f32 / p.total as f32).text(format!("{} / {}", p.acknowledged, p.total)));
    }
}

fn jog_pad(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let enabled = !app.job_active();
    let origin = app.session.doc.machine.origin;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Step");
            egui::ComboBox::from_id_salt("jog_step").selected_text(format!("{} mm", app.jog_step)).show_ui(ui, |ui| {
                for s in [0.1, 1.0, 10.0, 50.0] {
                    ui.selectable_value(&mut app.jog_step, s, format!("{s} mm"));
                }
            });
            ui.label("Speed");
            ui.add(egui::DragValue::new(&mut app.jog_feed).range(10.0..=30_000.0).suffix(" mm/min"));
        });
        let step = app.jog_step;
        let jog = |app: &mut LightFoundryApp, wx: f64, wy: f64| {
            let (dx, dy) = machine_delta(origin, wx * step, wy * step);
            let feed = app.jog_feed;
            app.link_do(|l| l.jog(dx, dy, feed));
        };
        // Directions are in workspace terms: "up" is toward the top of the canvas.
        let rows: [[Option<(f64, f64)>; 3]; 3] =
            [[None, Some((0.0, -1.0)), None], [Some((-1.0, 0.0)), None, Some((1.0, 0.0))], [None, Some((0.0, 1.0)), None]];
        egui::Grid::new("jog_grid").spacing([2.0, 2.0]).show(ui, |ui| {
            for cells in rows {
                for cell in cells {
                    if let Some((wx, wy)) = cell {
                        if arrow_button(ui, wx, wy).clicked() {
                            jog(app, wx, wy);
                        }
                    } else {
                        ui.allocate_space(egui::vec2(34.0, 28.0));
                    }
                }
                ui.end_row();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Home").on_hover_text("Run the homing cycle ($H)").clicked() {
                app.link_do(Link::home);
            }
            if ui.button("Unlock").on_hover_text("Clear an alarm ($X)").clicked() {
                app.link_do(Link::unlock);
            }
            if ui.button("Set origin here").on_hover_text("Make the head's position the work origin (G92)").clicked() {
                app.link_do(Link::set_zero);
            }
            if ui.button("Go to origin").clicked() {
                app.link_do(Link::go_to_zero);
            }
        });
    });
}

fn console(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.collapsing("Console", |ui| {
        let lines = app.snap.as_ref().map(|s| s.console.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
            for l in lines.iter().rev().take(100).rev() {
                ui.label(egui::RichText::new(l).monospace().small());
            }
        });
        let r =
            ui.add(egui::TextEdit::singleline(&mut app.console_input).hint_text("G-code or $ command, Enter to send").desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let line = std::mem::take(&mut app.console_input);
            app.link_do(|l| l.send_line(&line));
            r.request_focus();
        }
    });
}

/// A square button with a triangle pointing along `(dx, dy)`, drawn in code (the default font has
/// no arrow glyphs, and we ship no icon assets).
fn arrow_button(ui: &mut egui::Ui, dx: f64, dy: f64) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(34.0, 28.0), egui::Sense::click());
    let visuals = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    let (fx, fy) = (dx as f32, dy as f32);
    let c = rect.center();
    let along = egui::vec2(fx, fy) * 7.0;
    let across = egui::vec2(-fy, fx) * 6.0;
    let points = vec![c + along, c - along * 0.6 + across, c - along * 0.6 - across];
    ui.painter().add(egui::Shape::convex_polygon(points, visuals.fg_stroke.color, egui::Stroke::NONE));
    resp
}

/// One cell of the 3×3 job-origin picker: a ring, filled when selected (drawn in code).
fn anchor_button(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    let color = if selected { ui.visuals().selection.stroke.color } else { visuals.fg_stroke.color };
    if selected {
        ui.painter().circle_filled(rect.center(), 5.5, color);
    } else {
        ui.painter().circle_stroke(rect.center(), 5.0, egui::Stroke::new(1.5, color));
    }
    resp
}

/// The editor for a single selected text object.
fn text_properties(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let [id] = app.session.selection.as_slice() else {
        app.text_buffer = None;
        return;
    };
    let id = *id;
    let Some((content, style)) = app.session.doc.shape(id).and_then(|s| match &s.kind {
        ShapeKind::Text { content, style, .. } => Some((content.clone(), style.clone())),
        _ => None,
    }) else {
        app.text_buffer = None;
        return;
    };
    if app.text_buffer.as_ref().is_none_or(|(buf_id, _)| *buf_id != id) {
        app.text_buffer = Some((id, content.clone()));
    }
    ui.separator();
    ui.label(egui::RichText::new("Text").strong());
    let mut commit = None;
    if let Some((_, buffer)) = app.text_buffer.as_mut() {
        let r = ui.add(egui::TextEdit::multiline(buffer).desired_rows(2).desired_width(f32::INFINITY).hint_text("Type text; click away to apply"));
        if r.lost_focus() && *buffer != content && !buffer.trim().is_empty() {
            commit = Some(json!({ "id": id, "text": buffer.clone() }));
        }
    }
    egui::Grid::new("text_style").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (label, key, value, range) in [
            ("Height", "height", style.height, 0.1..=1000.0),
            ("Letter gap", "letter_spacing", style.letter_spacing, 0.0..=10.0),
            ("Line gap", "line_spacing", style.line_spacing, 0.0..=10.0),
        ] {
            ui.label(label);
            if let Some(v) = number(ui, value, range, if key == "height" { " mm" } else { "×" }) {
                commit = Some(json!({ "id": id, key: v }));
            }
            ui.end_row();
        }
        ui.label("Align");
        let mut align = style.align;
        egui::ComboBox::from_id_salt("text_align")
            .selected_text(match align {
                TextAlign::Left => "Left",
                TextAlign::Center => "Center",
                TextAlign::Right => "Right",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut align, TextAlign::Left, "Left");
                ui.selectable_value(&mut align, TextAlign::Center, "Center");
                ui.selectable_value(&mut align, TextAlign::Right, "Right");
            });
        if align != style.align {
            commit = Some(json!({ "id": id, "align": align }));
        }
        ui.end_row();
    });
    if let Some(params) = commit {
        if let Some(v) = app.run("text.edit", params) {
            app.note_unsupported(&v);
        }
        app.text_buffer = None;
    }
    if let Some(w) = &app.text_warning {
        ui.colored_label(app.tokens.error, w);
    }
    ui.weak("Single-line stroke font: engraves each stroke in one pass (Line mode).");
}
