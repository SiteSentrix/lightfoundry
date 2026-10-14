//! Fitting smooth curves through a polyline by least squares (the approach described in Philip J.
//! Schneider, "An Algorithm for Automatically Fitting Digitized Curves", Graphics Gems, 1990):
//! each run between corners becomes as few cubic Béziers as will stay within a tolerance of the
//! original points, split where the fit is worst.

use kurbo::{Point, Vec2};

use crate::nodes::{Node, SubPath};

/// Recursion depth after which a piece is accepted as it is, so hostile input cannot run away.
const MAX_DEPTH: usize = 40;
/// Newton refinements of the point parameters before giving up and splitting.
const REPARAMETERIZE: usize = 4;

type Cubic = [Point; 4];

/// Turns `points` into curves that stay within `tolerance` of them. Bends sharper than
/// `corner_deg` degrees stay corners; everywhere else the curve is smooth. Fewer than three points
/// are returned as they are.
pub fn fit_polyline(points: &[Point], closed: bool, corner_deg: f64, tolerance: f64) -> SubPath {
    let tol = if tolerance.is_finite() { tolerance.clamp(1e-6, 1e6) } else { 1.0 };
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if p.x.is_finite() && p.y.is_finite() && pts.last().is_none_or(|q| q.distance(*p) > 1e-9) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.distance(*b) <= 1e-9) {
        pts.pop();
    }
    if pts.len() < 3 {
        return SubPath { nodes: pts.into_iter().map(Node::corner).collect(), closed: closed && points.len() >= 3 };
    }
    let limit = corner_deg.clamp(0.0, 180.0).to_radians();
    let n = pts.len();
    let at = |i: usize| pts[i % n];
    let is_corner = |i: usize| -> bool {
        if !closed && (i == 0 || i == n - 1) {
            return true;
        }
        let (a, b, c) = (at(i + n - 1), at(i), at(i + 1));
        let (d_in, d_out) = (b - a, c - b);
        let turn = (d_in.x * d_out.y - d_in.y * d_out.x).atan2(d_in.x * d_out.x + d_in.y * d_out.y).abs();
        turn > limit
    };
    let corners: Vec<usize> = (0..n).filter(|i| is_corner(*i)).collect();

    // Runs of points between consecutive corners; a closed loop without corners is one run that
    // starts and ends at point 0 and is smooth across the join.
    let mut runs: Vec<(Vec<Point>, bool)> = Vec::new(); // (points, smooth_start)
    if corners.is_empty() {
        runs.push(((0..=n).map(at).collect(), true));
    } else {
        for (k, &start) in corners.iter().enumerate() {
            let end = corners.get(k + 1).copied().unwrap_or(if closed { corners[0] + n } else { start });
            if end <= start {
                continue;
            }
            runs.push(((start..=end).map(at).collect(), false));
        }
    }

    let mut nodes: Vec<Node> = Vec::new();
    for (run, smooth_start) in &runs {
        let (first, last) = (run[0], run[run.len() - 1]);
        let (t1, t2) = if *smooth_start && closed && corners.is_empty() {
            let t = unit(at(1) - at(n - 1)).unwrap_or(Vec2::new(1.0, 0.0));
            (t, -t)
        } else {
            (unit(run[1] - first).unwrap_or(Vec2::ZERO), unit(run[run.len() - 2] - last).unwrap_or(Vec2::ZERO))
        };
        let segments = fit_cubic(run, t1, t2, tol * tol, 0);
        for (i, seg) in segments.iter().enumerate() {
            let smooth_here = i > 0 || (*smooth_start && corners.is_empty());
            let out_h = Some(seg[1] - seg[0]);
            if i == 0 {
                nodes.push(Node { p: seg[0], in_h: None, out_h, smooth: smooth_here });
            } else if let Some(prev) = nodes.last_mut() {
                // `prev` is the node at this joint: its outgoing handle starts the next segment.
                prev.out_h = out_h;
            }
            nodes.push(Node { p: seg[3], in_h: Some(seg[2] - seg[3]), out_h: None, smooth: true });
        }
    }
    finish(nodes, &corners, closed)
}

/// Merges the joints between runs (corners) and closes the loop.
fn finish(nodes: Vec<Node>, corners: &[usize], closed: bool) -> SubPath {
    // Each run pushed its own first and last node; where one run ends at a corner and the next
    // starts there, keep a single corner node carrying the handles of both sides.
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for node in nodes {
        match out.last_mut() {
            Some(prev) if prev.p.distance(node.p) < 1e-9 && node.in_h.is_none() => {
                // Start of a new run at the same point as the previous run's end.
                prev.out_h = node.out_h;
                prev.smooth = false;
            }
            _ => out.push(node),
        }
    }
    if closed {
        if out.len() >= 2 && out[0].p.distance(out[out.len() - 1].p) < 1e-9 {
            let last = out.pop();
            if let (Some(last), Some(first)) = (last, out.first_mut()) {
                first.in_h = last.in_h;
                first.smooth = corners.is_empty();
            }
        }
    } else if let Some(last) = out.last_mut() {
        last.smooth = false;
    }
    if let Some(first) = out.first_mut()
        && !closed
    {
        first.smooth = false;
    }
    SubPath { nodes: out, closed }
}

fn unit(v: Vec2) -> Option<Vec2> {
    let l = v.hypot();
    (l > 1e-12 && l.is_finite()).then(|| v / l)
}

fn fit_cubic(pts: &[Point], t1: Vec2, t2: Vec2, tol_sq: f64, depth: usize) -> Vec<Cubic> {
    let n = pts.len();
    let (p0, p3) = (pts[0], pts[n - 1]);
    if n == 2 {
        let d = p0.distance(p3) / 3.0;
        return vec![[p0, p0 + t1 * d, p3 + t2 * d, p3]];
    }
    let mut u = chord_params(pts);
    let mut bez = generate(pts, &u, t1, t2);
    let (mut err, mut split) = max_error(pts, &bez, &u);
    if err < tol_sq {
        return vec![bez];
    }
    if err < tol_sq * 16.0 {
        for _ in 0..REPARAMETERIZE {
            u = reparameterize(pts, &u, &bez);
            bez = generate(pts, &u, t1, t2);
            (err, split) = max_error(pts, &bez, &u);
            if err < tol_sq {
                return vec![bez];
            }
        }
    }
    if depth >= MAX_DEPTH || split == 0 || split + 1 >= n {
        return vec![bez];
    }
    let centre = unit(pts[split - 1] - pts[split + 1]).unwrap_or_else(|| {
        let d = p3 - p0;
        unit(Vec2::new(-d.y, d.x)).unwrap_or(Vec2::new(1.0, 0.0))
    });
    let mut left = fit_cubic(&pts[..=split], t1, centre, tol_sq, depth + 1);
    left.extend(fit_cubic(&pts[split..], -centre, t2, tol_sq, depth + 1));
    left
}

fn chord_params(pts: &[Point]) -> Vec<f64> {
    let mut u = vec![0.0];
    for w in pts.windows(2) {
        u.push(u.last().copied().unwrap_or(0.0) + w[0].distance(w[1]));
    }
    let total = u.last().copied().unwrap_or(0.0);
    if total > 0.0 {
        for v in &mut u {
            *v /= total;
        }
    }
    u
}

fn generate(pts: &[Point], u: &[f64], t1: Vec2, t2: Vec2) -> Cubic {
    let (p0, p3) = (pts[0], pts[pts.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &t) in pts.iter().zip(u) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t);
        let (a1, a2) = (t1 * b1, t2 * b2);
        c00 += a1.dot(a1);
        c01 += a1.dot(a2);
        c11 += a2.dot(a2);
        let tmp = *p - (p0.to_vec2() * (b0 + b1) + p3.to_vec2() * (b2 + b3)).to_point();
        x0 += a1.dot(tmp);
        x1 += a2.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let seg = p0.distance(p3);
    let (mut alpha1, mut alpha2) = if det.abs() > 1e-12 { ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det) } else { (0.0, 0.0) };
    // A negative or tiny handle would fold the curve back on itself: fall back to thirds.
    let eps = 1e-6 * seg;
    if !(alpha1.is_finite() && alpha2.is_finite()) || alpha1 < eps || alpha2 < eps {
        alpha1 = seg / 3.0;
        alpha2 = seg / 3.0;
    }
    [p0, p0 + t1 * alpha1, p3 + t2 * alpha2, p3]
}

fn eval(b: &Cubic, t: f64) -> Point {
    let s = 1.0 - t;
    let v = b[0].to_vec2() * (s * s * s) + b[1].to_vec2() * (3.0 * t * s * s) + b[2].to_vec2() * (3.0 * t * t * s) + b[3].to_vec2() * (t * t * t);
    v.to_point()
}

fn max_error(pts: &[Point], b: &Cubic, u: &[f64]) -> (f64, usize) {
    let (mut worst, mut at) = (0.0, pts.len() / 2);
    for i in 1..pts.len() - 1 {
        let d = eval(b, u[i]).distance_squared(pts[i]);
        if d >= worst {
            worst = d;
            at = i;
        }
    }
    (worst, at)
}

fn reparameterize(pts: &[Point], u: &[f64], b: &Cubic) -> Vec<f64> {
    // First and second derivative control points.
    let q1 = [(b[1] - b[0]) * 3.0, (b[2] - b[1]) * 3.0, (b[3] - b[2]) * 3.0];
    let q2 = [(q1[1] - q1[0]) * 2.0, (q1[2] - q1[1]) * 2.0];
    pts.iter()
        .zip(u)
        .map(|(p, &t)| {
            let s = 1.0 - t;
            let d1 = q1[0] * (s * s) + q1[1] * (2.0 * s * t) + q1[2] * (t * t);
            let d2 = q2[0] * s + q2[1] * t;
            let diff = eval(b, t) - *p;
            let den = d1.dot(d1) + diff.dot(d2);
            let next = if den.abs() > 1e-12 { t - diff.dot(d1) / den } else { t };
            if next.is_finite() { next.clamp(0.0, 1.0) } else { t }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{CubicBez, ParamCurve, ParamCurveNearest};

    fn circle(n: usize, r: f64) -> Vec<Point> {
        (0..n)
            .map(|i| Point::new(r * (i as f64 / n as f64 * std::f64::consts::TAU).cos(), r * (i as f64 / n as f64 * std::f64::consts::TAU).sin()))
            .collect()
    }

    fn curves(sp: &SubPath) -> Vec<CubicBez> {
        let n = sp.nodes.len();
        let count = if sp.closed { n } else { n.saturating_sub(1) };
        (0..count)
            .map(|i| {
                let (a, b) = (&sp.nodes[i], &sp.nodes[(i + 1) % n]);
                CubicBez::new(a.p, a.p + a.out_h.unwrap_or(Vec2::ZERO), b.p + b.in_h.unwrap_or(Vec2::ZERO), b.p)
            })
            .collect()
    }

    #[test]
    fn a_circle_of_many_points_becomes_a_few_smooth_curves_within_tolerance() {
        let pts = circle(120, 50.0);
        let sp = fit_polyline(&pts, true, 60.0, 0.2);
        assert!(sp.closed && sp.nodes.len() <= 12, "{} nodes", sp.nodes.len());
        assert!(sp.nodes.iter().all(|n| n.smooth), "no corners on a circle");
        let cs = curves(&sp);
        for p in &pts {
            let d = cs.iter().map(|c| c.nearest(*p, 1e-6).distance_sq.sqrt()).fold(f64::INFINITY, f64::min);
            assert!(d < 0.25, "point {p:?} is {d} away");
        }
        let end_gap = cs.last().map(|c| c.eval(1.0).distance(sp.nodes[0].p));
        assert!(end_gap.is_some_and(|g| g < 1e-9), "the loop closes");
    }

    #[test]
    fn a_square_keeps_its_corners_and_its_straight_sides() {
        let pts = [Point::new(0.0, 0.0), Point::new(5.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        let sp = fit_polyline(&pts, true, 60.0, 0.1);
        assert!(sp.closed);
        let corners: Vec<Point> = sp.nodes.iter().filter(|n| !n.smooth).map(|n| n.p).collect();
        for c in [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)] {
            assert!(corners.contains(&c), "{c:?} stays a corner: {corners:?}");
        }
        for c in curves(&sp) {
            let mid = c.eval(0.5);
            assert!(mid.x >= -1e-6 && mid.x <= 10.0 + 1e-6 && mid.y >= -1e-6 && mid.y <= 10.0 + 1e-6, "{mid:?}");
        }
    }

    #[test]
    fn an_open_wave_is_fitted_end_to_end() {
        let pts: Vec<Point> = (0..=60).map(|i| Point::new(i as f64, 8.0 * (i as f64 / 6.0).sin())).collect();
        let sp = fit_polyline(&pts, false, 60.0, 0.1);
        assert!(!sp.closed && sp.nodes.len() < 25, "{} nodes", sp.nodes.len());
        assert_eq!((sp.nodes[0].p, sp.nodes[sp.nodes.len() - 1].p), (pts[0], pts[60]));
        let cs = curves(&sp);
        for p in &pts {
            let d = cs.iter().map(|c| c.nearest(*p, 1e-6).distance_sq.sqrt()).fold(f64::INFINITY, f64::min);
            assert!(d < 0.15, "{p:?} {d}");
        }
    }

    #[test]
    fn degenerate_input_never_panics() {
        for (pts, closed) in [
            (vec![], true),
            (vec![Point::new(1.0, 1.0)], false),
            (vec![Point::new(1.0, 1.0); 5], true),
            (vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)], true),
            (vec![Point::new(f64::NAN, 0.0), Point::new(1.0, 0.0), Point::new(2.0, 1.0)], false),
            ((0..50).map(|i| Point::new(i as f64, 0.0)).collect(), false),
            ((0..50).map(|i| Point::new((i % 2) as f64 * 1e6, i as f64)).collect(), true),
        ] {
            for tol in [0.0, f64::NAN, 1e-9, 1e12] {
                let sp = fit_polyline(&pts, closed, 60.0, tol);
                assert!(sp.nodes.iter().all(|n| n.p.x.is_finite() && n.p.y.is_finite()));
            }
        }
    }
}
