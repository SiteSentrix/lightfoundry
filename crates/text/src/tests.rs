use kurbo::Shape;

use super::*;

fn bounds(text: &str, style: &TextStyle) -> kurbo::Rect {
    layout(text, style).unwrap().path.bounding_box()
}

mod layout_tests {
    use super::*;

    #[test]
    fn cap_height_matches_the_requested_height() {
        let b = bounds("H", &TextStyle { height: 12.0, ..TextStyle::default() });
        assert!((b.height() - 12.0).abs() < 1e-6, "{b:?}");
        assert!((b.y0).abs() < 1e-6, "top of the caps sits at y = 0");
        assert!((b.width() - 8.0).abs() < 1e-6, "H is 4 units wide at 2 mm per unit: {b:?}");
    }

    #[test]
    fn descenders_hang_below_the_baseline_and_ascenders_reach_the_cap_line() {
        let s = TextStyle { height: 6.0, ..TextStyle::default() };
        assert!(bounds("g", &s).y1 > 6.0 + 1.0);
        assert!((bounds("b", &s).y0).abs() < 1e-6);
        assert!(bounds("x", &s).y0 > 1.5, "x-height is below the cap line");
    }

    #[test]
    fn letter_spacing_widens_text() {
        let tight = bounds("AB", &TextStyle { letter_spacing: 0.0, ..TextStyle::default() }).width();
        let loose = bounds("AB", &TextStyle { letter_spacing: 3.0, ..TextStyle::default() }).width();
        assert!(loose > tight + 5.0, "{tight} -> {loose}");
    }

    #[test]
    fn lines_stack_and_alignment_shifts_short_lines() {
        let left = TextStyle { align: Align::Left, ..TextStyle::default() };
        let center = TextStyle { align: Align::Center, ..TextStyle::default() };
        let right = TextStyle { align: Align::Right, ..TextStyle::default() };
        let text = "WIDE TEXT\nI";
        assert!(bounds(text, &left).height() > 10.0 * 1.4, "two lines are taller than one");
        let small = |s: &TextStyle| {
            let l = layout(text, s).unwrap();
            // The second line starts at the lowest y values; find the bounds of strokes in the lower half.
            let mut min_x = f64::INFINITY;
            for el in l.path.elements() {
                if let kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) = el
                    && p.y > 12.0
                {
                    min_x = min_x.min(p.x);
                }
            }
            min_x
        };
        assert!(small(&center) > small(&left) + 5.0);
        assert!(small(&right) > small(&center) + 5.0);
    }

    #[test]
    fn empty_and_whitespace_text_make_no_geometry() {
        assert_eq!(layout("", &TextStyle::default()).unwrap().path.elements().len(), 0);
        assert_eq!(layout("   \n  ", &TextStyle::default()).unwrap().path.elements().len(), 0);
    }

    #[test]
    fn unsupported_characters_are_reported_and_drawn_as_boxes() {
        let l = layout("a€b€é", &TextStyle::default()).unwrap();
        assert_eq!(l.unsupported, vec!['€', 'é']);
        assert!(l.path.bounding_box().width() > 0.0);
    }
}

mod validation_tests {
    use super::*;

    #[test]
    fn hostile_styles_are_errors() {
        for h in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert_eq!(layout("A", &TextStyle { height: h, ..TextStyle::default() }), Err(TextError::BadHeight), "{h}");
        }
        for s in [-1.0, f64::NAN, 11.0] {
            assert_eq!(layout("A", &TextStyle { letter_spacing: s, ..TextStyle::default() }), Err(TextError::BadSpacing));
            assert_eq!(layout("A", &TextStyle { line_spacing: s, ..TextStyle::default() }), Err(TextError::BadSpacing));
        }
    }

    #[test]
    fn overlong_text_is_refused() {
        assert_eq!(layout(&"A".repeat(MAX_CHARS + 1), &TextStyle::default()), Err(TextError::TooLong));
        assert!(layout(&"A".repeat(MAX_CHARS), &TextStyle::default()).is_ok());
    }
}

mod glyph_table_tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_glyph() {
        let missing: Vec<char> = (0x20u8..0x7f).map(char::from).filter(|c| !glyphs::lookup(*c).1).collect();
        // Characters we deliberately do not draw yet.
        assert_eq!(missing, vec!['`', '{', '}', '~'], "update this list when adding glyphs");
    }

    #[test]
    fn glyph_geometry_is_sane() {
        for c in (0x21u8..0x7f).map(char::from) {
            let (g, known) = glyphs::lookup(c);
            if !known {
                continue;
            }
            assert!(!g.strokes.is_empty(), "{c:?} has no strokes");
            for s in &g.strokes {
                for p in &s.points {
                    assert!(p.x.is_finite() && p.y.is_finite());
                    assert!(p.x >= -0.3 && p.x <= g.width + 0.3, "{c:?}: x {} outside width {}", p.x, g.width);
                    assert!((-2.1..=6.9).contains(&p.y), "{c:?}: y {} outside the glyph box", p.y);
                }
            }
        }
    }

    #[test]
    fn smoothing_keeps_open_stroke_endpoints_and_closes_loops() {
        let (o, _) = glyphs::lookup('O');
        assert!(o.strokes[0].closed);
        let (c, _) = glyphs::lookup('C');
        let pts = &c.strokes[0].points;
        assert_eq!((pts[0].x, pts[0].y), (4.0, 5.0), "start preserved");
        let last = pts[pts.len() - 1];
        assert_eq!((last.x, last.y), (4.0, 1.0), "end preserved");
        assert!(pts.len() > 20, "smoothed into many short segments");
    }

    #[test]
    fn distinct_letters_make_distinct_shapes() {
        let s = TextStyle::default();
        let shapes: Vec<_> =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().map(|c| format!("{:?}", layout(&c.to_string(), &s).unwrap().path.elements())).collect();
        let unique: std::collections::HashSet<_> = shapes.iter().collect();
        assert_eq!(unique.len(), 26);
    }
}
