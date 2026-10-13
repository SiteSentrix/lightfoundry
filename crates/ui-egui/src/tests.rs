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

mod job_tests {
    use super::*;

    #[test]
    fn simulator_job_runs_to_completion_and_reports() {
        let mut h = Harness::new();
        h.app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20}));
        let program = h.app.run("job.gcode", json!({"include": true})).and_then(|v| v["gcode"].as_str().map(String::from)).unwrap();
        h.app.start_job(program);
        let start = std::time::Instant::now();
        while h.app.runner.as_ref().is_some_and(|r| r.is_running()) {
            h.frame(vec![]);
            assert!(start.elapsed() < std::time::Duration::from_secs(10), "job hung");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        h.frame(vec![]);
        assert!(matches!(h.app.run_result, Some(Ok(_))), "{:?}", h.app.run_result);
    }

    #[test]
    fn serial_target_without_a_platform_reports_instead_of_crashing() {
        let mut h = Harness::new();
        h.app.target = Target::Serial("/dev/ttyUSB0".into());
        h.app.start_job("G21\n".into());
        assert!(matches!(&h.app.status, Some((m, true)) if m.contains("Could not open")), "{:?}", h.app.status);
        assert!(h.app.runner.is_none());
    }
}
