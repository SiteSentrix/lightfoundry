//! Snapping and ruler maths, kept free of egui so it can be tested exhaustively.
//!
//! Everything is in millimetres on the workspace (y down).

use lightfoundry_geom::{Point, Rect};

/// How close (in screen points) something must be to attract the cursor.
pub const SNAP_RADIUS_PX: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapSettings {
    /// Snap to this grid pitch (mm) when no object is closer; `None` turns grid snapping off.
    pub grid: Option<f64>,
    /// Snap to the edges, centres and mid-lines of other shapes, the bed, and guides.
    pub objects: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self { grid: None, objects: true }
    }
}

/// Coordinates the cursor can lock onto, per axis.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapTargets {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

impl SnapTargets {
    /// Adds a rectangle's edges and centre lines.
    pub fn add_rect(&mut self, r: Rect) {
        self.xs.extend([r.x0, (r.x0 + r.x1) / 2.0, r.x1]);
        self.ys.extend([r.y0, (r.y0 + r.y1) / 2.0, r.y1]);
    }
}

/// The result of snapping one point: where it ended up and which target lines it locked onto
/// (for drawing the alignment guides).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    pub point: Point,
    pub x_line: Option<f64>,
    pub y_line: Option<f64>,
}

/// Snaps a single point. `threshold` is in mm.
pub fn snap_point(p: Point, settings: &SnapSettings, targets: &SnapTargets, threshold: f64) -> Snapped {
    let (x, x_line) = snap_axis(p.x, settings, &targets.xs, threshold);
    let (y, y_line) = snap_axis(p.y, settings, &targets.ys, threshold);
    Snapped { point: Point::new(x, y), x_line, y_line }
}

/// Snaps a translation of a group of coordinates (the moving selection's edges and centre): the
/// closest key-to-target alignment within `threshold` wins on each axis, else the grid snaps the
/// first key. Returns the adjusted `(dx, dy)` and the target lines locked onto.
pub fn snap_translation(
    delta: (f64, f64),
    keys: &SnapTargets,
    settings: &SnapSettings,
    targets: &SnapTargets,
    threshold: f64,
) -> ((f64, f64), Option<f64>, Option<f64>) {
    let (dx, x_line) = snap_delta_axis(delta.0, &keys.xs, settings, &targets.xs, threshold);
    let (dy, y_line) = snap_delta_axis(delta.1, &keys.ys, settings, &targets.ys, threshold);
    ((dx, dy), x_line, y_line)
}

fn snap_axis(v: f64, settings: &SnapSettings, targets: &[f64], threshold: f64) -> (f64, Option<f64>) {
    if !v.is_finite() {
        return (v, None);
    }
    if settings.objects
        && let Some(t) = nearest(v, targets, threshold)
    {
        return (t, Some(t));
    }
    match settings.grid {
        Some(step) if step.is_finite() && step > 0.0 => ((v / step).round() * step, None),
        _ => (v, None),
    }
}

fn snap_delta_axis(delta: f64, keys: &[f64], settings: &SnapSettings, targets: &[f64], threshold: f64) -> (f64, Option<f64>) {
    if !delta.is_finite() {
        return (delta, None);
    }
    if settings.objects {
        let mut best: Option<(f64, f64)> = None; // (adjustment, target line)
        for k in keys {
            if let Some(t) = nearest(k + delta, targets, threshold) {
                let adjust = t - (k + delta);
                if best.is_none_or(|(a, _)| adjust.abs() < a.abs()) {
                    best = Some((adjust, t));
                }
            }
        }
        if let Some((adjust, line)) = best {
            return (delta + adjust, Some(line));
        }
    }
    match (settings.grid, keys.first()) {
        (Some(step), Some(k0)) if step.is_finite() && step > 0.0 => (((k0 + delta) / step).round() * step - k0, None),
        _ => (delta, None),
    }
}

fn nearest(v: f64, targets: &[f64], threshold: f64) -> Option<f64> {
    targets.iter().copied().filter(|t| t.is_finite() && (t - v).abs() <= threshold).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
}

/// Shift while drawing a box or ellipse: the shorter side grows to match the longer, keeping the
/// direction the cursor moved in.
pub fn constrain_square(from: Point, to: Point) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let side = dx.abs().max(dy.abs());
    Point::new(from.x + side.copysign(if dx == 0.0 { 1.0 } else { dx }), from.y + side.copysign(if dy == 0.0 { 1.0 } else { dy }))
}

/// Shift while drawing a line: the end point is rotated about the start to the nearest multiple
/// of `step_deg`, keeping its length.
pub fn constrain_angle(from: Point, to: Point, step_deg: f64) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let len = dx.hypot(dy);
    if len == 0.0 || !(step_deg.is_finite() && step_deg > 0.0) {
        return to;
    }
    let step = step_deg.to_radians();
    let a = (dy.atan2(dx) / step).round() * step;
    Point::new(from.x + len * a.cos(), from.y + len * a.sin())
}

/// Ruler tick spacing in mm for a zoom of `px_per_mm`: `(major, minor)`. Majors are at least
/// ~60 px apart and always a 1-2-5 step; minors divide a major by five or ten.
pub fn ruler_steps(px_per_mm: f64) -> (f64, f64) {
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return (100.0, 10.0);
    }
    let want = 60.0 / px_per_mm;
    let mut magnitude = 10f64.powf(want.log10().floor());
    // 1-2-5 sequence at this magnitude, then the next decade.
    let mut major = loop {
        let Some(m) = [1.0, 2.0, 5.0, 10.0].iter().map(|f| f * magnitude).find(|m| *m >= want) else {
            magnitude *= 10.0;
            continue;
        };
        break m;
    };
    major = major.clamp(1e-3, 1e6);
    let minor = if (major.log10() - major.log10().round()).abs() < 1e-9
        || (major / 5.0 - (major / 5.0).round()).abs() < 1e-9 && major.log10().fract().abs() > 0.1
    {
        major / 10.0
    } else {
        major / 5.0
    };
    (major, if minor * px_per_mm >= 5.0 { minor } else { major })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> SnapTargets {
        let mut t = SnapTargets::default();
        t.add_rect(Rect::new(100.0, 50.0, 140.0, 90.0));
        t
    }

    mod point_tests {
        use super::*;

        #[test]
        fn nearby_edges_and_centres_attract_and_report_their_line() {
            let s = snap_point(Point::new(101.2, 69.0), &SnapSettings::default(), &targets(), 2.0);
            assert_eq!(s.point, Point::new(100.0, 70.0), "left edge, and the centre line y=70");
            assert_eq!((s.x_line, s.y_line), (Some(100.0), Some(70.0)));
        }

        #[test]
        fn far_points_are_left_alone_and_axes_snap_independently() {
            let p = Point::new(124.5, 69.5);
            let s = snap_point(p, &SnapSettings::default(), &targets(), 2.0);
            assert_eq!(s.point, Point::new(124.5, 70.0), "x is over 4 mm from every line but y is on the centre line");
            assert_eq!(s.x_line, None);
        }

        #[test]
        fn the_grid_applies_when_no_object_is_close_and_objects_win_when_one_is() {
            let on = SnapSettings { grid: Some(5.0), objects: true };
            assert_eq!(snap_point(Point::new(12.4, 17.6), &on, &targets(), 2.0).point, Point::new(10.0, 20.0));
            assert_eq!(snap_point(Point::new(101.0, 17.6), &on, &targets(), 2.0).point, Point::new(100.0, 20.0), "object beats grid on x");
            let grid_only = SnapSettings { grid: Some(5.0), objects: false };
            assert_eq!(snap_point(Point::new(101.0, 70.4), &grid_only, &targets(), 2.0).point, Point::new(100.0, 70.0));
            let off = SnapSettings { grid: None, objects: false };
            assert_eq!(snap_point(Point::new(101.0, 70.4), &off, &targets(), 2.0).point, Point::new(101.0, 70.4));
        }

        #[test]
        fn hostile_values_pass_through_or_are_ignored() {
            let s = snap_point(Point::new(f64::NAN, f64::INFINITY), &SnapSettings { grid: Some(5.0), objects: true }, &targets(), 2.0);
            assert!(s.point.x.is_nan() && s.point.y.is_infinite());
            let bad_grid = SnapSettings { grid: Some(0.0), objects: false };
            assert_eq!(snap_point(Point::new(3.3, 4.4), &bad_grid, &targets(), 2.0).point, Point::new(3.3, 4.4));
            let nan_grid = SnapSettings { grid: Some(f64::NAN), objects: false };
            assert_eq!(snap_point(Point::new(3.3, 4.4), &nan_grid, &targets(), 2.0).point, Point::new(3.3, 4.4));
            let mut t = SnapTargets::default();
            t.xs.push(f64::NAN);
            assert_eq!(snap_point(Point::new(1.0, 1.0), &SnapSettings::default(), &t, 2.0).point, Point::new(1.0, 1.0));
        }
    }

    mod translation_tests {
        use super::*;

        fn keys(r: Rect) -> SnapTargets {
            let mut k = SnapTargets::default();
            k.add_rect(r);
            k
        }

        #[test]
        fn a_moving_selection_locks_its_nearest_edge_to_a_target() {
            // A 20x20 box at (60,50) dragged right by 18.5: its right edge (80+18.5=98.5) is 1.5 from x=100? No: its
            // left edge ends at 78.5 and right at 98.5; the right edge is within 2 of the target at x=100.
            let k = keys(Rect::new(60.0, 50.0, 80.0, 70.0));
            let ((dx, dy), xl, _) = snap_translation((18.5, 0.0), &k, &SnapSettings::default(), &targets(), 2.0);
            assert!((dx - 20.0).abs() < 1e-9 && dy == 0.0, "{dx}");
            assert_eq!(xl, Some(100.0));
        }

        #[test]
        fn the_closest_alignment_wins_among_several() {
            let k = keys(Rect::new(0.0, 0.0, 40.0, 40.0)); // same size as the target: edges can align two ways
            let ((dx, _), _, _) = snap_translation((100.7, 0.0), &k, &SnapSettings::default(), &targets(), 2.0);
            assert!((dx - 100.0).abs() < 1e-9, "left edge to 100 (0.7 away) beats right edge to 140 (0.7 away on the other side, equal) — got {dx}");
        }

        #[test]
        fn grid_snapping_moves_the_selections_corner_onto_the_grid() {
            let k = keys(Rect::new(3.0, 3.0, 13.0, 13.0));
            let grid = SnapSettings { grid: Some(5.0), objects: false };
            let ((dx, dy), _, _) = snap_translation((9.0, 4.0), &k, &grid, &targets(), 2.0);
            assert!((3.0 + dx - 10.0).abs() < 1e-9 && (3.0 + dy - 5.0).abs() < 1e-9, "{dx} {dy}");
        }

        #[test]
        fn nothing_in_range_changes_nothing_and_nan_is_untouched() {
            let k = keys(Rect::new(0.0, 0.0, 5.0, 5.0));
            assert_eq!(snap_translation((13.0, 7.0), &k, &SnapSettings::default(), &targets(), 2.0).0, (13.0, 7.0));
            let ((dx, _), _, _) = snap_translation((f64::NAN, 0.0), &k, &SnapSettings::default(), &targets(), 2.0);
            assert!(dx.is_nan());
            assert_eq!(
                snap_translation((1.0, 1.0), &SnapTargets::default(), &SnapSettings { grid: Some(5.0), objects: true }, &targets(), 2.0).0,
                (1.0, 1.0),
                "no keys, nothing to snap"
            );
        }
    }

    mod constraint_tests {
        use super::*;

        #[test]
        fn square_constraint_keeps_direction_and_uses_the_longer_side() {
            assert_eq!(constrain_square(Point::new(10.0, 10.0), Point::new(40.0, 25.0)), Point::new(40.0, 40.0));
            assert_eq!(constrain_square(Point::new(10.0, 10.0), Point::new(-5.0, 12.0)), Point::new(-5.0, 25.0));
            assert_eq!(constrain_square(Point::new(10.0, 10.0), Point::new(10.0, 10.0)), Point::new(10.0, 10.0));
        }

        #[test]
        fn angle_constraint_snaps_to_multiples_and_keeps_length() {
            let end = constrain_angle(Point::new(0.0, 0.0), Point::new(10.0, 0.8), 15.0);
            assert!((end.x.hypot(end.y) - 10.0f64.hypot(0.8)).abs() < 1e-9);
            assert!(end.y.abs() < 1e-9, "4.6 degrees rounds to 0: {end:?}");
            let up = constrain_angle(Point::new(0.0, 0.0), Point::new(10.0, 1.5), 15.0);
            assert!((up.y.atan2(up.x).to_degrees() - 15.0).abs() < 1e-9, "8.5 degrees is nearer 15 than 0: {up:?}");
            let diag = constrain_angle(Point::new(0.0, 0.0), Point::new(9.0, 11.0), 45.0);
            assert!((diag.x - diag.y).abs() < 1e-9 && diag.x > 0.0);
            assert_eq!(constrain_angle(Point::new(1.0, 1.0), Point::new(1.0, 1.0), 15.0), Point::new(1.0, 1.0));
            assert_eq!(constrain_angle(Point::new(0.0, 0.0), Point::new(3.0, 4.0), 0.0), Point::new(3.0, 4.0));
        }
    }

    mod ruler_tests {
        use super::*;

        #[test]
        fn majors_are_one_two_five_steps_at_least_sixty_points_apart() {
            for zoom in [0.2, 0.5, 1.0, 2.0, 3.78, 10.0, 50.0, 400.0] {
                let (major, minor) = ruler_steps(zoom);
                assert!(major * zoom >= 59.9, "{zoom}: major {major}");
                let mantissa = major / 10f64.powf(major.log10().floor());
                assert!([1.0, 2.0, 5.0].iter().any(|m| (mantissa - m).abs() < 1e-6), "{zoom}: {major} is not 1-2-5");
                assert!(
                    minor <= major && (major / minor).fract().abs() < 1e-6 || (major / minor - (major / minor).round()).abs() < 1e-6,
                    "{zoom}: minor {minor} must divide major {major}"
                );
            }
        }

        #[test]
        fn hostile_zoom_has_a_sane_fallback() {
            for z in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                assert_eq!(ruler_steps(z), (100.0, 10.0));
            }
        }
    }
}
