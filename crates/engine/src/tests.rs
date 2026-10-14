use serde_json::{Value, json};

use crate::{EngineError, Session, commands, find_command};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <rect x="10" y="10" width="30" height="30" fill="none" stroke="#ff0000"/>
  <circle cx="70" cy="70" r="10" fill="none" stroke="#0000ff"/>
</svg>"##;

fn rect(s: &mut Session) -> u64 {
    let v = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 20, "height": 10})).unwrap();
    v["id"].as_u64().unwrap()
}

mod history_tests {
    use super::*;

    #[test]
    fn undo_and_redo_restore_document_and_selection() {
        let mut s = Session::new();
        let id = rect(&mut s);
        assert_eq!(s.selection, vec![id]);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.doc.shapes.is_empty() && s.selection.is_empty());
        s.execute("edit.redo", &Value::Null).unwrap();
        assert_eq!(s.doc.shapes.len(), 1);
    }

    #[test]
    fn a_new_action_clears_redo() {
        let mut s = Session::new();
        rect(&mut s);
        s.execute("edit.undo", &Value::Null).unwrap();
        rect(&mut s);
        assert!(!s.can_redo());
    }

    #[test]
    fn failed_commands_leave_no_trace() {
        let mut s = Session::new();
        rect(&mut s);
        let before = s.doc.clone();
        assert!(s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": -5, "height": 5})).is_err());
        assert_eq!(s.doc, before);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.doc.shapes.is_empty(), "the failed command must not have created an undo step");
    }

    #[test]
    fn batch_is_atomic_and_one_undo_step() {
        let mut s = Session::new();
        let bad = json!({"commands": [
            {"id": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 5, "height": 5}},
            {"id": "shape.rectangle", "params": {"width": "nope"}},
        ]});
        assert!(s.execute("command.batch", &bad).is_err());
        assert!(s.doc.shapes.is_empty());

        let good = json!({"commands": [
            {"id": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 5, "height": 5}},
            {"id": "shape.ellipse", "params": {"cx": 20, "cy": 20, "rx": 3, "ry": 3}},
        ]});
        s.execute("command.batch", &good).unwrap();
        assert_eq!(s.doc.shapes.len(), 2);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.doc.shapes.is_empty());
    }

    #[test]
    fn history_is_bounded() {
        let mut s = Session::new();
        for _ in 0..(crate::HISTORY_LIMIT + 20) {
            rect(&mut s);
        }
        let mut undone = 0;
        while s.can_undo() {
            s.execute("edit.undo", &Value::Null).unwrap();
            undone += 1;
        }
        assert_eq!(undone, crate::HISTORY_LIMIT);
    }
}

mod edit_tests {
    use super::*;

    #[test]
    fn move_resize_rotate_change_bounds_as_expected() {
        let mut s = Session::new();
        let id = rect(&mut s);
        s.execute("edit.move", &json!({"dx": 5, "dy": -5})).unwrap();
        let b = s.doc.bounds_of(&[id]).unwrap();
        assert_eq!((b.x0, b.y0), (15.0, 5.0));
        s.execute("edit.resize", &json!({"width": 40, "lock_aspect": true})).unwrap();
        let b = s.doc.bounds_of(&[id]).unwrap();
        assert!((b.width() - 40.0).abs() < 1e-9 && (b.height() - 20.0).abs() < 1e-9);
        assert!((b.x0 - 15.0).abs() < 1e-9, "resize keeps the top-left fixed");
        s.execute("edit.rotate", &json!({"degrees": 90})).unwrap();
        let b = s.doc.bounds_of(&[id]).unwrap();
        assert!((b.width() - 20.0).abs() < 1e-6 && (b.height() - 40.0).abs() < 1e-6);
    }

    #[test]
    fn align_and_flip() {
        let mut s = Session::new();
        let a = rect(&mut s);
        s.execute("shape.rectangle", &json!({"x": 100, "y": 50, "width": 10, "height": 10})).unwrap();
        s.execute("edit.select_all", &Value::Null).unwrap();
        s.execute("edit.align", &json!({"to": "left"})).unwrap();
        let bs: Vec<_> = s.doc.shapes.iter().filter_map(|sh| sh.world_bounds()).collect();
        assert!(bs.iter().all(|b| (b.x0 - 10.0).abs() < 1e-9));
        s.execute("edit.flip", &json!({"ids": [a], "axis": "horizontal"})).unwrap();
        assert_eq!(s.doc.bounds_of(&[a]).map(|b| (b.x0.round(), b.x1.round())), Some((10.0, 30.0)));
    }

    #[test]
    fn editing_with_nothing_selected_is_an_error() {
        let mut s = Session::new();
        assert!(matches!(s.execute("edit.move", &json!({"dx": 1, "dy": 1})), Err(EngineError::NothingSelected)));
    }

    #[test]
    fn duplicate_selects_the_copy() {
        let mut s = Session::new();
        let a = rect(&mut s);
        s.execute("edit.duplicate", &json!({})).unwrap();
        assert_eq!(s.doc.shapes.len(), 2);
        assert_ne!(s.selection, vec![a]);
    }
}

mod text_tests {
    use super::*;

    #[test]
    fn text_is_created_placed_and_engraved_as_lines() {
        let mut s = Session::new();
        s.execute("machine.set", &json!({"origin": "top_left"})).unwrap();
        let v = s.execute("text.create", &json!({"text": "HELLO", "x": 20, "y": 30, "height": 12})).unwrap();
        let id = v["id"].as_u64().unwrap();
        let b = s.doc.bounds_of(&[id]).unwrap();
        assert!((b.height() - 12.0).abs() < 1e-6 && (b.x0 - 20.0).abs() < 1e-6 && (b.y0 - 30.0).abs() < 1e-6, "{b:?}");
        let g = s.execute("job.gcode", &json!({"include": true})).unwrap();
        assert!(g["gcode"].as_str().unwrap().matches("G1").count() > 50, "text becomes many cutting moves");
    }

    #[test]
    fn editing_text_regenerates_the_geometry_in_one_undo_step() {
        let mut s = Session::new();
        let id = s.execute("text.create", &json!({"text": "AB", "height": 10})).unwrap()["id"].as_u64().unwrap();
        let narrow = s.doc.bounds_of(&[id]).unwrap().width();
        s.execute("text.edit", &json!({"text": "ABCDEFG", "height": 20})).unwrap();
        let wide = s.doc.bounds_of(&[id]).unwrap();
        assert!(wide.width() > narrow * 4.0 && (wide.height() - 20.0).abs() < 1e-6, "{wide:?}");
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!((s.doc.bounds_of(&[id]).unwrap().width() - narrow).abs() < 1e-9);
    }

    #[test]
    fn a_moved_text_keeps_its_transform_when_edited() {
        let mut s = Session::new();
        let id = s.execute("text.create", &json!({"text": "HI", "x": 0, "y": 0})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.move", &json!({"dx": 40, "dy": 25})).unwrap();
        s.execute("text.edit", &json!({"text": "HELLO"})).unwrap();
        let b = s.doc.bounds_of(&[id]).unwrap();
        assert!((b.x0 - 40.0).abs() < 1e-6 && (b.y0 - 25.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn bad_text_input_is_an_error_and_unsupported_characters_are_reported() {
        let mut s = Session::new();
        assert!(s.execute("text.create", &json!({"text": "   "})).is_err());
        assert!(s.execute("text.create", &json!({"text": "A", "height": -3})).is_err());
        assert!(s.execute("text.create", &json!({"text": "A", "height": 1e12})).is_err());
        assert!(s.execute("text.edit", &json!({})).is_err(), "nothing selected");
        let v = s.execute("text.create", &json!({"text": "caf\u{e9}"})).unwrap();
        assert_eq!(v["unsupported"], json!(["\u{e9}"]));
        let rect = s.execute("shape.rectangle", &json!({"width": 5, "height": 5})).unwrap()["id"].as_u64().unwrap();
        assert!(s.execute("text.edit", &json!({"id": rect, "text": "x"})).is_err(), "only text can be edited as text");
    }

    #[test]
    fn text_survives_save_and_load() {
        let mut s = Session::new();
        s.execute("text.create", &json!({"text": "SAVE ME", "align": "center"})).unwrap();
        let back = lightfoundry_format::from_bytes(&lightfoundry_format::to_bytes(&s.doc).unwrap()).unwrap();
        assert_eq!(back.shapes, s.doc.shapes);
    }
}

mod import_tests {
    use super::*;

    #[test]
    fn svg_colours_map_to_layers() {
        let mut s = Session::new();
        s.execute("file.import_svg", &json!({"data": SVG})).unwrap();
        let layers: Vec<_> = s.doc.shapes.iter().map(|sh| sh.layer).collect();
        assert_eq!(layers, vec![2, 1]); // #ff0000 -> C02, #0000ff -> C01
    }

    #[test]
    fn dxf_layers_become_cut_layers() {
        let dxf =
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nA\n10\n0\n20\n0\n11\n10\n21\n0\n0\nLINE\n8\nB\n10\n0\n20\n5\n11\n10\n21\n5\n0\nENDSEC\n0\nEOF\n";
        let mut s = Session::new();
        s.execute("file.import_dxf", &json!({"data": dxf})).unwrap();
        let layers: Vec<_> = s.doc.shapes.iter().map(|sh| sh.layer).collect();
        assert_eq!(layers, vec![0, 1]);
    }

    #[test]
    fn imports_are_undoable_as_one_step() {
        let mut s = Session::new();
        s.execute("file.import_svg", &json!({"data": SVG})).unwrap();
        assert_eq!(s.doc.shapes.len(), 2);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.doc.shapes.is_empty());
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        let mut s = Session::new();
        assert!(s.execute("file.import_svg", &json!({"data": "<svg"})).is_err());
        assert!(s.execute("file.import_dxf", &json!({"data": "junk"})).is_err());
        assert!(s.execute("file.import_svg", &json!({})).is_err());
        assert!(s.execute("file.import_image", &json!({"path": "/definitely/not/here.png"})).is_err());
    }

    #[test]
    fn png_import_becomes_a_raster_shape() {
        let dir = std::env::temp_dir().join(format!("lf-engine-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        image::GrayImage::from_fn(10, 5, |x, _| image::Luma([(x * 25) as u8])).save(&path).unwrap();
        let mut s = Session::new();
        let v = s.execute("file.import_image", &json!({"path": path, "width": 20})).unwrap();
        assert!((v["height_mm"].as_f64().unwrap() - 10.0).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod job_tests {
    use super::*;

    fn square_session() -> Session {
        let mut s = Session::new();
        s.execute("machine.set", &json!({"origin": "top_left"})).unwrap();
        rect(&mut s);
        s
    }

    #[test]
    fn gcode_is_generated_and_streams_to_the_simulator() {
        let mut s = square_session();
        let v = s.execute("job.gcode", &json!({"include": true})).unwrap();
        assert!(v["gcode"].as_str().unwrap().contains("G1"));
        assert!(s.last_gcode.is_some());
        let sim = s.execute("job.simulate", &Value::Null).unwrap();
        assert_eq!(sim["overran_buffer"], json!(false));
        assert!(sim["lines_streamed"].as_u64().unwrap() > 5);
    }

    #[test]
    fn estimate_and_frame_work() {
        let mut s = square_session();
        assert!(s.execute("job.estimate", &Value::Null).unwrap()["seconds"].as_f64().unwrap() > 0.0);
        let f = s.execute("job.frame", &json!({})).unwrap();
        assert_eq!(f["bounds"], json!([10.0, 10.0, 30.0, 20.0]));
    }

    #[test]
    fn out_of_bed_designs_are_refused() {
        let mut s = square_session();
        s.execute("edit.move", &json!({"dx": 1000, "dy": 0})).unwrap();
        assert!(matches!(s.execute("job.gcode", &Value::Null), Err(EngineError::Gcode(_))));
    }

    #[test]
    fn start_from_current_position_makes_the_job_relative_to_its_anchor() {
        let mut s = square_session(); // 20x10 rect at (10,10), top-left origin
        s.execute("job.set_origin", &json!({"start_from": "current_position", "anchor": "center"})).unwrap();
        let g = s.execute("job.gcode", &json!({"include": true})).unwrap();
        let text = g["gcode"].as_str().unwrap();
        assert!(text.contains("G92 X0 Y0"), "{text}");
        assert!(text.contains("X-10") && text.contains("Y-5"), "centre anchor: corner is at (-10,-5)\n{text}");
        // Even a job far outside the design bed is fine when placement is relative.
        s.execute("edit.move", &json!({"dx": 5000, "dy": 0})).unwrap();
        assert!(s.execute("job.gcode", &Value::Null).is_ok());
        s.execute("job.set_origin", &json!({"start_from": "absolute"})).unwrap();
        assert!(matches!(s.execute("job.gcode", &Value::Null), Err(EngineError::Gcode(_))));
    }

    #[test]
    fn job_origin_is_undoable_and_saved() {
        let mut s = square_session();
        s.execute("job.set_origin", &json!({"anchor": "bottom_right"})).unwrap();
        let bytes = lightfoundry_format::to_bytes(&s.doc).unwrap();
        assert_eq!(lightfoundry_format::from_bytes(&bytes).unwrap().job_origin.anchor, lightfoundry_doc::Anchor::BottomRight);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.doc.job_origin.anchor, lightfoundry_doc::Anchor::TopLeft);
    }

    #[test]
    fn layer_settings_flow_through_to_gcode() {
        let mut s = square_session();
        s.execute("layer.set", &json!({"id": 0, "speed": 777, "max_power": 25, "passes": 2})).unwrap();
        let g = s.execute("job.gcode", &json!({"include": true})).unwrap();
        let text = g["gcode"].as_str().unwrap();
        assert!(text.contains("F777") && text.contains("S250"));
        assert_eq!(text.matches("pass").count(), 2);
    }

    #[test]
    fn layer_and_machine_settings_are_validated() {
        let mut s = Session::new();
        for bad in [
            json!({"id": 0, "speed": 0}),
            json!({"id": 0, "max_power": 101}),
            json!({"id": 0, "interval": -1}),
            json!({"id": 99, "speed": 10}),
            json!({"id": 0, "speed": null, "passes": "x"}),
        ] {
            assert!(s.execute("layer.set", &bad).is_err(), "{bad}");
        }
        assert!(s.execute("machine.set", &json!({"bed_width": 0})).is_err());
        assert!(s.execute("machine.set", &json!({"max_s": 0})).is_err());
    }
}

mod file_tests {
    use super::*;

    #[test]
    fn save_and_open_round_trip() {
        let dir = std::env::temp_dir().join(format!("lf-engine-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.lightfoundry");
        let mut s = Session::new();
        rect(&mut s);
        s.execute("file.save", &json!({"path": path})).unwrap();
        assert!(!s.dirty);
        let mut t = Session::new();
        t.execute("file.open", &json!({"path": path})).unwrap();
        assert_eq!(t.doc, s.doc);
        assert!(!t.can_undo(), "history does not carry across documents");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_without_a_path_is_an_error() {
        assert!(Session::new().execute("file.save", &json!({})).is_err());
    }
}

mod robustness_tests {
    use super::*;

    #[test]
    fn unknown_commands_are_errors() {
        assert!(matches!(Session::new().execute("nope.nothing", &Value::Null), Err(EngineError::UnknownCommand(_))));
    }

    #[test]
    fn a_panicking_command_is_contained_and_rolls_back() {
        fn boom(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
            s.doc.shapes.clear();
            s.selection.clear();
            panic!("kaboom");
        }
        let spec = crate::CommandSpec { id: "test.boom", label: "Boom", menu: "", shortcut: None, params_doc: "{}", mutates: true, run: boom };
        let mut s = Session::new();
        rect(&mut s);
        let before = s.doc.clone();
        let selection = s.selection.clone();
        let err = s.execute_spec_for_test(&spec, &Value::Null).unwrap_err();
        assert!(matches!(&err, EngineError::Internal(m) if m.contains("kaboom")), "{err}");
        assert_eq!(s.doc, before, "the document must be restored after a panic");
        assert_eq!(s.selection, selection);
        // The failed command is not an undo step: one undo removes only the rectangle.
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.doc.shapes.is_empty());
    }

    /// Every command, with a menagerie of hostile parameters, must return (Ok or Err) and never panic.
    #[test]
    fn command_sweep_never_panics() {
        let hostile: Vec<Value> = vec![
            Value::Null,
            json!({}),
            json!([]),
            json!("string"),
            json!({"x": f64::MAX, "y": -f64::MAX, "width": 1e308, "height": 1e-308}),
            json!({"ids": [0, 1, 18446744073709551615u64], "dx": 1e300, "dy": -1e300}),
            json!({"width": 0, "height": 0, "radius": 0, "sides": 4294967295u64, "degrees": 1e308}),
            json!({"id": -1, "layer": 4294967295u64, "speed": "fast"}),
            json!({"path": "/nonexistent/\u{0}/x", "data": "\u{0}\u{1}\u{2}"}),
            json!({"commands": [{"id": "command.batch"}, {"id": "edit.delete"}]}),
        ];
        for spec in commands() {
            for params in &hostile {
                let mut s = Session::new();
                rect(&mut s);
                s.execute("shape.ellipse", &json!({"cx": 5, "cy": 5, "rx": 2, "ry": 2})).unwrap();
                let _ = s.execute(spec.id, params);
                // Invariants that must survive any input: layers intact, shapes reference real layers
                // and have finite geometry, and the planner still returns instead of panicking.
                assert_eq!(s.doc.layers.len(), 16, "{} with {params}", spec.id);
                for sh in &s.doc.shapes {
                    assert!(s.doc.layer(sh.layer).is_some(), "{} with {params}: dangling layer", spec.id);
                    let finite = sh.world_bounds().is_none_or(|b| b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite());
                    assert!(finite, "{} with {params}: non-finite geometry", spec.id);
                }
                let _ = s.execute("job.estimate", &Value::Null);
            }
        }
    }

    #[test]
    fn every_command_has_docs_and_a_unique_id() {
        let mut seen = std::collections::HashSet::new();
        for c in commands() {
            assert!(seen.insert(c.id), "duplicate id {}", c.id);
            assert!(!c.label.is_empty() && !c.params_doc.is_empty(), "{} lacks docs", c.id);
            assert!(find_command(c.id).is_some());
        }
        assert!(seen.len() >= 25, "{}", seen.len());
    }
}
