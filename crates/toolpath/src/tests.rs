use std::sync::Arc;

use lightfoundry_doc::{Document, LayerMode, RasterImage, Shape, ShapeKind};
use lightfoundry_geom::{Affine, Point, Polyline, ellipse_path, rect_path};

use super::*;

fn rect_doc(w: f64, h: f64, mode: LayerMode) -> Document {
    let mut doc = Document::default();
    if let Some(l) = doc.layer_mut(0) {
        l.mode = mode;
    }
    doc.add_shape(Shape::path("r", 0, rect_path(10.0, 10.0, w, h)));
    doc
}

mod line_tests {
    use super::*;

    #[test]
    fn rectangle_is_travel_then_four_cuts_back_to_start() {
        let job = plan(&rect_doc(20.0, 10.0, LayerMode::Line)).unwrap();
        assert_eq!(job.ops.len(), 1);
        let moves = &job.ops[0].moves;
        assert!(matches!(moves[0], Move::Travel(_)));
        assert_eq!(moves.len(), 1 + 4);
        assert_eq!(moves.last().map(Move::target), moves.first().map(Move::target));
    }

    #[test]
    fn inner_shapes_are_cut_before_outer() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("outer", 0, rect_path(0.0, 0.0, 100.0, 100.0)));
        doc.add_shape(Shape::path("inner", 0, ellipse_path(50.0, 50.0, 10.0, 10.0)));
        let job = plan(&doc).unwrap();
        let first = job.ops[0].moves[0].target();
        // The circle (near the centre) is cut first even though the rectangle's corner is nearer the origin.
        assert!(first.distance(Point::new(50.0, 50.0)) < 15.0, "{first:?}");
    }

    #[test]
    fn disabled_layers_produce_no_output() {
        let mut doc = rect_doc(5.0, 5.0, LayerMode::Line);
        if let Some(l) = doc.layer_mut(0) {
            l.output = false;
        }
        assert!(plan(&doc).unwrap().ops.is_empty());
    }
}

mod kerf_tests {
    use super::*;

    fn bounds(job: &Job) -> lightfoundry_geom::Rect {
        lightfoundry_geom::bounds_of_points(job.ops.iter().flat_map(|o| o.moves.iter().map(Move::target))).unwrap()
    }

    #[test]
    fn kerf_grows_the_cut_path_around_a_part_and_shrinks_it_inside_a_hole() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("part", 0, rect_path(0.0, 0.0, 40.0, 40.0)));
        doc.add_shape(Shape::path("hole", 0, rect_path(10.0, 10.0, 20.0, 20.0)));
        let plain = bounds(&plan(&doc).unwrap());
        assert_eq!((plain.x0, plain.x1), (0.0, 40.0));
        doc.layer_mut(0).unwrap().kerf_offset = 0.1;
        let job = plan(&doc).unwrap();
        let b = bounds(&job);
        assert!((b.x0 + 0.1).abs() < 1e-6 && (b.x1 - 40.1).abs() < 1e-6, "outer path moved out by 0.1: {b:?}");
        // The hole contour is cut first (inner-first) and sits 0.1 mm *inside* the hole.
        let first = job.ops[0].moves[0].target();
        assert!(first.x > 10.05 && first.x < 29.95, "{first:?}");
    }

    #[test]
    fn negative_kerf_shrinks_parts_and_open_paths_are_untouched() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("part", 0, rect_path(0.0, 0.0, 40.0, 40.0)));
        doc.add_shape(Shape::path("line", 0, lightfoundry_geom::line_path(Point::new(100.0, 100.0), Point::new(120.0, 100.0))));
        doc.layer_mut(0).unwrap().kerf_offset = -0.2;
        let job = plan(&doc).unwrap();
        let xs: Vec<f64> = job.ops[0].moves.iter().map(|m| m.target().x).collect();
        assert!(xs.iter().any(|x| (x - 0.2).abs() < 1e-6) && xs.iter().any(|x| (x - 39.8).abs() < 1e-6));
        assert!(xs.iter().any(|x| (x - 100.0).abs() < 1e-9) && xs.iter().any(|x| (x - 120.0).abs() < 1e-9), "the open line is cut as drawn");
    }

    #[test]
    fn a_shape_that_collapses_under_kerf_is_dropped_and_bad_kerf_is_rejected() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("tiny", 0, rect_path(0.0, 0.0, 0.1, 0.1)));
        doc.layer_mut(0).unwrap().kerf_offset = -1.0;
        assert!(plan(&doc).unwrap().ops.is_empty());
        for bad in [f64::NAN, 1e9] {
            doc.layer_mut(0).unwrap().kerf_offset = bad;
            assert!(matches!(plan(&doc), Err(PlanError::BadLayerSetting { .. })), "{bad}");
        }
    }

    #[test]
    fn fill_ignores_kerf() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("part", 0, rect_path(0.0, 0.0, 10.0, 10.0)));
        let layer = doc.layer_mut(0).unwrap();
        layer.mode = LayerMode::Fill;
        layer.interval = 1.0;
        layer.kerf_offset = 0.5;
        let b = bounds(&plan(&doc).unwrap());
        assert_eq!((b.x0, b.x1), (0.0, 10.0));
    }
}

mod override_tests {
    use lightfoundry_doc::CutOverride;

    use super::*;

    fn two_squares() -> Document {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("plain", 0, rect_path(0.0, 0.0, 10.0, 10.0)));
        doc.add_shape(Shape::path("special", 0, rect_path(50.0, 0.0, 10.0, 10.0)));
        let layer = doc.layer_mut(0).unwrap();
        layer.speed = 1000.0;
        layer.max_power = 40.0;
        doc
    }

    #[test]
    fn an_overridden_shape_runs_at_its_own_speed_and_power_and_the_rest_do_not() {
        let mut doc = two_squares();
        doc.shapes[1].cut = Some(CutOverride { speed: Some(300.0), max_power: Some(90.0), ..Default::default() });
        let job = plan(&doc).unwrap();
        assert_eq!(job.ops.len(), 2);
        let (plain, special) = (&job.ops[0], &job.ops[1]);
        assert_eq!(plain.speed, 1000.0);
        assert!(plain.moves.iter().all(|m| !matches!(m, Move::Cut { power, .. } if *power != 40.0)));
        assert_eq!(special.speed, 300.0);
        assert!(special.moves.iter().filter(|m| matches!(m, Move::Cut { .. })).all(|m| matches!(m, Move::Cut { power, .. } if *power == 90.0)));
        assert!(special.moves[0].target().x >= 50.0, "the overridden square is the second op");
    }

    #[test]
    fn shapes_with_equal_overrides_share_an_op_and_empty_overrides_are_ignored() {
        let mut doc = two_squares();
        doc.shapes[0].cut = Some(CutOverride::default());
        doc.shapes[1].cut = Some(CutOverride { passes: Some(3), ..Default::default() });
        let job = plan(&doc).unwrap();
        assert_eq!(job.ops.len(), 2);
        assert_eq!((job.ops[0].passes, job.ops[1].passes), (1, 3));
        let mut doc = two_squares();
        let same = Some(CutOverride { speed: Some(500.0), ..Default::default() });
        doc.shapes[0].cut = same;
        doc.shapes[1].cut = same;
        assert_eq!(plan(&doc).unwrap().ops.len(), 1, "one op for both");
    }

    #[test]
    fn bad_override_values_are_rejected_like_bad_layer_values() {
        let mut doc = two_squares();
        doc.shapes[1].cut = Some(CutOverride { speed: Some(f64::NAN), ..Default::default() });
        assert!(matches!(plan(&doc), Err(PlanError::BadLayerSetting { .. })));
        doc.shapes[1].cut = Some(CutOverride { speed: Some(-5.0), ..Default::default() });
        assert!(matches!(plan(&doc), Err(PlanError::BadLayerSetting { .. })));
    }
}

mod tab_and_lead_tests {
    use super::*;

    fn square_doc(size: f64, at: f64) -> Document {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("part", 0, rect_path(at, at, size, size)));
        doc
    }

    /// Lengths of (powered, unpowered) cut moves in the first op, from each move's predecessor.
    fn lengths(job: &Job) -> (f64, f64) {
        let (mut on, mut off) = (0.0, 0.0);
        let mut at = None;
        for m in &job.ops[0].moves {
            if let (Some(a), Move::Cut { to, power }) = (at, m) {
                let d = Point::distance(a, *to);
                if *power > 0.0 { on += d } else { off += d }
            }
            at = Some(m.target());
        }
        (on, off)
    }

    #[test]
    fn tabs_leave_gaps_with_the_laser_off_and_keep_cutting_speed() {
        let mut doc = square_doc(40.0, 10.0);
        let layer = doc.layer_mut(0).unwrap();
        layer.tab_count = 4;
        layer.tab_width = 2.0;
        let job = plan(&doc).unwrap();
        let (on, off) = lengths(&job);
        assert!((off - 8.0).abs() < 1e-6 && (on - 152.0).abs() < 1e-6, "{on} {off}");
        let gaps = job.ops[0]
            .moves
            .windows(2)
            .filter(|w| matches!(w[1], Move::Cut { power, .. } if power == 0.0) && !matches!(w[0], Move::Cut { power, .. } if power == 0.0))
            .count();
        assert_eq!(gaps, 4, "four separate tabs");
    }

    #[test]
    fn holes_get_no_tabs_but_outer_outlines_do() {
        let mut doc = square_doc(60.0, 10.0);
        doc.add_shape(Shape::path("hole", 0, rect_path(30.0, 30.0, 10.0, 10.0)));
        let layer = doc.layer_mut(0).unwrap();
        layer.tab_count = 2;
        layer.tab_width = 3.0;
        let (_, off) = lengths(&plan(&doc).unwrap());
        assert!((off - 6.0).abs() < 1e-6, "two tabs on the outer outline only, got {off}");
    }

    #[test]
    fn tabs_that_do_not_fit_leave_the_outline_whole() {
        let mut doc = square_doc(4.0, 10.0);
        let layer = doc.layer_mut(0).unwrap();
        layer.tab_count = 4;
        layer.tab_width = 3.0;
        let (on, off) = lengths(&plan(&doc).unwrap());
        assert_eq!(off, 0.0);
        assert!((on - 16.0).abs() < 1e-6);
    }

    #[test]
    fn a_lead_in_approaches_from_the_scrap_side_and_a_hole_leads_inside_itself() {
        let mut doc = square_doc(60.0, 10.0);
        doc.add_shape(Shape::path("hole", 0, rect_path(30.0, 30.0, 10.0, 10.0)));
        let layer = doc.layer_mut(0).unwrap();
        layer.lead_in = 4.0;
        layer.lead_out = 2.0;
        let job = plan(&doc).unwrap();
        let moves = &job.ops[0].moves;
        // Inner-first: the hole is cut first.
        let (entry, start) = (moves[0].target(), moves[1].target());
        assert!((entry.distance(start) - 4.0).abs() < 1e-9, "{entry:?} -> {start:?}");
        assert!(entry.x > 30.0 && entry.x < 40.0 && entry.y > 30.0 && entry.y < 40.0, "the hole's lead-in is inside the hole: {entry:?}");
        assert!(matches!(moves[0], Move::Travel(_)) && matches!(moves[1], Move::Cut { power, .. } if power > 0.0), "the approach is a powered cut");
        // The outer outline's lead-in starts outside the 60 mm square.
        let outer = moves.iter().enumerate().skip(2).find(|(_, m)| matches!(m, Move::Travel(_))).unwrap().0;
        let entry = moves[outer].target();
        assert!(entry.x < 10.0 || entry.x > 70.0 || entry.y < 10.0 || entry.y > 70.0, "{entry:?}");
        // The lead-out leaves the outline again after it has closed.
        let last = moves.last().unwrap().target();
        let first_cut = moves[outer + 1].target();
        assert!((last.distance(first_cut) - 2.0).abs() < 1e-9, "{last:?} vs {first_cut:?}");
    }

    #[test]
    fn leads_are_shortened_to_stay_on_the_bed_for_absolute_placement_only() {
        let mut doc = square_doc(20.0, 0.0); // flush against the bed's top-left corner
        doc.layer_mut(0).unwrap().lead_in = 5.0;
        for m in &plan(&doc).unwrap().ops[0].moves {
            let p = m.target();
            assert!(p.x >= 0.0 && p.y >= 0.0, "{p:?} is off the bed");
        }
        doc.job_origin.start_from = lightfoundry_doc::StartFrom::CurrentPosition;
        let job = plan(&doc).unwrap();
        assert!(job.ops[0].moves.iter().any(|m| m.target().y < 0.0 || m.target().x < 0.0), "relative placement keeps the full lead");
    }

    #[test]
    fn open_paths_and_fill_are_untouched_and_bad_values_are_rejected() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("line", 0, lightfoundry_geom::line_path(Point::new(10.0, 10.0), Point::new(50.0, 10.0))));
        let layer = doc.layer_mut(0).unwrap();
        layer.lead_in = 3.0;
        layer.tab_count = 3;
        let job = plan(&doc).unwrap();
        assert_eq!(job.ops[0].moves.len(), 2, "travel + one cut, no lead, no tabs");
        for field in 0..3 {
            let mut bad = doc.clone();
            let layer = bad.layer_mut(0).unwrap();
            match field {
                0 => layer.tab_width = f64::NAN,
                1 => layer.lead_in = -1.0,
                _ => layer.lead_out = 1e9,
            }
            assert!(matches!(plan(&bad), Err(PlanError::BadLayerSetting { .. })), "field {field}");
        }
    }
}

mod fill_tests {
    use super::*;

    #[test]
    fn fill_covers_rectangle_with_expected_row_count() {
        let mut doc = rect_doc(10.0, 10.0, LayerMode::Fill);
        if let Some(l) = doc.layer_mut(0) {
            l.interval = 0.5;
        }
        let job = plan(&doc).unwrap();
        assert_eq!(job.ops[0].kind, OpKind::Fill);
        let cuts = job.ops[0].moves.iter().filter(|m| matches!(m, Move::Cut { .. })).count();
        assert_eq!(cuts, 20);
    }

    #[test]
    fn an_ellipse_fills_with_rows_shorter_near_its_poles() {
        let mut doc = Document::default();
        if let Some(l) = doc.layer_mut(0) {
            l.mode = LayerMode::Fill;
            l.interval = 1.0;
        }
        doc.add_shape(Shape::path("e", 0, ellipse_path(50.0, 50.0, 20.0, 10.0)));
        let job = plan(&doc).unwrap();
        let spans: Vec<f64> = job.ops[0].moves.chunks(2).map(|c| c[0].target().distance(c[1].target())).collect();
        assert_eq!(spans.len(), 20, "one row per mm over a 20 mm tall ellipse");
        assert!(spans[10] > 39.0 && spans[0] < 15.0, "wide in the middle, narrow at the pole: {spans:?}");
    }

    #[test]
    fn bidirectional_fill_alternates_direction() {
        let segs = fill_segments(&lightfoundry_geom::flatten(&rect_path(0.0, 0.0, 10.0, 10.0), 0.1), 1.0, 0.0, true);
        assert!(segs[0].0.x < segs[0].1.x);
        assert!(segs[1].0.x > segs[1].1.x);
    }

    #[test]
    fn holes_are_not_filled() {
        let mut lines = lightfoundry_geom::flatten(&rect_path(0.0, 0.0, 10.0, 10.0), 0.1);
        lines.extend(lightfoundry_geom::flatten(&rect_path(4.0, 0.0, 2.0, 10.0), 0.1));
        let segs = fill_segments(&lines, 1.0, 0.0, false);
        assert_eq!(segs.len(), 20);
    }

    #[test]
    fn hostile_interval_does_not_explode() {
        let lines = lightfoundry_geom::flatten(&rect_path(0.0, 0.0, 1e9, 1e9), 0.1);
        assert!(fill_segments(&lines, 1e-9, 0.0, false).is_empty());
        assert!(fill_segments(&lines, f64::NAN, 0.0, false).is_empty());
    }
}

mod image_tests {
    use super::*;

    fn image_doc(gray: Vec<u8>, w: u32, h: u32) -> Document {
        let mut doc = Document::default();
        if let Some(l) = doc.layer_mut(0) {
            l.interval = 1.0;
            l.min_power = 0.0;
            l.max_power = 100.0;
        }
        let image = Arc::new(RasterImage::new(w, h, gray).unwrap());
        doc.add_shape(Shape {
            id: 0,
            name: "img".into(),
            layer: 0,
            transform: Affine::IDENTITY,
            kind: ShapeKind::Image { image, width_mm: w as f64, height_mm: h as f64, adjust: Default::default() },
            cut: None,
            group: None,
        });
        doc
    }

    #[test]
    fn white_image_burns_nothing() {
        let job = plan(&image_doc(vec![255; 16], 4, 4)).unwrap();
        assert!(job.ops.is_empty());
    }

    #[test]
    fn black_image_burns_full_power_with_overscan() {
        let job = plan(&image_doc(vec![0; 16], 4, 4)).unwrap();
        let moves = &job.ops[0].moves;
        let full: Vec<_> = moves.iter().filter(|m| matches!(m, Move::Cut { power, .. } if *power == 100.0)).collect();
        assert_eq!(full.len(), 4);
        // Overscan lead-in and lead-out rows exist at zero power.
        assert!(moves.iter().any(|m| matches!(m, Move::Cut { power, .. } if *power == 0.0)));
    }

    #[test]
    fn a_picture_flush_with_the_bed_edge_gets_a_shorter_lead_in_instead_of_leaving_the_bed() {
        let mut doc = image_doc(vec![0; 16], 4, 4);
        doc.machine.bed_width = 4.0;
        let job = plan(&doc).unwrap();
        for m in &job.ops[0].moves {
            let x = m.target().x;
            assert!((0.0..=4.0).contains(&x), "{x} is outside the bed");
        }
        // Relative placement cannot know the room, so the full lead-in is kept.
        doc.job_origin.start_from = lightfoundry_doc::StartFrom::UserOrigin;
        let job = plan(&doc).unwrap();
        assert!(job.ops[0].moves.iter().any(|m| m.target().x < 0.0), "lead-in extends past the picture");
    }

    #[test]
    fn a_negative_burns_the_picture_but_never_the_white_around_a_rotated_one() {
        let mut doc = image_doc(vec![255; 16], 4, 4); // an all-white 4x4 mm picture
        if let Some(ShapeKind::Image { adjust, .. }) = doc.shapes.get_mut(0).map(|s| &mut s.kind) {
            adjust.negative = true;
        }
        doc.shapes[0].transform = Affine::translate((50.0, 50.0)) * Affine::rotate(0.6) * Affine::IDENTITY;
        let job = plan(&doc).unwrap();
        let burned: Vec<_> = job.ops[0].moves.iter().filter(|m| matches!(m, Move::Cut { power, .. } if *power > 0.0)).collect();
        assert!(!burned.is_empty(), "the white picture, negated, is black");
        let shape = &doc.shapes[0];
        let inside = shape.world_polylines().into_iter().next().unwrap();
        // Every burning move ends within a pixel of the picture: nothing outside it burns.
        for m in burned {
            let p = m.target();
            assert!(inside.contains(p) || inside.distance_to(p) <= 1.5, "{p:?} burns outside the picture");
        }
        // And the un-negated white picture burns nothing at all.
        if let Some(ShapeKind::Image { adjust, .. }) = doc.shapes.get_mut(0).map(|s| &mut s.kind) {
            adjust.negative = false;
        }
        assert!(plan(&doc).unwrap().ops.is_empty());
    }

    #[test]
    fn brightness_lightens_a_gray_image_until_it_stops_burning() {
        let mut doc = image_doc(vec![100; 16], 4, 4);
        if let Some(l) = doc.layer_mut(0) {
            l.dither = lightfoundry_doc::Dither::Grayscale;
        }
        let power =
            |doc: &Document| -> f64 {
                plan(doc).unwrap().ops.first().map_or(0.0, |o| {
                    o.moves.iter().filter_map(|m| if let Move::Cut { power, .. } = m { Some(*power) } else { None }).fold(0.0, f64::max)
                })
            };
        let base = power(&doc);
        assert!(base > 50.0);
        if let Some(ShapeKind::Image { adjust, .. }) = doc.shapes.get_mut(0).map(|s| &mut s.kind) {
            adjust.brightness = 0.3;
        }
        assert!(power(&doc) < base - 20.0, "brighter picture burns less");
        if let Some(ShapeKind::Image { adjust, .. }) = doc.shapes.get_mut(0).map(|s| &mut s.kind) {
            adjust.brightness = 1.0;
        }
        assert_eq!(power(&doc), 0.0);
    }

    #[test]
    fn oversized_raster_is_refused() {
        let mut doc = image_doc(vec![0; 4], 2, 2);
        if let Some(l) = doc.layer_mut(0) {
            l.interval = 0.0001;
        }
        if let Some(s) = doc.shapes.first_mut() {
            s.transform = Affine::scale(1000.0);
        }
        assert!(matches!(plan(&doc), Err(PlanError::RasterTooLarge(_))));
    }

    #[test]
    fn floyd_steinberg_preserves_mean_darkness() {
        let gray = vec![128.0f32; 100 * 100];
        let out = dither(&gray, 100, 100, lightfoundry_doc::Dither::FloydSteinberg);
        let mean: f32 = out.iter().sum::<f32>() / out.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
    }

    #[test]
    fn every_dither_mode_keeps_the_overall_tone_and_the_extremes() {
        use lightfoundry_doc::Dither;
        for mode in Dither::ALL {
            if mode == Dither::Threshold {
                continue; // a threshold has no tone to keep, only the extremes below
            }
            for (level, expected) in [(64.0f32, 0.75f32), (128.0, 0.5), (192.0, 0.25)] {
                let out = dither(&vec![level; 120 * 120], 120, 120, mode);
                let mean: f32 = out.iter().sum::<f32>() / out.len() as f32;
                let tolerance = if mode == Dither::Atkinson { 0.25 } else { 0.06 }; // Atkinson drops a quarter of the error on purpose
                assert!((mean - expected).abs() < tolerance, "{mode:?} at {level}: {mean}");
            }
            let white = dither(&vec![255.0; 40 * 40], 40, 40, mode);
            let black = dither(&vec![0.0; 40 * 40], 40, 40, mode);
            assert!(white.iter().all(|v| *v == 0.0), "{mode:?}: white burns nothing");
            assert!(black.iter().all(|v| *v == 1.0), "{mode:?}: black burns everything");
        }
    }

    #[test]
    fn dithers_survive_odd_sizes_and_mismatched_buffers() {
        use lightfoundry_doc::Dither;
        for mode in Dither::ALL {
            assert!(dither(&[], 0, 0, mode).is_empty());
            assert_eq!(dither(&[10.0], 1, 1, mode).len(), 1);
            assert_eq!(dither(&[10.0; 5], 3, 3, mode).len(), 5, "{mode:?}: a short buffer must not index out of range");
            assert_eq!(dither(&[99.0; 7 * 3], 7, 3, mode).len(), 21);
        }
    }

    #[test]
    fn the_bayer_ordered_pattern_is_regular_and_a_halftone_forms_dots() {
        use lightfoundry_doc::Dither;
        let g = vec![128.0f32; 16 * 16];
        let out = dither(&g, 16, 16, Dither::Ordered);
        assert_eq!(out[..8], out[8..16], "the 8-pixel tile repeats");
        let half = dither(&vec![128.0; 48 * 48], 48, 48, Dither::Halftone);
        let rows_with_both = (0..48).filter(|y| half[y * 48..(y + 1) * 48].contains(&1.0) && half[y * 48..(y + 1) * 48].contains(&0.0)).count();
        assert!(rows_with_both > 40, "dots, not a solid fill: {rows_with_both}");
    }
}

mod validation_tests {
    use super::*;

    #[test]
    fn bad_speed_is_an_error_not_a_panic() {
        let mut doc = rect_doc(5.0, 5.0, LayerMode::Line);
        for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            if let Some(l) = doc.layer_mut(0) {
                l.speed = bad;
            }
            assert!(matches!(plan(&doc), Err(PlanError::BadLayerSetting { .. })));
        }
    }

    #[test]
    fn estimate_scales_with_passes() {
        let mut doc = rect_doc(60.0, 0.0001, LayerMode::Line);
        if let Some(l) = doc.layer_mut(0) {
            l.speed = 600.0;
            l.passes = 1;
        }
        let one = estimate(&plan(&doc).unwrap(), 6000.0);
        if let Some(l) = doc.layer_mut(0) {
            l.passes = 3;
        }
        let three = estimate(&plan(&doc).unwrap(), 6000.0);
        assert!((three.seconds / one.seconds - 3.0).abs() < 0.2);
    }
}

mod timeline_tests {
    use super::*;

    #[test]
    fn totals_match_the_estimate_and_layers_add_up() {
        let job = plan(&two_layer_doc(2)).unwrap();
        let t = Timeline::new(&job, 6000.0);
        let e = estimate(&job, 6000.0);
        assert!((t.total - e.seconds).abs() < 1e-9 && (t.cut_mm() - e.cut_mm).abs() < 1e-9);
        let layers = t.layers();
        assert_eq!(layers.len(), 2);
        assert!((layers.iter().map(|l| l.seconds).sum::<f64>() - t.total).abs() < 1e-9);
        assert!(layers[1].seconds > layers[0].seconds, "the 30 mm square at the same speed takes longer than the 10 mm one");
    }

    #[test]
    fn at_finds_the_operation_and_pass() {
        let job = plan(&two_layer_doc(3)).unwrap();
        let t = Timeline::new(&job, 6000.0);
        let first = t.ops[0];
        let m = t.at(first.start + first.pass_seconds * 1.5).unwrap();
        assert_eq!((m.op, m.pass), (0, 1));
        assert!((m.into_pass - first.pass_seconds * 0.5).abs() < 1e-9);
        let second = t.ops[1];
        assert_eq!(t.at(second.start + 0.001).unwrap().op, 1);
        let end = t.at(t.total).unwrap();
        assert_eq!((end.op, end.pass), (1, 2), "the end is the last pass of the last operation");
        assert_eq!(t.at(0.0).unwrap().op, 0);
    }

    #[test]
    fn hostile_times_and_empty_jobs_are_handled() {
        let t = Timeline::new(&plan(&two_layer_doc(1)).unwrap(), 6000.0);
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -5.0, 1e300] {
            assert!(t.at(x).is_some(), "{x}");
        }
        assert_eq!(Timeline::new(&Job::default(), 6000.0).at(1.0), None);
        let bad = Timeline::new(&plan(&two_layer_doc(1)).unwrap(), f64::NAN);
        assert!(bad.total.is_finite() && bad.total > 0.0, "a bad rapid speed falls back to a default");
    }

    fn two_layer_doc(passes: u32) -> Document {
        let mut doc = rect_doc(10.0, 10.0, LayerMode::Line);
        let second = 1;
        for id in [0, second] {
            if let Some(l) = doc.layer_mut(id) {
                l.mode = LayerMode::Line;
                l.speed = 600.0;
                l.passes = passes;
            }
        }
        doc.add_shape(Shape::path("big", second, rect_path(50.0, 10.0, 30.0, 30.0)));
        doc
    }
}

mod order_option_tests {
    use lightfoundry_doc::Optimize;

    use super::*;

    #[test]
    fn identical_outlines_are_cut_once_whatever_their_direction_or_start() {
        let sq = |pts: [(f64, f64); 4]| Polyline::new(pts.iter().map(|&(x, y)| Point::new(x, y)).collect(), true);
        let a = sq([(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let reversed = sq([(0.0, 10.0), (10.0, 10.0), (10.0, 0.0), (0.0, 0.0)]);
        let rotated = sq([(10.0, 10.0), (0.0, 10.0), (0.0, 0.0), (10.0, 0.0)]);
        let other = sq([(0.0, 0.0), (11.0, 0.0), (11.0, 10.0), (0.0, 10.0)]);
        let kept = remove_duplicates(vec![a.clone(), reversed, rotated, other.clone()]);
        assert_eq!(kept, vec![a, other]);

        let open = Polyline::new(vec![Point::new(0.0, 0.0), Point::new(5.0, 5.0), Point::new(9.0, 1.0)], false);
        let open_back = Polyline::new(open.points.iter().rev().copied().collect(), false);
        assert_eq!(remove_duplicates(vec![open.clone(), open_back]).len(), 1);
        let closed_same_points = Polyline::new(open.points.clone(), true);
        assert_eq!(remove_duplicates(vec![open, closed_same_points]).len(), 2, "open and closed are different cuts");
    }

    #[test]
    fn hostile_coordinates_never_panic_or_vanish() {
        let bad = Polyline::new(vec![Point::new(f64::NAN, 0.0), Point::new(1.0, f64::INFINITY)], false);
        let huge = Polyline::new(vec![Point::new(1e300, 0.0), Point::new(0.0, -1e300)], false);
        assert_eq!(remove_duplicates(vec![bad.clone(), bad, huge.clone(), huge]).len(), 4, "lines it cannot compare are left alone");
        assert!(remove_duplicates(Vec::new()).is_empty());
        assert_eq!(remove_duplicates(vec![Polyline::new(Vec::new(), true), Polyline::new(Vec::new(), true)]).len(), 1);
    }

    #[test]
    fn planning_drops_a_pasted_copy_unless_the_option_is_off() {
        let mut doc = rect_doc(20.0, 20.0, LayerMode::Line);
        doc.add_shape(Shape::path("copy", 0, rect_path(10.0, 10.0, 20.0, 20.0)));
        let cut_moves = |doc: &Document| plan(doc).unwrap().ops.iter().flat_map(|o| &o.moves).filter(|m| matches!(m, Move::Cut { .. })).count();
        let once = cut_moves(&doc);
        doc.optimize.remove_duplicates = false;
        assert_eq!(cut_moves(&doc), once * 2);
    }

    #[test]
    fn a_duplicate_pair_does_not_cancel_itself_out_of_a_fill() {
        let mut doc = rect_doc(20.0, 20.0, LayerMode::Fill);
        doc.add_shape(Shape::path("copy", 0, rect_path(10.0, 10.0, 20.0, 20.0)));
        let moves = plan(&doc).unwrap().ops.iter().map(|o| o.moves.len()).sum::<usize>();
        assert!(moves > 0, "the square is still engraved");
    }

    #[test]
    fn ordering_options_choose_between_depth_drawing_order_and_nearest() {
        let rect = |x: f64, y: f64, w: f64| {
            Polyline::new(vec![Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + w), Point::new(x, y + w)], true)
        };
        let outer = rect(0.0, 0.0, 100.0);
        let inner = rect(40.0, 40.0, 10.0);
        let far = rect(300.0, 0.0, 10.0);
        let lines = vec![outer.clone(), far.clone(), inner.clone()];
        let start = Point::new(0.0, 0.0);
        let order = |inner_first, reduce_travel| {
            let opts = Optimize { inner_first, reduce_travel, remove_duplicates: false };
            order_polylines(lines.clone(), start, &opts).iter().map(|l| l.bounds().map_or(0.0, |b| b.x1 - b.x0).round() as usize).collect::<Vec<_>>()
        };
        let sizes = |v: Vec<usize>| v;
        // Default: the hole-like inner square first, then the rest by distance.
        assert_eq!(sizes(order(true, true))[0], 10);
        // Neither option: exactly the drawing order.
        assert_eq!(sizes(order(false, false)), vec![100, 10, 10]);
        // Depth only: inner first, the rest keep their drawing order.
        assert_eq!(sizes(order(true, false)), vec![10, 100, 10]);
        // Nearest only: starting at the origin the outer square is nearest.
        assert_eq!(sizes(order(false, true))[0], 100);
    }
}
