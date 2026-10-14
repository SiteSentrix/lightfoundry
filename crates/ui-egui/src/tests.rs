//! Headless frame tests: real egui input events through the real layout, no window.

use egui::{Event, Modifiers, PointerButton, RawInput, pos2, vec2};
use lightfoundry_geom::Point;
use serde_json::json;

use super::*;

const SCREEN: egui::Vec2 = vec2(1280.0, 800.0);

struct Harness {
    ctx: egui::Context,
    app: LightFoundryApp,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let mut h = Harness { ctx: egui::Context::default(), app: LightFoundryApp::new(Session::new(), Box::new(NoPlatform)), time: 0.0 };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.016;
        let input =
            RawInput { screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), SCREEN)), time: Some(self.time), events, ..Default::default() };
        let app = &mut self.app;
        self.ctx.run_ui(input, |ui| app.ui(ui)).drop_without_applying_deltas();
    }

    /// Screen position of a workspace point, using the view the app settled on.
    fn at(&self, x: f64, y: f64) -> egui::Pos2 {
        self.app.view.map(|v| v.to_screen(Point::new(x, y))).unwrap_or_default()
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let button = PointerButton::Primary;
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Event::PointerButton { pos: from, button, pressed: true, modifiers: Modifiers::NONE }]);
        let mid = from + (to - from) * 0.5;
        self.frame(vec![Event::PointerMoved(mid)]);
        self.frame(vec![Event::PointerMoved(to)]);
        self.frame(vec![Event::PointerButton { pos: to, button, pressed: false, modifiers: Modifiers::NONE }]);
        self.frame(vec![]);
    }

    fn click(&mut self, at: egui::Pos2) {
        self.drag(at, at);
    }
}

/// A platform with a private recovery folder, and canned answers for dialogs.
struct RecoveryPlatform {
    file: std::path::PathBuf,
    open_answer: Option<std::path::PathBuf>,
}

impl Platform for RecoveryPlatform {
    fn pick_open(&self, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        self.open_answer.clone()
    }

    fn pick_save(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
        None
    }

    fn recovery_file(&self) -> Option<std::path::PathBuf> {
        Some(self.file.clone())
    }

    fn clear_recovery(&self) {
        let _ = std::fs::remove_file(&self.file);
    }

    fn serial_ports(&self) -> Vec<String> {
        Vec::new()
    }

    fn open_serial(&self, _: &str) -> Result<Box<dyn lightfoundry_device::Transport + Send>, String> {
        Err("none".into())
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lf-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness_with(platform: RecoveryPlatform) -> Harness {
    let mut h = Harness { ctx: egui::Context::default(), app: LightFoundryApp::new(Session::new(), Box::new(platform)), time: 0.0 };
    h.frame(vec![]);
    h.frame(vec![]);
    h
}

mod recovery_tests {
    use super::*;

    #[test]
    fn changes_are_autosaved_and_the_copy_is_removed_once_saved() {
        let dir = temp_dir("autosave");
        let file = dir.join("recovery.lightfoundry");
        let mut h = harness_with(RecoveryPlatform { file: file.clone(), open_answer: None });
        h.app.autosave_interval = Duration::ZERO;
        h.frame(vec![]);
        assert!(!file.exists(), "nothing to save yet");
        h.app.run("shape.rectangle", json!({"width": 20, "height": 10}));
        h.frame(vec![]);
        assert!(file.exists(), "a changed document is written to the recovery file");
        let copy = lightfoundry_format::load(&file).unwrap();
        assert_eq!(copy.shapes.len(), 1);
        h.app.run("shape.ellipse", json!({"cx": 50, "cy": 50, "rx": 5, "ry": 5}));
        h.frame(vec![]);
        assert_eq!(lightfoundry_format::load(&file).unwrap().shapes.len(), 2, "it follows later edits");
        let saved = dir.join("saved.lightfoundry");
        h.app.run("file.save", json!({"path": saved}));
        h.frame(vec![]);
        assert!(!file.exists(), "saving removes the recovery copy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_idle_window_still_gets_a_repaint_scheduled_so_autosave_can_fire() {
        let dir = temp_dir("repaint");
        let mut h = harness_with(RecoveryPlatform { file: dir.join("recovery.lightfoundry"), open_answer: None });
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.app.last_autosave = Instant::now(); // pretend we just autosaved: the next one is due in the future
        h.time += 0.016;
        let input = RawInput { screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), SCREEN)), time: Some(h.time), ..Default::default() };
        let app = &mut h.app;
        let output = h.ctx.run_ui(input, |ui| app.ui(ui));
        let delay = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap_or(Duration::MAX);
        output.drop_without_applying_deltas();
        assert!(delay <= AUTOSAVE_INTERVAL, "a repaint must be requested within the autosave interval, got {delay:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_copy_is_offered_and_recovered_as_unsaved_work() {
        let dir = temp_dir("recover");
        let file = dir.join("recovery.lightfoundry");
        let mut previous = Session::new();
        previous.execute("shape.rectangle", &json!({"width": 30, "height": 30})).unwrap();
        previous.execute("file.autosave", &json!({"path": file})).unwrap();

        let mut h = harness_with(RecoveryPlatform { file: file.clone(), open_answer: None });
        assert!(h.app.pending_recovery.is_some(), "found at startup");
        h.app.autosave_interval = Duration::ZERO;
        h.app.run("shape.line", json!({"x1": 0, "y1": 0, "x2": 5, "y2": 5})); // typing behind the prompt must not clobber the copy
        h.frame(vec![]);
        assert_eq!(lightfoundry_format::load(&file).unwrap().shapes.len(), 1, "the previous session's copy is untouched");

        h.app.answer_recovery(true);
        assert!(h.app.pending_recovery.is_none());
        assert_eq!(h.app.session.doc.shapes.len(), 1);
        assert!(h.app.session.dirty && h.app.session.path.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discarding_the_offer_removes_the_copy() {
        let dir = temp_dir("discard");
        let file = dir.join("recovery.lightfoundry");
        Session::new().execute("file.autosave", &json!({"path": file})).unwrap();
        let mut h = harness_with(RecoveryPlatform { file: file.clone(), open_answer: None });
        assert!(h.app.pending_recovery.is_some());
        h.app.answer_recovery(false);
        assert!(h.app.pending_recovery.is_none() && !file.exists());
        assert!(h.app.session.doc.shapes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_recovery_files_are_reported_not_fatal() {
        let dir = temp_dir("corrupt");
        let file = dir.join("recovery.lightfoundry");
        std::fs::write(&file, b"half a project").unwrap();
        let mut h = harness_with(RecoveryPlatform { file: file.clone(), open_answer: None });
        h.app.answer_recovery(true);
        assert!(matches!(&h.app.status, Some((_, true))), "the failure is shown");
        assert!(h.app.session.doc.shapes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod unsaved_changes_tests {
    use super::*;

    #[test]
    fn new_on_a_clean_document_just_happens_but_a_dirty_one_asks() {
        let dir = temp_dir("confirm");
        let mut h = harness_with(RecoveryPlatform { file: dir.join("r.lightfoundry"), open_answer: None });
        h.app.request(PendingAction::New);
        assert!(h.app.pending_action.is_none(), "nothing to lose");
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.app.request(PendingAction::New);
        assert_eq!(h.app.pending_action, Some(PendingAction::New));
        assert_eq!(h.app.session.doc.shapes.len(), 1, "the document is untouched while the question is open");
        h.frame(vec![]); // the dialog renders
        assert!(h.app.has_modal());
        h.app.answer_unsaved(Answer::Cancel);
        assert!(h.app.pending_action.is_none() && h.app.session.doc.shapes.len() == 1, "cancel changes nothing");
        h.app.request(PendingAction::New);
        h.app.answer_unsaved(Answer::Save); // no file dialog answer and no path: nothing gets saved
        assert_eq!(h.app.pending_action, Some(PendingAction::New), "a save that did not happen keeps the question open");
        assert_eq!(h.app.session.doc.shapes.len(), 1);
        h.app.answer_unsaved(Answer::Discard);
        assert!(h.app.session.doc.shapes.is_empty() && h.app.pending_action.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_first_then_continuing_works_when_the_document_has_a_path() {
        let dir = temp_dir("savefirst");
        let target = dir.join("work.lightfoundry");
        let mut h = harness_with(RecoveryPlatform { file: dir.join("r.lightfoundry"), open_answer: None });
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.app.run("file.save", json!({"path": target}));
        h.app.run("shape.rectangle", json!({"width": 4, "height": 4}));
        h.app.request(PendingAction::New);
        h.app.answer_unsaved(Answer::Save);
        assert!(h.app.session.doc.shapes.is_empty(), "the new document replaced it");
        assert_eq!(lightfoundry_format::load(&target).unwrap().shapes.len(), 2, "after the changes were saved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_are_blocked_while_a_question_is_open() {
        let dir = temp_dir("modal");
        let mut h = harness_with(RecoveryPlatform { file: dir.join("r.lightfoundry"), open_answer: None });
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.app.request(PendingAction::New);
        h.frame(vec![Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert_eq!(h.app.session.doc.shapes.len(), 1, "Delete must not act behind the dialog");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_title_shows_the_name_and_an_unsaved_marker() {
        let mut s = Session::new();
        assert_eq!(window_title(&s), "Untitled — LightFoundry");
        s.execute("shape.rectangle", &json!({"width": 5, "height": 5})).unwrap();
        assert_eq!(window_title(&s), "*Untitled — LightFoundry");
        s.path = Some("/tmp/boxes.lightfoundry".into());
        s.dirty = false;
        assert_eq!(window_title(&s), "boxes.lightfoundry — LightFoundry");
    }

    #[test]
    fn a_close_request_on_unsaved_work_becomes_a_question() {
        let dir = temp_dir("close");
        let mut h = harness_with(RecoveryPlatform { file: dir.join("r.lightfoundry"), open_answer: None });
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.time += 0.016;
        let mut input = RawInput { screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), SCREEN)), time: Some(h.time), ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        let app = &mut h.app;
        h.ctx.run_ui(input, |ui| app.ui(ui)).drop_without_applying_deltas();
        assert_eq!(h.app.pending_action, Some(PendingAction::Close));
        assert!(!h.app.allow_close);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod frame_tests {
    use super::*;

    #[test]
    fn an_empty_document_renders() {
        let h = Harness::new();
        assert!(h.app.view.is_some());
    }

    #[test]
    fn a_busy_document_renders_with_preview_and_travel() {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 50, "height": 30}));
        h.app.run("shape.ellipse", json!({"cx": 100, "cy": 100, "rx": 20, "ry": 10, "layer": 2}));
        h.app.run("layer.set", json!({"id": 0, "mode": "fill_and_line", "interval": 1.0}));
        h.app.show_preview = true;
        h.app.show_travel = true;
        h.frame(vec![]);
        assert!(h.app.preview.error.is_none(), "{:?}", h.app.preview.error);
        assert!(!h.app.preview.segments.is_empty());
    }

    #[test]
    fn a_failing_plan_is_shown_not_hidden() {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"x": 1, "y": 1, "width": 5, "height": 5}));
        h.app.session.doc.layers[0].speed = f64::NAN; // bypasses command validation on purpose
        h.app.session.revision += 1;
        h.app.show_preview = true;
        h.frame(vec![]);
        assert!(h.app.preview.error.is_some());
    }
}

mod tool_tests {
    use super::*;

    #[test]
    fn rectangle_tool_creates_a_rectangle_of_the_dragged_size() {
        let mut h = Harness::new();
        h.app.tool = Tool::Rectangle;
        h.drag(h.at(50.0, 50.0), h.at(90.0, 80.0));
        assert_eq!(h.app.session.doc.shapes.len(), 1, "{:?}", h.app.status);
        let b = h.app.session.doc.bounds_of(&h.app.session.selection).unwrap();
        assert!((b.width() - 40.0).abs() < 1.0 && (b.height() - 30.0).abs() < 1.0, "{b:?}");
    }

    #[test]
    fn ellipse_line_and_polygon_tools_create_shapes() {
        let mut h = Harness::new();
        for tool in [Tool::Ellipse, Tool::Line, Tool::Polygon] {
            h.app.tool = tool;
            h.drag(h.at(100.0, 100.0), h.at(140.0, 130.0));
        }
        assert_eq!(h.app.session.doc.shapes.len(), 3);
    }

    #[test]
    fn a_tiny_drag_creates_nothing() {
        let mut h = Harness::new();
        h.app.tool = Tool::Rectangle;
        h.click(h.at(50.0, 50.0));
        assert!(h.app.session.doc.shapes.is_empty());
    }

    #[test]
    fn new_shapes_land_on_the_active_layer() {
        let mut h = Harness::new();
        h.app.active_layer = 4;
        h.app.tool = Tool::Rectangle;
        h.drag(h.at(20.0, 20.0), h.at(60.0, 60.0));
        assert_eq!(h.app.session.doc.shapes[0].layer, 4);
    }
}

mod text_tool_tests {
    use super::*;

    #[test]
    fn clicking_with_the_text_tool_places_text_and_returns_to_select() {
        let mut h = Harness::new();
        h.app.tool = Tool::Text;
        h.app.active_layer = 3;
        h.click(h.at(60.0, 70.0));
        assert_eq!(h.app.session.doc.shapes.len(), 1, "{:?}", h.app.status);
        let shape = &h.app.session.doc.shapes[0];
        assert_eq!(shape.layer, 3);
        assert!(matches!(&shape.kind, lightfoundry_doc::ShapeKind::Text { content, .. } if content == "Text"));
        let b = shape.world_bounds().unwrap();
        assert!((b.x0 - 60.0).abs() < 1.5 && (b.y0 - 70.0).abs() < 1.5, "{b:?}");
        assert_eq!(h.app.tool, Tool::Select);
        assert_eq!(h.app.session.selection, vec![shape.id]);
    }

    #[test]
    fn the_inspector_follows_the_selected_text_and_edits_apply() {
        let mut h = Harness::new();
        h.app.run("text.create", json!({"text": "ONE", "x": 10, "y": 10}));
        h.frame(vec![]);
        assert!(matches!(&h.app.text_buffer, Some((_, t)) if t == "ONE"));
        h.app.run("text.edit", json!({"text": "TWO"}));
        h.app.text_buffer = None; // what the inspector does after committing
        h.frame(vec![]);
        assert!(matches!(&h.app.text_buffer, Some((_, t)) if t == "TWO"));
        h.app.run("edit.select", json!({"ids": []}));
        h.frame(vec![]);
        assert!(h.app.text_buffer.is_none());
    }

    #[test]
    fn text_is_selected_by_clicking_anywhere_in_its_block_but_not_beside_it() {
        let mut h = Harness::new();
        h.app.run("text.create", json!({"text": "TEXT", "x": 100, "y": 100, "height": 20}));
        h.app.run("edit.select", json!({"ids": []}));
        h.frame(vec![]);
        // Between the stem of the T and the E: no stroke is near, but it is inside the block.
        h.click(h.at(104.0, 112.0));
        assert_eq!(h.app.session.selection.len(), 1, "inside the block");
        h.click(h.at(104.0, 150.0));
        assert!(h.app.session.selection.is_empty(), "well below the block");
    }

    #[test]
    fn unsupported_characters_raise_a_visible_warning() {
        let mut h = Harness::new();
        if let Some(v) = h.app.run("text.create", json!({"text": "caf\u{e9}"})) {
            h.app.note_unsupported(&v);
        }
        assert!(h.app.text_warning.as_deref().is_some_and(|w| w.contains('\u{e9}')));
        if let Some(v) = h.app.run("text.edit", json!({"text": "cafe"})) {
            h.app.note_unsupported(&v);
        }
        assert!(h.app.text_warning.is_none());
    }
}

mod image_tests {
    use std::sync::Arc;

    use lightfoundry_doc::{ImageAdjust, RasterImage, Shape, ShapeKind};
    use lightfoundry_geom::Affine;

    use super::*;

    fn add_gradient(h: &mut Harness, transform: Affine) -> u64 {
        let gray: Vec<u8> = (0..16 * 16).map(|i| ((i % 16) * 16) as u8).collect();
        let image = Arc::new(RasterImage::new(16, 16, gray).unwrap());
        let id = h.app.session.doc.add_shape(Shape {
            id: 0,
            name: "pic".into(),
            layer: 0,
            transform,
            kind: ShapeKind::Image { image, width_mm: 40.0, height_mm: 40.0, adjust: ImageAdjust::default() },
        });
        h.app.session.revision += 1;
        id
    }

    #[test]
    fn images_get_a_texture_that_follows_their_adjustment_and_is_dropped_with_the_shape() {
        let mut h = Harness::new();
        let id = add_gradient(&mut h, Affine::translate((20.0, 20.0)));
        h.frame(vec![]);
        assert!(h.app.thumbs.map_len() == 1);
        let before = h.app.thumbs.cached_key(id);
        h.app.run("image.adjust", json!({"id": id, "negative": true}));
        h.frame(vec![]);
        assert_ne!(h.app.thumbs.cached_key(id), before, "a changed adjustment rebuilds the thumbnail");
        let key = h.app.thumbs.cached_key(id);
        h.app.run("edit.move", json!({"ids": [id], "dx": 10, "dy": 0}));
        h.frame(vec![]);
        assert_eq!(h.app.thumbs.cached_key(id), key, "moving does not rebuild it");
        h.app.run("edit.delete", json!({"ids": [id]}));
        h.frame(vec![]);
        assert_eq!(h.app.thumbs.map_len(), 0);
    }

    #[test]
    fn rotated_and_flipped_images_render_and_are_selectable() {
        let mut h = Harness::new();
        let id = add_gradient(&mut h, Affine::translate((80.0, 80.0)) * Affine::rotate(0.7) * Affine::scale_non_uniform(-1.0, 1.0));
        h.frame(vec![]);
        h.frame(vec![]);
        let c = h.app.session.doc.bounds_of(&[id]).unwrap().center();
        h.click(h.at(c.x, c.y));
        assert_eq!(h.app.session.selection, vec![id]);
        h.frame(vec![]); // the Image inspector is drawn for it
    }
}

mod array_and_radius_tests {
    use super::*;

    #[test]
    fn the_rectangle_tool_uses_the_corner_radius() {
        let mut h = Harness::new();
        h.app.tool = Tool::Rectangle;
        h.app.corner_radius = 6.0;
        h.drag(h.at(50.0, 50.0), h.at(110.0, 100.0));
        let shape = &h.app.session.doc.shapes[0];
        assert_eq!(shape.name, "Rounded rectangle", "{:?}", h.app.status);
        assert!(!shape.world_polylines()[0].contains(Point::new(50.2, 50.2)), "the corner is cut away");
    }

    #[test]
    fn the_array_section_drives_both_array_commands() {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        h.frame(vec![]); // renders the Array section
        h.app.array_columns = 2;
        h.app.array_rows = 2;
        h.app.run("arrange.grid", json!({"columns": h.app.array_columns, "rows": h.app.array_rows}));
        assert_eq!(h.app.session.doc.shapes.len(), 4);
        h.app.run("arrange.circular", json!({"count": h.app.array_count, "radius": h.app.array_radius}));
        assert_eq!(h.app.session.doc.shapes.len(), 4 * 6, "the whole grid is rotated as one selection");
    }
}

mod shape_operation_tests {
    use super::*;

    fn two_squares(h: &mut Harness) {
        h.app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 40, "height": 40}));
        h.app.run("shape.rectangle", json!({"x": 40, "y": 40, "width": 40, "height": 40}));
        h.app.run("edit.select_all", serde_json::Value::Null);
        h.frame(vec![]);
    }

    #[test]
    fn menu_and_panel_defaults_drive_the_engine_commands() {
        let mut h = Harness::new();
        two_squares(&mut h);
        h.app.run("path.boolean", json!({"op": "union"}));
        assert_eq!(h.app.session.doc.shapes.len(), 1);
        h.app.offset_distance = 3.0;
        h.app.offset_join = "miter";
        let params = json!({ "distance": h.app.offset_distance, "join": h.app.offset_join, "keep_originals": h.app.offset_keep });
        h.app.run("path.offset", params);
        assert_eq!(h.app.session.doc.shapes.len(), 2, "originals are kept by default");
        h.frame(vec![]);
    }

    #[test]
    fn join_from_the_panel_closes_a_loop_of_lines() {
        let mut h = Harness::new();
        for (a, b) in [((10, 10), (50, 10)), ((50, 10), (50, 40)), ((50, 40), (10, 40)), ((10, 40), (10, 10))] {
            h.app.run("shape.line", json!({"x1": a.0, "y1": a.1, "x2": b.0, "y2": b.1}));
        }
        h.app.run("edit.select_all", serde_json::Value::Null);
        h.frame(vec![]);
        h.app.run("path.join", json!({}));
        assert_eq!(h.app.session.doc.shapes.len(), 1);
        assert!(h.app.session.doc.shapes[0].world_polylines()[0].closed);
    }

    #[test]
    fn a_boolean_with_one_shape_selected_reports_instead_of_crashing() {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"width": 10, "height": 10}));
        assert!(h.app.run("path.boolean", json!({"op": "union"})).is_none());
        assert!(matches!(&h.app.status, Some((m, true)) if m.contains("at least two")), "{:?}", h.app.status);
    }
}

mod select_tests {
    use super::*;

    fn with_two_rects() -> Harness {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 40, "height": 40}));
        h.app.run("shape.rectangle", json!({"x": 200, "y": 200, "width": 40, "height": 40}));
        h.app.run("edit.select", json!({"ids": []}));
        h.frame(vec![]);
        h
    }

    #[test]
    fn click_selects_and_empty_click_clears() {
        let mut h = with_two_rects();
        h.click(h.at(40.0, 40.0));
        assert_eq!(h.app.session.selection.len(), 1);
        h.click(h.at(120.0, 120.0));
        assert!(h.app.session.selection.is_empty());
    }

    #[test]
    fn dragging_a_shape_moves_it_as_one_undo_step() {
        let mut h = with_two_rects();
        let before = h.app.session.doc.shape(1).unwrap().world_bounds().unwrap();
        h.drag(h.at(40.0, 40.0), h.at(70.0, 55.0));
        let after = h.app.session.doc.shape(1).unwrap().world_bounds().unwrap();
        assert!((after.x0 - before.x0 - 30.0).abs() < 1.0 && (after.y0 - before.y0 - 15.0).abs() < 1.0, "{before:?} -> {after:?}");
        h.app.run("edit.undo", serde_json::Value::Null);
        assert_eq!(h.app.session.doc.shape(1).unwrap().world_bounds().unwrap(), before);
    }

    #[test]
    fn rubber_band_selects_fully_enclosed_shapes() {
        let mut h = with_two_rects();
        h.drag(h.at(10.0, 10.0), h.at(260.0, 260.0));
        assert_eq!(h.app.session.selection.len(), 2);
        h.drag(h.at(5.0, 5.0), h.at(70.0, 70.0));
        assert_eq!(h.app.session.selection, vec![1]);
    }

    #[test]
    fn dragging_a_corner_handle_resizes_in_one_step() {
        let mut h = with_two_rects();
        h.click(h.at(40.0, 40.0));
        h.drag(h.at(60.0, 60.0), h.at(100.0, 120.0)); // bottom-right handle of the 40x40 rect at (20,20)
        let b = h.app.session.doc.shape(1).unwrap().world_bounds().unwrap();
        assert!((b.x0 - 20.0).abs() < 1.0 && (b.width() - 80.0).abs() < 1.5 && (b.height() - 100.0).abs() < 1.5, "{b:?}");
        h.app.run("edit.undo", serde_json::Value::Null);
        let b = h.app.session.doc.shape(1).unwrap().world_bounds().unwrap();
        assert!((b.width() - 40.0).abs() < 1e-6);
    }

    #[test]
    fn delete_key_and_undo_shortcut_work() {
        let mut h = with_two_rects();
        h.click(h.at(40.0, 40.0));
        h.frame(vec![Event::Key { key: egui::Key::Delete, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert_eq!(h.app.session.doc.shapes.len(), 1);
        h.frame(vec![Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        assert_eq!(h.app.session.doc.shapes.len(), 2);
    }
}

mod machine_tests {
    use std::time::{Duration, Instant};

    use lightfoundry_device::JobState;
    use lightfoundry_doc::Origin;

    use super::*;

    /// Runs frames until `done` holds (the worker thread updates asynchronously).
    fn pump(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
        let start = Instant::now();
        while !done(h) {
            h.frame(vec![]);
            assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}; status {:?}", h.app.status);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn position(h: &Harness) -> Option<(f64, f64)> {
        h.app.snap.as_ref().and_then(|s| s.status.as_ref()).and_then(|st| st.position).map(|(x, y, _)| (x, y))
    }

    fn connected() -> Harness {
        let mut h = Harness::new();
        h.app.connect();
        pump(&mut h, "the first status report", |h| position(h).is_some());
        h
    }

    #[test]
    fn simulator_job_runs_to_completion_and_reports() {
        let mut h = connected();
        h.app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20}));
        let program = h.app.run("job.gcode", json!({"include": true})).and_then(|v| v["gcode"].as_str().map(String::from)).unwrap();
        h.app.start_job(program);
        pump(&mut h, "the job", |h| matches!(h.app.snap.as_ref().map(|s| &s.job), Some(JobState::Finished(_))));
        h.frame(vec![]);
        assert!(matches!(&h.app.status, Some((m, false)) if m.starts_with("Job finished")), "{:?}", h.app.status);
    }

    #[test]
    fn jog_directions_follow_the_machine_origin() {
        assert_eq!(panels::machine_delta(Origin::TopLeft, 0.0, -5.0), (0.0, -5.0));
        assert_eq!(panels::machine_delta(Origin::BottomLeft, 0.0, -5.0), (0.0, 5.0), "screen-up is +Y on a bottom-left machine");
        assert_eq!(panels::machine_delta(Origin::BottomLeft, 3.0, 0.0), (3.0, 0.0));
    }

    #[test]
    fn a_jog_through_the_link_moves_the_simulated_head() {
        let mut h = connected();
        h.app.link_do(|l| l.jog(0.0, 10.0, 3000.0));
        pump(&mut h, "the jog", |h| position(h) == Some((0.0, 10.0)));
    }

    #[test]
    fn starting_without_a_connection_explains_what_to_do() {
        let mut h = Harness::new();
        h.app.start_job("G21\n".into());
        assert!(matches!(&h.app.status, Some((m, true)) if m.contains("Connect")), "{:?}", h.app.status);
    }

    #[test]
    fn serial_target_without_a_platform_reports_instead_of_crashing() {
        let mut h = Harness::new();
        h.app.target = Target::Serial("/dev/ttyUSB0".into());
        h.app.connect();
        assert!(matches!(&h.app.status, Some((m, true)) if m.contains("Could not open")), "{:?}", h.app.status);
        assert!(h.app.link.is_none());
    }

    #[test]
    fn disconnect_mid_job_cancels_and_clears_state() {
        let mut h = connected();
        let program: String = (0..3000).map(|i| format!("G1 X{} Y0 F100\n", i % 100)).collect();
        h.app.start_job(program);
        pump(&mut h, "the job to start", |h| h.app.job_active());
        h.app.disconnect();
        assert!(h.app.link.is_none() && h.app.snap.is_none());
        h.frame(vec![]);
    }

    #[test]
    fn job_origin_controls_drive_the_engine_command() {
        let mut h = Harness::new();
        h.app.run("job.set_origin", json!({"start_from": "user_origin", "anchor": "center"}));
        h.frame(vec![]);
        assert_eq!(h.app.session.doc.job_origin.anchor, lightfoundry_doc::Anchor::Center);
    }

    #[test]
    fn the_laser_panel_renders_connected_and_disconnected() {
        let mut h = connected();
        for _ in 0..3 {
            h.frame(vec![]);
        }
        h.app.disconnect();
        h.frame(vec![]);
    }
}
