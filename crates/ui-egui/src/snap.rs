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

/// What a snap target is, which decides its marker and its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// A side of the bed.
    Bed,
    /// The middle of the bed.
    BedCenter,
    /// A guide pulled out of a ruler.
    Guide,
    /// A side of a shape's bounding box.
    Edge,
    /// The middle of a shape's bounding box.
    Center,
    /// The middle of a straight segment.
    Midpoint,
    /// A corner or segment end of a drawn path.
    Endpoint,
}

impl TargetKind {
    /// Wins when several targets are equally close: the more specific one.
    fn rank(self) -> u8 {
        match self {
            Self::Guide => 1,
            Self::Bed => 2,
            Self::Edge => 3,
            Self::Center | Self::BedCenter => 4,
            Self::Midpoint => 5,
            Self::Endpoint => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bed => "Bed edge",
            Self::BedCenter => "Bed center",
            Self::Guide => "Guide",
            Self::Edge => "Edge",
            Self::Center => "Center",
            Self::Midpoint => "Midpoint",
            Self::Endpoint => "Endpoint",
        }
    }
}

/// One coordinate the cursor can lock onto, with what it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The coordinate on the snapping axis, in mm.
    pub at: f64,
    pub kind: TargetKind,
    /// How far the owner extends along the other axis (`None`: unbounded, like a guide). A point's span is one value.
    pub span: Option<(f64, f64)>,
}

/// Coordinates the cursor can lock onto, per axis.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapTargets {
    pub xs: Vec<Target>,
    pub ys: Vec<Target>,
}

impl SnapTargets {
    /// Adds a shape's edges and centre lines.
    pub fn add_rect(&mut self, r: Rect) {
        self.add_rect_as(r, TargetKind::Edge, TargetKind::Center);
    }

    /// Adds the bed's sides and centre lines.
    pub fn add_bed(&mut self, r: Rect) {
        self.add_rect_as(r, TargetKind::Bed, TargetKind::BedCenter);
    }

    fn add_rect_as(&mut self, r: Rect, edge: TargetKind, center: TargetKind) {
        let (xspan, yspan) = (Some((r.y0, r.y1)), Some((r.x0, r.x1)));
        for (at, kind) in [(r.x0, edge), ((r.x0 + r.x1) / 2.0, center), (r.x1, edge)] {
            self.xs.push(Target { at, kind, span: xspan });
        }
        for (at, kind) in [(r.y0, edge), ((r.y0 + r.y1) / 2.0, center), (r.y1, edge)] {
            self.ys.push(Target { at, kind, span: yspan });
        }
    }

    /// Adds the vertical guide at `x`, or the horizontal one at `y`.
    pub fn add_guide(&mut self, vertical: bool, pos: f64) {
        let target = Target { at: pos, kind: TargetKind::Guide, span: None };
        if vertical { self.xs.push(target) } else { self.ys.push(target) }
    }

    /// Adds the lines through a point (a vertex or midpoint).
    pub fn add_point(&mut self, p: Point, kind: TargetKind) {
        self.xs.push(Target { at: p.x, kind, span: Some((p.y, p.y)) });
        self.ys.push(Target { at: p.y, kind, span: Some((p.x, p.x)) });
    }
}

/// Most vertex targets taken from one shape, and from the whole drawing.
pub const MAX_VERTEX_TARGETS_PER_SHAPE: usize = 200;
pub const MAX_VERTEX_TARGETS: usize = 5000;

/// The points of a path worth snapping to, in workspace mm: every segment end, plus the midpoint
/// of each straight segment. Stops after `cap` points.
pub fn path_points(path: &kurbo::BezPath, transform: kurbo::Affine, cap: usize) -> Vec<(Point, TargetKind)> {
    use kurbo::PathEl;
    let mut out = Vec::new();
    let mut at: Option<Point> = None;
    let mut start: Option<Point> = None;
    for el in path.elements() {
        if out.len() >= cap {
            break;
        }
        match *el {
            PathEl::MoveTo(p) => {
                out.push((transform * p, TargetKind::Endpoint));
                at = Some(p);
                start = Some(p);
            }
            PathEl::LineTo(p) => {
                if let Some(a) = at {
                    out.push((transform * a.midpoint(p), TargetKind::Midpoint));
                }
                out.push((transform * p, TargetKind::Endpoint));
                at = Some(p);
            }
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                out.push((transform * p, TargetKind::Endpoint));
                at = Some(p);
            }
            PathEl::ClosePath => {
                if let (Some(a), Some(s)) = (at, start) {
                    out.push((transform * a.midpoint(s), TargetKind::Midpoint));
                    at = Some(s);
                }
            }
        }
    }
    out.truncate(cap);
    out.retain(|(p, _)| p.x.is_finite() && p.y.is_finite());
    out
}

/// The result of snapping one point: where it ended up and which target lines it locked onto
/// (for drawing the alignment guides).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    pub point: Point,
    pub x_line: Option<Target>,
    pub y_line: Option<Target>,
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
) -> ((f64, f64), Option<Target>, Option<Target>) {
    let (dx, x_line) = snap_delta_axis(delta.0, &keys.xs, settings, &targets.xs, threshold);
    let (dy, y_line) = snap_delta_axis(delta.1, &keys.ys, settings, &targets.ys, threshold);
    ((dx, dy), x_line, y_line)
}

fn snap_axis(v: f64, settings: &SnapSettings, targets: &[Target], threshold: f64) -> (f64, Option<Target>) {
    if !v.is_finite() {
        return (v, None);
    }
    if settings.objects
        && let Some(t) = nearest(v, targets, threshold)
    {
        return (t.at, Some(t));
    }
    match settings.grid {
        Some(step) if step.is_finite() && step > 0.0 => ((v / step).round() * step, None),
        _ => (v, None),
    }
}

fn snap_delta_axis(delta: f64, keys: &[Target], settings: &SnapSettings, targets: &[Target], threshold: f64) -> (f64, Option<Target>) {
    if !delta.is_finite() {
        return (delta, None);
    }
    if settings.objects {
        let mut best: Option<(f64, Target)> = None; // (adjustment, target line)
        for k in keys {
            if let Some(t) = nearest(k.at + delta, targets, threshold) {
                let adjust = t.at - (k.at + delta);
                if best
                    .is_none_or(|(a, bt)| adjust.abs() < a.abs() - 1e-9 || (adjust.abs() - a.abs()).abs() <= 1e-9 && t.kind.rank() > bt.kind.rank())
                {
                    best = Some((adjust, t));
                }
            }
        }
        if let Some((adjust, line)) = best {
            return (delta + adjust, Some(line));
        }
    }
    match (settings.grid, keys.first()) {
        (Some(step), Some(k0)) if step.is_finite() && step > 0.0 => (((k0.at + delta) / step).round() * step - k0.at, None),
        _ => (delta, None),
    }
}

/// The closest target within `threshold`; of equally close ones, the most specific.
fn nearest(v: f64, targets: &[Target], threshold: f64) -> Option<Target> {
    targets.iter().copied().filter(|t| t.at.is_finite() && (t.at - v).abs() <= threshold).min_by(|a, b| {
        let (da, db) = ((a.at - v).abs(), (b.at - v).abs());
        if (da - db).abs() <= 1e-9 { b.kind.rank().cmp(&a.kind.rank()) } else { da.total_cmp(&db) }
    })
}

/// The name to show beside a snap: the most specific thing the cursor or selection locked onto.
pub fn label(x: Option<&Target>, y: Option<&Target>) -> Option<&'static str> {
    [x, y].into_iter().flatten().max_by_key(|t| t.kind.rank()).map(|t| t.kind.label())
}

/// The shape of a mark drawn where things line up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// An X on an edge or corner.
    Cross,
    /// A + at a centre.
    Plus,
    /// A square at an endpoint.
    Square,
    /// A triangle at a midpoint.
    Triangle,
}

/// What to draw for one snapped axis, in workspace mm.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// The alignment line's two ends.
    pub line: (Point, Point),
    pub marks: Vec<(Mark, Point)>,
}

/// Builds the alignment line and marks for a vertical line locked onto `hit` (pass `vertical:
/// false` for a horizontal one). `mover` is the moving selection's box, or a zero-size box at the
/// cursor; `extent` bounds lines whose owner has no span (guides). Both ends are ordered low to high.
pub fn alignment(vertical: bool, hit: &Target, mover: Rect, extent: Rect) -> Alignment {
    // Work in "line at u, running along v", then swap back for horizontal lines.
    let (mu, mv, ev) = if vertical {
        ((mover.x0, mover.x1), (mover.y0, mover.y1), (extent.y0, extent.y1))
    } else {
        ((mover.y0, mover.y1), (mover.x0, mover.x1), (extent.x0, extent.x1))
    };
    let u = hit.at;
    let span = hit.span.unwrap_or(ev);
    let (lo, hi) = (span.0.min(mv.0), span.1.max(mv.1));
    let at = |u: f64, v: f64| if vertical { Point::new(u, v) } else { Point::new(v, u) };
    let mut marks = Vec::new();
    let mid = |a: f64, b: f64| (a + b) / 2.0;
    match hit.kind {
        TargetKind::Center | TargetKind::BedCenter => marks.push((Mark::Plus, at(u, mid(span.0, span.1)))),
        TargetKind::Endpoint => marks.push((Mark::Square, at(u, span.0))),
        TargetKind::Midpoint => marks.push((Mark::Triangle, at(u, span.0))),
        TargetKind::Edge | TargetKind::Bed => {
            marks.push((Mark::Cross, at(u, span.0)));
            marks.push((Mark::Cross, at(u, span.1)));
        }
        TargetKind::Guide => {}
    }
    if mv.1 - mv.0 > 1e-9 {
        // A moving box: mark where its edge, or centre, meets the line.
        if (mid(mu.0, mu.1) - u).abs() <= 1e-6 {
            marks.push((Mark::Plus, at(u, mid(mv.0, mv.1))));
        } else {
            marks.push((Mark::Cross, at(u, mv.0)));
            marks.push((Mark::Cross, at(u, mv.1)));
        }
    }
    Alignment { line: (at(u, lo), at(u, hi)), marks }
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

    mod alignment_tests {
        use super::*;

        const EXTENT: Rect = Rect { x0: 0.0, y0: 0.0, x1: 400.0, y1: 300.0 };

        fn cursor(x: f64, y: f64) -> Rect {
            Rect::new(x, y, x, y)
        }

        #[test]
        fn rectangles_become_edges_and_a_centre_that_remember_their_extent() {
            let mut t = SnapTargets::default();
            t.add_rect(Rect::new(10.0, 20.0, 50.0, 40.0));
            let kinds: Vec<_> = t.xs.iter().map(|x| (x.at, x.kind)).collect();
            assert_eq!(kinds, [(10.0, TargetKind::Edge), (30.0, TargetKind::Center), (50.0, TargetKind::Edge)]);
            assert!(t.xs.iter().all(|x| x.span == Some((20.0, 40.0))), "x lines run along y");
            assert!(t.ys.iter().all(|y| y.span == Some((10.0, 50.0))), "y lines run along x");
            let mut bed = SnapTargets::default();
            bed.add_bed(Rect::new(0.0, 0.0, 400.0, 300.0));
            assert_eq!(bed.xs.iter().map(|x| x.kind).collect::<Vec<_>>(), [TargetKind::Bed, TargetKind::BedCenter, TargetKind::Bed]);
            bed.add_guide(true, 77.0);
            assert_eq!(bed.xs.last().map(|g| (g.kind, g.span)), Some((TargetKind::Guide, None)));
        }

        #[test]
        fn of_equally_close_targets_the_more_specific_one_wins() {
            let mut t = SnapTargets::default();
            t.add_rect(Rect::new(0.0, 0.0, 100.0, 100.0)); // edge at x = 100
            t.add_point(Point::new(100.0, 40.0), TargetKind::Endpoint);
            t.add_point(Point::new(100.0, 60.0), TargetKind::Midpoint);
            let s = snap_point(Point::new(99.5, 40.0), &SnapSettings::default(), &t, 2.0);
            assert_eq!(s.x_line.map(|x| x.kind), Some(TargetKind::Endpoint));
            // A nearer edge still beats a farther endpoint.
            let mut t = SnapTargets::default();
            t.add_rect(Rect::new(0.0, 0.0, 100.0, 100.0));
            t.add_point(Point::new(101.5, 40.0), TargetKind::Endpoint);
            let s = snap_point(Point::new(99.8, 40.0), &SnapSettings::default(), &t, 2.0);
            assert_eq!(s.x_line.map(|x| x.kind), Some(TargetKind::Edge));
        }

        #[test]
        fn the_label_names_the_most_specific_hit() {
            let hit = |kind| Some(Target { at: 0.0, kind, span: None });
            assert_eq!(label(hit(TargetKind::Edge).as_ref(), hit(TargetKind::Endpoint).as_ref()), Some("Endpoint"));
            assert_eq!(label(None, hit(TargetKind::BedCenter).as_ref()), Some("Bed center"));
            assert_eq!(label(hit(TargetKind::Guide).as_ref(), hit(TargetKind::Bed).as_ref()), Some("Bed edge"));
            assert_eq!(label(None, None), None);
        }

        #[test]
        fn a_cursor_on_an_edge_gets_a_line_back_to_that_shape_with_crosses_on_its_corners() {
            let hit = Target { at: 50.0, kind: TargetKind::Edge, span: Some((10.0, 30.0)) };
            let a = alignment(true, &hit, cursor(50.0, 60.0), EXTENT);
            assert_eq!(a.line, (Point::new(50.0, 10.0), Point::new(50.0, 60.0)));
            assert_eq!(a.marks, [(Mark::Cross, Point::new(50.0, 10.0)), (Mark::Cross, Point::new(50.0, 30.0))]);
        }

        #[test]
        fn centres_get_a_plus_and_vertices_a_square_or_triangle() {
            let c = Target { at: 50.0, kind: TargetKind::Center, span: Some((10.0, 30.0)) };
            assert_eq!(alignment(true, &c, cursor(50.0, 5.0), EXTENT).marks, [(Mark::Plus, Point::new(50.0, 20.0))]);
            let v = Target { at: 50.0, kind: TargetKind::Endpoint, span: Some((12.0, 12.0)) };
            assert_eq!(alignment(true, &v, cursor(50.0, 12.0), EXTENT).marks, [(Mark::Square, Point::new(50.0, 12.0))]);
            let m = Target { kind: TargetKind::Midpoint, ..v };
            assert_eq!(alignment(true, &m, cursor(50.0, 12.0), EXTENT).marks, [(Mark::Triangle, Point::new(50.0, 12.0))]);
        }

        #[test]
        fn a_moving_box_is_marked_where_its_edge_or_centre_meets_the_line() {
            let hit = Target { at: 50.0, kind: TargetKind::Edge, span: Some((0.0, 10.0)) };
            let edge = alignment(true, &hit, Rect::new(50.0, 40.0, 70.0, 80.0), EXTENT);
            assert_eq!(edge.line, (Point::new(50.0, 0.0), Point::new(50.0, 80.0)));
            assert!(edge.marks.contains(&(Mark::Cross, Point::new(50.0, 40.0))) && edge.marks.contains(&(Mark::Cross, Point::new(50.0, 80.0))));
            let centre = alignment(true, &hit, Rect::new(40.0, 40.0, 60.0, 80.0), EXTENT);
            assert!(centre.marks.contains(&(Mark::Plus, Point::new(50.0, 60.0))), "{:?}", centre.marks);
        }

        #[test]
        fn horizontal_lines_mirror_the_vertical_ones() {
            let hit = Target { at: 20.0, kind: TargetKind::Endpoint, span: Some((5.0, 5.0)) };
            let a = alignment(false, &hit, cursor(30.0, 20.0), EXTENT);
            assert_eq!(a.line, (Point::new(5.0, 20.0), Point::new(30.0, 20.0)));
            assert_eq!(a.marks, [(Mark::Square, Point::new(5.0, 20.0))]);
        }

        #[test]
        fn guides_and_the_bed_centre_run_the_length_they_are_given() {
            let guide = Target { at: 70.0, kind: TargetKind::Guide, span: None };
            assert_eq!(alignment(true, &guide, cursor(70.0, 100.0), EXTENT).line, (Point::new(70.0, 0.0), Point::new(70.0, 300.0)));
            let centre = Target { at: 200.0, kind: TargetKind::BedCenter, span: Some((0.0, 300.0)) };
            let a = alignment(true, &centre, cursor(200.0, 10.0), EXTENT);
            assert_eq!(a.marks, [(Mark::Plus, Point::new(200.0, 150.0))]);
        }

        #[test]
        fn non_finite_numbers_do_not_produce_panics() {
            let hit = Target { at: f64::NAN, kind: TargetKind::Edge, span: Some((f64::INFINITY, f64::NAN)) };
            let _ = alignment(true, &hit, Rect::new(f64::NAN, 0.0, 1.0, f64::INFINITY), EXTENT);
            let _ = alignment(false, &hit, cursor(f64::NAN, f64::NAN), EXTENT);
        }
    }

    mod vertex_tests {
        use kurbo::{Affine, BezPath};

        use super::*;

        #[test]
        fn rectangles_give_corners_and_edge_midpoints_in_workspace_space() {
            let path = lightfoundry_geom::rect_path(0.0, 0.0, 10.0, 4.0);
            let pts = path_points(&path, Affine::translate((100.0, 50.0)), 100);
            for want in [Point::new(100.0, 50.0), Point::new(110.0, 54.0), Point::new(105.0, 50.0), Point::new(110.0, 52.0)] {
                assert!(pts.iter().any(|(p, _)| p.distance(want) < 1e-9), "{want:?} in {pts:?}");
            }
        }

        #[test]
        fn curves_contribute_endpoints_only_and_the_cap_and_bad_numbers_are_respected() {
            let mut curve = BezPath::new();
            curve.move_to((0.0, 0.0));
            curve.curve_to((5.0, 30.0), (15.0, 30.0), (20.0, 0.0));
            let pts = path_points(&curve, Affine::IDENTITY, 100);
            assert_eq!(pts, vec![(Point::new(0.0, 0.0), TargetKind::Endpoint), (Point::new(20.0, 0.0), TargetKind::Endpoint)], "no control points");
            let big = lightfoundry_geom::rect_path(0.0, 0.0, 10.0, 10.0);
            assert_eq!(path_points(&big, Affine::IDENTITY, 3).len(), 3);
            assert!(path_points(&big, Affine::IDENTITY, 0).is_empty());
            assert!(path_points(&big, Affine::scale(f64::NAN), 100).is_empty(), "non-finite results are dropped");
        }

        #[test]
        fn vertices_attract_on_both_axes() {
            let mut t = SnapTargets::default();
            t.add_point(Point::new(33.0, 44.0), TargetKind::Endpoint);
            let s = snap_point(Point::new(32.0, 45.0), &SnapSettings::default(), &t, 2.0);
            assert_eq!(s.point, Point::new(33.0, 44.0));
        }
    }

    mod point_tests {
        use super::*;

        #[test]
        fn nearby_edges_and_centres_attract_and_report_their_line() {
            let s = snap_point(Point::new(101.2, 69.0), &SnapSettings::default(), &targets(), 2.0);
            assert_eq!(s.point, Point::new(100.0, 70.0), "left edge, and the centre line y=70");
            assert_eq!((s.x_line.map(|t| t.at), s.y_line.map(|t| t.at)), (Some(100.0), Some(70.0)));
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
            t.xs.push(Target { at: f64::NAN, kind: TargetKind::Edge, span: None });
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
            assert_eq!(xl.map(|t| t.at), Some(100.0));
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
