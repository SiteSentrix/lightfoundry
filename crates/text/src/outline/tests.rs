use kurbo::Shape;

use super::*;
use crate::outline::testfont::{tiny_font, tiny_font_with_kerning};

fn style(height: f64) -> TextStyle {
    TextStyle { height, ..TextStyle::default() }
}

mod layout_tests {
    use super::*;

    #[test]
    fn glyphs_are_scaled_so_capitals_have_the_requested_height_and_y_points_down() {
        let l = layout_outline(&tiny_font(), "A", &style(14.0)).unwrap();
        let b = l.path.bounding_box();
        // 700 units are 14 mm, so 1 unit = 0.02 mm: the 600-unit-wide glyph is 12 mm wide.
        assert!((b.width() - 12.0).abs() < 1e-3 && (b.height() - 14.0).abs() < 1e-3, "{b:?}");
        assert!((b.y0).abs() < 1e-3, "the top of the capital is at y = 0: {b:?}");
        assert!(l.unsupported.is_empty());
    }

    #[test]
    fn the_hole_in_a_glyph_is_a_second_closed_contour() {
        let l = layout_outline(&tiny_font(), "A", &style(7.0)).unwrap();
        let contours = lightfoundry_geom::flatten(&l.path, 0.01);
        assert_eq!(contours.len(), 2);
        assert!(contours.iter().all(|c| c.closed));
    }

    #[test]
    fn advances_spacing_lines_and_alignment_follow_the_font() {
        let f = tiny_font();
        let one = layout_outline(&f, "A", &style(7.0)).unwrap().path.bounding_box();
        let two = layout_outline(&f, "AB", &style(7.0)).unwrap().path.bounding_box();
        // At 0.01 mm per unit: B starts after A's 700-unit advance, so the pair spans 700 + 400.
        assert!((two.width() - 11.0).abs() < 1e-3, "{two:?}");
        assert!((one.width() - 6.0).abs() < 1e-3);
        let wide = layout_outline(&f, "AB", &TextStyle { letter_spacing: 2.0, ..style(7.0) }).unwrap().path.bounding_box();
        assert!(wide.width() > two.width() + 6.0, "spacing scales the advance");

        let lines = layout_outline(&f, "A\nB", &style(7.0)).unwrap().path.bounding_box();
        assert!(lines.height() > 7.0 * 2.0, "the second line sits a full line below: {lines:?}");
        let right = layout_outline(&f, "AB\nA", &TextStyle { align: Align::Right, ..style(7.0) }).unwrap().path.bounding_box();
        let left = layout_outline(&f, "AB\nA", &style(7.0)).unwrap().path.bounding_box();
        assert!((right.width() - left.width()).abs() < 1e-6, "alignment does not change the overall width");
    }

    #[test]
    fn kerning_from_the_font_pulls_pairs_together() {
        let plain = layout_outline(&tiny_font(), "AB", &style(7.0)).unwrap().path.bounding_box();
        let kerned = layout_outline(&tiny_font_with_kerning(), "AB", &style(7.0)).unwrap().path.bounding_box();
        // 100 font units at 0.01 mm per unit.
        assert!((plain.width() - kerned.width() - 1.0).abs() < 1e-3, "{} vs {}", plain.width(), kerned.width());
        let reversed = layout_outline(&tiny_font_with_kerning(), "BA", &style(7.0)).unwrap().path.bounding_box();
        let unkerned_reversed = layout_outline(&tiny_font(), "BA", &style(7.0)).unwrap().path.bounding_box();
        assert!((reversed.width() - unkerned_reversed.width()).abs() < 1e-3, "only the A-B pair is kerned");
    }

    #[test]
    fn missing_characters_are_reported_and_drawn_as_the_fonts_notdef() {
        let l = layout_outline(&tiny_font(), "AZ", &style(7.0)).unwrap();
        assert_eq!(l.unsupported, vec!['Z']);
        let spaces = layout_outline(&tiny_font(), "A A", &style(7.0)).unwrap();
        assert!(spaces.unsupported.is_empty(), "a space is not a missing glyph");
    }
}

mod hostile_input_tests {
    use super::*;

    #[test]
    fn broken_fonts_and_settings_are_errors_not_panics() {
        let good = tiny_font();
        assert!(matches!(layout_outline(&[], "A", &style(7.0)), Err(TextError::BadFont(_))));
        assert!(matches!(layout_outline(b"not a font at all", "A", &style(7.0)), Err(TextError::BadFont(_))));
        for cut in [4, 12, 40, good.len() / 2, good.len() - 1] {
            let _ = layout_outline(&good[..cut], "AB", &style(7.0));
        }
        let mut seed = 1u64;
        for _ in 0..300 {
            let mut f = good.clone();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = (seed % f.len() as u64) as usize;
                f[i] = (seed >> 20) as u8;
            }
            let _ = layout_outline(&f, "AB\nA", &style(7.0));
        }
        assert_eq!(layout_outline(&good, "A", &style(0.0)), Err(TextError::BadHeight));
        assert_eq!(layout_outline(&good, "A", &style(f64::NAN)), Err(TextError::BadHeight));
        assert_eq!(layout_outline(&good, "A", &TextStyle { letter_spacing: -1.0, ..style(7.0) }), Err(TextError::BadSpacing));
        assert_eq!(layout_outline(&good, &"A".repeat(MAX_CHARS + 1), &style(7.0)), Err(TextError::TooLong));
        assert!(layout_outline(&good, "", &style(7.0)).unwrap().path.elements().is_empty());
    }
}
