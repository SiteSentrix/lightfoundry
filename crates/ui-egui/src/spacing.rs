//! Equal-spacing hints for a shape being moved, free of egui so it can be tested exhaustively.
//!
//! Along one axis the moving box snaps when its gaps match: centred between two neighbours, or
//! repeating the gap between two shapes already in a row. Everything is in millimetres.

use lightfoundry_geom::{Point, Rect};

/// Most neighbours considered, so a drawing with thousands of shapes cannot slow a drag.
pub const MAX_NEIGHBOURS: usize = 5000;
/// Gaps smaller than this are touching, not spacing worth showing.
const MIN_GAP: f64 = 0.01;

/// One measured gap to draw: a line from `a` to `b` (both on the same axis line) of `size` mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    pub a: Point,
    pub b: Point,
    pub size: f64,
}

/// A spacing snap: how far to shift the moving box along the axis, and the equal gaps to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Spacing {
    pub adjust: f64,
    pub gaps: Vec<Gap>,
}

/// Finds the closest equal-spacing snap along x (`vertical_axis: false`) or along y (`true`)
/// within `threshold` mm of where `moving` is now. `others` are the shapes that stay put; only
/// those that overlap the moving box on the other axis count as being in its row.
pub fn equal_spacing(moving: Rect, others: &[Rect], threshold: f64, vertical_axis: bool) -> Option<Spacing> {
    let flip = |r: Rect| Rect::new(r.y0, r.x0, r.y1, r.x1);
    let flip_point = |p: Point| Point::new(p.y, p.x);
    if !vertical_axis {
        return along_x(moving, others, threshold);
    }
    let others: Vec<Rect> = others.iter().map(|r| flip(*r)).collect();
    let mut found = along_x(flip(moving), &others, threshold)?;
    for g in &mut found.gaps {
        (g.a, g.b) = (flip_point(g.a), flip_point(g.b));
    }
    Some(found)
}

fn along_x(m: Rect, others: &[Rect], threshold: f64) -> Option<Spacing> {
    let finite = |r: &Rect| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) && r.x1 >= r.x0 && r.y1 >= r.y0;
    if !finite(&m) || !threshold.is_finite() || threshold < 0.0 {
        return None;
    }
    let overlap = |r: &Rect| r.y0.min(r.y1).max(m.y0) < r.y1.min(m.y1) && finite(r);
    // The row: shapes entirely left of the moving box (nearest first) and entirely right of it.
    let mut left: Vec<Rect> = others.iter().take(MAX_NEIGHBOURS).filter(|r| overlap(r) && r.x1 <= m.x0).copied().collect();
    let mut right: Vec<Rect> = others.iter().take(MAX_NEIGHBOURS).filter(|r| overlap(r) && r.x0 >= m.x1).copied().collect();
    left.sort_by(|a, b| b.x1.total_cmp(&a.x1));
    right.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let width = m.x1 - m.x0;
    let mut best: Option<Spacing> = None;
    let mut consider = |adjust: f64, gaps: Vec<Gap>| {
        if adjust.abs() <= threshold && gaps.iter().all(|g| g.size >= MIN_GAP) && best.as_ref().is_none_or(|b| adjust.abs() < b.adjust.abs()) {
            best = Some(Spacing { adjust, gaps });
        }
    };

    if let (Some(l), Some(r)) = (left.first(), right.first()) {
        // Centred between the two nearest neighbours.
        let gap = (r.x0 - l.x1 - width) / 2.0;
        let x0 = l.x1 + gap;
        let at = Rect::new(x0, m.y0, x0 + width, m.y1);
        consider(x0 - m.x0, vec![gap_between(l, &at), gap_between(&at, r)]);
    }
    if let (Some(l), Some(ll)) = (left.first(), left.get(1)) {
        // Repeating the gap between the two nearest on the left, on their right.
        let g = l.x0 - ll.x1;
        let x0 = l.x1 + g;
        let at = Rect::new(x0, m.y0, x0 + width, m.y1);
        consider(x0 - m.x0, vec![gap_between(ll, l), gap_between(l, &at)]);
    }
    if let (Some(r), Some(rr)) = (right.first(), right.get(1)) {
        // ... and on the left of the two nearest on the right.
        let g = rr.x0 - r.x1;
        let x0 = r.x0 - g - width;
        let at = Rect::new(x0, m.y0, x0 + width, m.y1);
        consider(x0 - m.x0, vec![gap_between(&at, r), gap_between(r, rr)]);
    }
    best
}

/// The gap between `a` (left) and `b` (right), measured along the middle of where they overlap vertically.
fn gap_between(a: &Rect, b: &Rect) -> Gap {
    let (lo, hi) = (a.y0.max(b.y0), a.y1.min(b.y1));
    let y = if hi >= lo { (lo + hi) / 2.0 } else { (a.y0 + a.y1) / 2.0 };
    Gap { a: Point::new(a.x1, y), b: Point::new(b.x0, y), size: b.x0 - a.x1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect::new(x0, y0, x1, y1)
    }

    mod between_tests {
        use super::*;

        #[test]
        fn a_box_near_the_middle_of_two_neighbours_snaps_to_equal_gaps() {
            let others = [r(0.0, 0.0, 20.0, 20.0), r(100.0, 0.0, 120.0, 20.0)];
            // 20 wide: centred means it starts at 20 + (80 - 20) / 2 = 50.
            let s = equal_spacing(r(51.5, 0.0, 71.5, 20.0), &others, 3.0, false).unwrap();
            assert!((s.adjust + 1.5).abs() < 1e-9, "{s:?}");
            assert_eq!(s.gaps.len(), 2);
            assert!(s.gaps.iter().all(|g| (g.size - 30.0).abs() < 1e-9), "{:?}", s.gaps);
            assert_eq!(s.gaps[0].a, Point::new(20.0, 10.0));
            assert_eq!(s.gaps[1].b, Point::new(100.0, 10.0));
        }

        #[test]
        fn too_far_from_equal_does_nothing() {
            let others = [r(0.0, 0.0, 20.0, 20.0), r(100.0, 0.0, 120.0, 20.0)];
            assert!(equal_spacing(r(60.0, 0.0, 80.0, 20.0), &others, 3.0, false).is_none());
        }

        #[test]
        fn it_will_not_fit_between_neighbours_that_are_too_close() {
            let others = [r(0.0, 0.0, 20.0, 20.0), r(30.0, 0.0, 50.0, 20.0)];
            assert!(equal_spacing(r(22.0, 0.0, 42.0, 20.0), &others, 50.0, false).is_none(), "negative gaps are not spacing");
        }
    }

    mod pattern_tests {
        use super::*;

        #[test]
        fn a_box_repeats_the_gap_of_the_row_on_its_right() {
            let others = [r(0.0, 0.0, 20.0, 20.0), r(30.0, 0.0, 50.0, 20.0)]; // 10 mm gap
            let s = equal_spacing(r(61.0, 0.0, 81.0, 20.0), &others, 3.0, false).unwrap();
            assert!((s.adjust + 1.0).abs() < 1e-9, "{s:?}");
            assert!(s.gaps.iter().all(|g| (g.size - 10.0).abs() < 1e-9));
        }

        #[test]
        fn a_box_repeats_the_gap_of_the_row_on_its_left() {
            let others = [r(100.0, 0.0, 120.0, 20.0), r(130.0, 0.0, 150.0, 20.0)]; // 10 mm gap
            let s = equal_spacing(r(68.0, 0.0, 88.0, 20.0), &others, 3.0, false).unwrap();
            // Its right edge belongs at 100 - 10 = 90, so it starts at 70.
            assert!((s.adjust - 2.0).abs() < 1e-9, "{s:?}");
        }

        #[test]
        fn the_closest_of_several_snaps_wins() {
            // Centred between 0..20 and 60..80 it would start at 30, one mm away.
            let others = [r(0.0, 0.0, 20.0, 20.0), r(60.0, 0.0, 80.0, 20.0)];
            let s = equal_spacing(r(29.0, 0.0, 49.0, 20.0), &others, 3.0, false).unwrap();
            assert!((s.adjust - 1.0).abs() < 1e-9);
        }
    }

    mod axis_and_row_tests {
        use super::*;

        #[test]
        fn shapes_that_do_not_share_the_moving_boxs_row_are_ignored() {
            let others = [r(0.0, 100.0, 20.0, 120.0), r(100.0, 100.0, 120.0, 120.0)];
            assert!(equal_spacing(r(51.0, 0.0, 71.0, 20.0), &others, 3.0, false).is_none());
        }

        #[test]
        fn the_vertical_axis_mirrors_the_horizontal_one() {
            let others = [r(0.0, 0.0, 20.0, 20.0), r(0.0, 100.0, 20.0, 120.0)];
            let s = equal_spacing(r(0.0, 51.5, 20.0, 71.5), &others, 3.0, true).unwrap();
            assert!((s.adjust + 1.5).abs() < 1e-9, "{s:?}");
            assert_eq!(s.gaps[0].a, Point::new(10.0, 20.0));
            assert_eq!(s.gaps[0].b, Point::new(10.0, 50.0));
        }

        #[test]
        fn touching_shapes_and_missing_neighbours_give_nothing() {
            assert!(equal_spacing(r(0.0, 0.0, 10.0, 10.0), &[], 3.0, false).is_none());
            let others = [r(0.0, 0.0, 10.0, 10.0), r(10.0, 0.0, 20.0, 10.0)]; // zero gap
            assert!(equal_spacing(r(20.5, 0.0, 30.5, 10.0), &others, 3.0, false).is_none());
        }

        #[test]
        fn bad_numbers_and_huge_inputs_are_survived() {
            let nan = r(f64::NAN, 0.0, 1.0, 1.0);
            let good = [r(0.0, 0.0, 20.0, 20.0), r(100.0, 0.0, 120.0, 20.0)];
            assert!(equal_spacing(nan, &good, 3.0, false).is_none());
            assert!(equal_spacing(r(51.0, 0.0, 71.0, 20.0), &[nan, r(f64::INFINITY, 0.0, f64::INFINITY, 1.0)], 3.0, false).is_none());
            assert!(equal_spacing(r(51.0, 0.0, 71.0, 20.0), &good, f64::NAN, false).is_none());
            assert!(equal_spacing(r(51.0, 0.0, 71.0, 20.0), &good, -1.0, true).is_none());
            let many: Vec<Rect> = (0..20_000).map(|i| r(i as f64 * 3.0, 0.0, i as f64 * 3.0 + 1.0, 1.0)).collect();
            let _ = equal_spacing(r(5.0, 0.0, 6.0, 1.0), &many, 3.0, false);
        }
    }
}
