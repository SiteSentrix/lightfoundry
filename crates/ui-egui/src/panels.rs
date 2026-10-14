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
        (Tool::Nodes, "Nodes"),
        (Tool::Pen, "Pen"),
    ] {
        if ui.add_sized([72.0, 28.0], egui::Button::new(label).selected(app.tool == tool)).clicked() {
            if app.tool == Tool::Pen && tool != Tool::Pen {
                // Switching away completes the path being drawn rather than losing it.
                crate::nodes_ui::finish(app, false);
            }
            app.tool = tool;
            app.node_sel = None;
        }
    }
    if app.tool == Tool::Rectangle {
        ui.add_space(6.0);
        ui.label("Corner radius");
        ui.add(egui::DragValue::new(&mut app.corner_radius).range(0.0..=500.0).speed(0.1).suffix(" mm"));
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
            node_section(app, ui);
            text_properties(app, ui);
            image_properties(app, ui);
            shape_operations(app, ui);
            array_section(app, ui);
            override_section(app, ui);
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
    layer_header(app, ui, &layer);
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
            ("Kerf offset", "kerf_offset", layer.kerf_offset, -50.0..=50.0, " mm"),
            ("Tab width", "tab_width", layer.tab_width, 0.0..=1000.0, " mm"),
            ("Lead-in", "lead_in", layer.lead_in, 0.0..=1000.0, " mm"),
            ("Lead-out", "lead_out", layer.lead_out, 0.0..=1000.0, " mm"),
        ] {
            ui.label(label);
            if let Some(v) = number(ui, value, range, suffix) {
                app.run("layer.set", json!({ "id": layer.id, key: v }));
            }
            ui.end_row();
        }

        ui.label("Tabs");
        let mut tabs = layer.tab_count;
        let r = ui.add(egui::DragValue::new(&mut tabs).range(0..=1000));
        if committed(&r) && tabs != layer.tab_count {
            app.run("layer.set", json!({ "id": layer.id, "tab_count": tabs }));
        }
        ui.end_row();

        ui.label("Passes");
        let mut passes = layer.passes;
        let r = ui.add(egui::DragValue::new(&mut passes).range(1..=1000));
        if committed(&r) && passes != layer.passes {
            app.run("layer.set", json!({ "id": layer.id, "passes": passes }));
        }
        ui.end_row();

        ui.label("Image");
        let mut dither = layer.dither;
        egui::ComboBox::from_id_salt("dither").selected_text(dither.label()).show_ui(ui, |ui| {
            for d in Dither::ALL {
                ui.selectable_value(&mut dither, d, d.label());
            }
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
    presets_section(app, ui, layer.id);
}

/// Named cut settings kept in the project: apply one to this layer, or save this layer's as one.
fn presets_section(app: &mut LightFoundryApp, ui: &mut egui::Ui, layer: u32) {
    ui.collapsing("Presets", |ui| {
        let names: Vec<String> = app.session.doc.presets.iter().map(|p| p.name.clone()).collect();
        if names.is_empty() {
            ui.weak("No presets yet. Set up this layer, name it below and save.");
        }
        for name in &names {
            ui.horizontal(|ui| {
                if ui.button(name).on_hover_text("Apply to this layer").clicked() {
                    app.run("preset.apply", json!({ "name": name, "layer": layer }));
                }
                if ui.small_button("✕").on_hover_text("Delete this preset").clicked() {
                    app.run("preset.delete", json!({ "name": name }));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.preset_name).hint_text("Preset name").desired_width(120.0));
            if ui.add_enabled(!app.preset_name.trim().is_empty(), egui::Button::new("Save")).clicked()
                && app.run("preset.save", json!({ "name": app.preset_name, "layer": layer })).is_some()
            {
                app.preset_name.clear();
            }
        });
    });
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
        let mut dialect = m.dialect;
        ui.horizontal(|ui| {
            ui.label("G-code");
            egui::ComboBox::from_id_salt("dialect").selected_text(dialect.label()).show_ui(ui, |ui| {
                for d in lightfoundry_doc::Dialect::ALL {
                    ui.selectable_value(&mut dialect, d, d.label());
                }
            });
        });
        if dialect != m.dialect {
            app.run("machine.set", json!({ "dialect": dialect }));
        }
        if dialect != lightfoundry_doc::Dialect::Grbl {
            ui.weak("Written from the firmware's public documentation and untested on a machine. Use Frame at 0% power first. The controller-link features ($$, jog) are GRBL only.");
        }
        let mut dynamic = m.dynamic_power;
        if ui.checkbox(&mut dynamic, "Dynamic power (M4)").changed() {
            app.run("machine.set", json!({ "dynamic_power": dynamic }));
        }
    });

    job_origin(app, ui);
    cut_order(app, ui);
    test_grid_section(app, ui);
    ui.separator();
    connection(app, ui);
    job_controls(app, ui);
    if app.link.is_some() {
        jog_pad(app, ui);
        crate::settings_ui::section(app, ui);
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

pub(crate) fn committed(r: &egui::Response) -> bool {
    r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus())
}

/// Workspace-direction step → machine-axis step (the machine's Y axis may point up or down).
pub(crate) fn machine_delta(origin: Origin, dx: f64, dy: f64) -> (f64, f64) {
    match origin {
        Origin::TopLeft => (dx, dy),
        Origin::BottomLeft => (dx, -dy),
    }
}

fn cut_order(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let o = app.session.doc.optimize;
    ui.collapsing("Cut order", |ui| {
        for (key, label, tip, on) in [
            ("inner_first", "Inner shapes first", "Cut holes and nested shapes before the outlines around them", o.inner_first),
            ("reduce_travel", "Reduce travel", "Go to the nearest outline next instead of following the drawing order", o.reduce_travel),
            ("remove_duplicates", "Remove duplicate lines", "Skip outlines that lie exactly on top of another one", o.remove_duplicates),
        ] {
            let mut v = on;
            if ui.checkbox(&mut v, label).on_hover_text(tip).changed() {
                app.run("job.set_optimize", json!({ key: v }));
            }
        }
    });
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

/// Boolean combine and offset for the current selection.
fn shape_operations(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let count = app.session.selection.len();
    if count == 0 {
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new("Shape operations").strong());
    ui.add_enabled_ui(count >= 2, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (label, op, hint) in [
                ("Union", "union", "Weld the shapes into one outline"),
                ("Subtract", "difference", "The first selected shape minus the others"),
                ("Intersect", "intersect", "Keep only the overlap"),
                ("XOR", "xor", "Keep everything except the overlap"),
            ] {
                if ui.button(label).on_hover_text(hint).clicked() {
                    app.run("path.boolean", json!({ "op": op }));
                }
            }
        });
    });
    if ui.button("Join paths").on_hover_text("Chain open lines whose ends meet into paths, closing loops").clicked() {
        app.run("path.join", json!({}));
    }
    ui.horizontal(|ui| {
        ui.label("Offset");
        ui.add(egui::DragValue::new(&mut app.offset_distance).range(-1000.0..=1000.0).speed(0.1).suffix(" mm"));
        egui::ComboBox::from_id_salt("offset_join").selected_text(app.offset_join).width(70.0).show_ui(ui, |ui| {
            for j in ["round", "miter", "bevel"] {
                ui.selectable_value(&mut app.offset_join, j, j);
            }
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Apply offset").on_hover_text("Negative values shrink closed shapes; open lines become an outline around the line").clicked() {
            let params = json!({ "distance": app.offset_distance, "join": app.offset_join, "keep_originals": app.offset_keep });
            app.run("path.offset", params);
        }
        ui.checkbox(&mut app.offset_keep, "Keep original");
    });
}

/// Tone adjustments for a single selected image.
fn image_properties(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let [id] = app.session.selection.as_slice() else { return };
    let id = *id;
    let Some(adjust) = app.session.doc.shape(id).and_then(|s| match &s.kind {
        ShapeKind::Image { adjust, .. } => Some(*adjust),
        _ => None,
    }) else {
        return;
    };
    ui.separator();
    ui.label(egui::RichText::new("Image").strong());
    let mut commit = None;
    egui::Grid::new("image_adjust").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        for (label, key, value, range, speed) in [
            ("Brightness", "brightness", adjust.brightness, -1.0..=1.0, 0.01),
            ("Contrast", "contrast", adjust.contrast, -1.0..=1.0, 0.01),
            ("Gamma", "gamma", adjust.gamma, 0.1..=5.0, 0.02),
        ] {
            ui.label(label);
            let mut v = value;
            let r = ui.add(egui::DragValue::new(&mut v).range(range).speed(speed).max_decimals(2));
            if committed(&r) && (v - value).abs() > 1e-9 {
                commit = Some(json!({ "id": id, key: v }));
            }
            ui.end_row();
        }
    });
    ui.horizontal(|ui| {
        let mut negative = adjust.negative;
        if ui.checkbox(&mut negative, "Negative").changed() {
            commit = Some(json!({ "id": id, "negative": negative }));
        }
        if ui.add_enabled(!adjust.is_identity(), egui::Button::new("Reset")).clicked() {
            commit = Some(json!({ "id": id, "reset": true }));
        }
    });
    if let Some(params) = commit {
        app.run("image.adjust", params);
    }
    crop_and_sharpen(app, ui, id);
    ui.weak("The dither mode for engraving is set on the layer (Cuts / Layers).");
}

fn crop_and_sharpen(app: &mut LightFoundryApp, ui: &mut egui::Ui, id: u64) {
    ui.collapsing("Crop and sharpen", |ui| {
        egui::Grid::new("image_crop").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (label, slot) in ["Cut off left", "Cut off top", "Cut off right", "Cut off bottom"].into_iter().zip(app.crop_pct.iter_mut()) {
                ui.label(label);
                ui.add(egui::DragValue::new(slot).range(0.0..=95.0).speed(0.5).suffix(" %").max_decimals(1));
                ui.end_row();
            }
        });
        let [left, top, right, bottom] = app.crop_pct.map(|p| p / 100.0);
        let fits = left + right < 1.0 && top + bottom < 1.0;
        if ui.add_enabled(fits && app.crop_pct.iter().any(|p| *p > 0.0), egui::Button::new("Crop")).clicked()
            && app.run("image.crop", json!({ "id": id, "left": left, "top": top, "right": right, "bottom": bottom })).is_some()
        {
            app.crop_pct = [0.0; 4];
        }
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut app.sharpen_amount).range(0.0..=5.0).speed(0.05).max_decimals(2));
            if ui.button("Sharpen").clicked() {
                app.run("image.sharpen", json!({ "id": id, "amount": app.sharpen_amount }));
            }
        });
        ui.weak("Both change the stored picture (Undo brings it back).");
    });
}

/// Grid and circular arrays of the current selection.
fn array_section(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    if app.session.selection.is_empty() {
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new("Array").strong());
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut app.array_columns).range(1..=200).prefix("cols "));
        ui.add(egui::DragValue::new(&mut app.array_rows).range(1..=200).prefix("rows "));
        ui.add(egui::DragValue::new(&mut app.array_gap).range(-1000.0..=1000.0).speed(0.1).prefix("gap ").suffix(" mm"));
    });
    if ui.button("Grid array").on_hover_text("Copy the selection into a grid; the original stays top-left").clicked() {
        let params = json!({ "columns": app.array_columns, "rows": app.array_rows, "gap_x": app.array_gap, "gap_y": app.array_gap });
        app.run("arrange.grid", params);
    }
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut app.array_count).range(1..=1000).prefix("copies "));
        ui.add(egui::DragValue::new(&mut app.array_radius).range(0.0..=1000.0).speed(0.5).prefix("radius ").suffix(" mm"));
    });
    if ui.button("Circular array").on_hover_text("Rotate copies about a centre this far below the selection").clicked() {
        let params = json!({ "count": app.array_count, "radius": app.array_radius });
        app.run("arrange.circular", params);
    }
}

/// Per-shape cut settings that replace the layer's.
fn override_section(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let sel = app.session.selection.clone();
    let Some(first) = sel.first().and_then(|id| app.session.doc.shape(*id)) else { return };
    let layer = app.session.doc.layer(first.layer).cloned();
    let current = first.cut;
    ui.separator();
    ui.horizontal(|ui| {
        let mut on = current.is_some();
        if ui.checkbox(&mut on, "Own cut settings").on_hover_text("Use these settings for the selected shapes instead of their layer's").changed() {
            if on {
                // Start from the layer's values so switching it on changes nothing until edited.
                let (speed, power) = layer.as_ref().map_or((1000.0, 50.0), |l| (l.speed, l.max_power));
                app.run("shape.cut_override", json!({ "speed": speed, "max_power": power }));
            } else {
                app.run("shape.cut_override", json!({ "clear": true }));
            }
        }
    });
    let Some(c) = current else { return };
    let (speed, power, passes) = (c.speed.unwrap_or(0.0), c.max_power.unwrap_or(0.0), c.passes.unwrap_or(1));
    egui::Grid::new("override_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("Speed");
        if let Some(v) = number(ui, speed, 1.0..=100_000.0, " mm/min") {
            app.run("shape.cut_override", json!({ "speed": v }));
        }
        ui.end_row();
        ui.label("Power");
        if let Some(v) = number(ui, power, 0.0..=100.0, " %") {
            app.run("shape.cut_override", json!({ "max_power": v }));
        }
        ui.end_row();
        ui.label("Passes");
        let mut n = passes;
        let r = ui.add(egui::DragValue::new(&mut n).range(1..=1000));
        if committed(&r) && n != passes {
            app.run("shape.cut_override", json!({ "passes": n }));
        }
        ui.end_row();
    });
}

/// A speed × power matrix of squares for finding settings on a new material.
fn test_grid_section(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    ui.collapsing("Material test grid", |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut app.grid_cols).range(1..=20).prefix("speeds "));
            ui.add(egui::DragValue::new(&mut app.grid_rows).range(1..=20).prefix("powers "));
            ui.add(egui::DragValue::new(&mut app.grid_size).range(1.0..=100.0).suffix(" mm"));
        });
        ui.horizontal(|ui| {
            ui.label("Speed");
            ui.add(egui::DragValue::new(&mut app.grid_speed.0).range(1.0..=100_000.0));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut app.grid_speed.1).range(1.0..=100_000.0).suffix(" mm/min"));
        });
        ui.horizontal(|ui| {
            ui.label("Power");
            ui.add(egui::DragValue::new(&mut app.grid_power.0).range(0.0..=100.0));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut app.grid_power.1).range(0.0..=100.0).suffix(" %"));
        });
        if ui.button("Create on the active layer").on_hover_text("Squares use the active layer's mode; labels go on the next layer").clicked() {
            let params = json!({
                "cols": app.grid_cols, "rows": app.grid_rows, "size": app.grid_size, "layer": app.active_layer,
                "speed_min": app.grid_speed.0, "speed_max": app.grid_speed.1, "power_min": app.grid_power.0, "power_max": app.grid_power.1,
            });
            app.run("generate.test_grid", params);
        }
    });
}

/// Name, and position in the cut order (layers are cut top to bottom of this list).
fn layer_header(app: &mut LightFoundryApp, ui: &mut egui::Ui, layer: &lightfoundry_doc::Layer) {
    let position = app.session.doc.layers.iter().position(|l| l.id == layer.id).unwrap_or(0);
    let last = app.session.doc.layers.len().saturating_sub(1);
    ui.horizontal(|ui| {
        let mut name = layer.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(110.0));
        if r.lost_focus() && name != layer.name && !name.trim().is_empty() {
            app.run("layer.set", json!({ "id": layer.id, "name": name }));
        }
        ui.label(format!("#{} of {}", position + 1, last + 1));
        if ui.add_enabled(position > 0, egui::Button::new("Earlier")).on_hover_text("Cut this layer sooner").clicked() {
            app.run("layer.move", json!({ "id": layer.id, "index": position - 1 }));
        }
        if ui.add_enabled(position < last, egui::Button::new("Later")).on_hover_text("Cut this layer later").clicked() {
            app.run("layer.move", json!({ "id": layer.id, "index": position + 1 }));
        }
    });
}

/// Buttons for the selected node while the Nodes tool is active.
fn node_section(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    if app.tool != Tool::Nodes {
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new("Nodes").strong());
    let Some((shape, np)) = crate::nodes_ui::world_nodes(app) else {
        let text_selected = app.session.selection.len() == 1
            && app.session.selection.first().and_then(|id| app.session.doc.shape(*id)).is_some_and(|s| matches!(s.kind, ShapeKind::Text { .. }));
        if text_selected {
            ui.weak("Text must be converted to a path before its nodes can be edited.");
            if ui.button("Convert text to path").clicked() {
                app.run("edit.convert_to_path", json!({}));
            }
        } else {
            ui.weak("Select one path to edit its nodes.");
        }
        return;
    };
    ui.weak("Drag nodes and handles. Double-click a line to add a node. Delete removes the selected node.");
    let Some(sel) = app.node_sel.filter(|s| s.shape == shape) else {
        ui.weak(format!("{} nodes. Click a node to select it.", np.node_count()));
        return;
    };
    let Some(node) = np.subpaths.get(sel.subpath).and_then(|s| s.nodes.get(sel.index)) else { return };
    let closed = np.subpaths.get(sel.subpath).is_some_and(|s| s.closed);
    let segment_is_curve = {
        let sp = &np.subpaths[sel.subpath];
        let next = &sp.nodes[(sel.index + 1) % sp.nodes.len()];
        lightfoundry_geom::nodes::controls(node, next).is_some()
    };
    ui.label(format!("Node {} of {}", sel.index + 1, np.subpaths[sel.subpath].nodes.len()));
    ui.horizontal_wrapped(|ui| {
        let base = json!({ "id": shape, "subpath": sel.subpath });
        let with = |extra: serde_json::Value| {
            let mut v = base.clone();
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            v
        };
        if ui.button("Delete node").clicked() {
            crate::nodes_ui::delete_selected(app);
        }
        if ui.button(if node.smooth { "Make corner" } else { "Make smooth" }).clicked() {
            app.run("node.set_smooth", with(json!({ "index": sel.index, "smooth": !node.smooth })));
        }
        if ui.button(if segment_is_curve { "Next segment: line" } else { "Next segment: curve" }).clicked() {
            app.run("node.set_segment", with(json!({ "segment": sel.index, "curve": !segment_is_curve })));
        }
        if ui.button(if closed { "Open path" } else { "Close path" }).clicked() {
            app.run("node.set_closed", with(json!({ "closed": !closed })));
        }
    });
}
