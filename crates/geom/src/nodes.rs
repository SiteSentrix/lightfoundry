//! An editable view of a Bézier path: nodes with optional handles, as a person edits it.
//!
//! A `NodePath` is made of sub-paths; each sub-path is a list of nodes. Segment `i` joins node `i`
//! to node `i + 1` (and, on a closed sub-path, the last node to the first). A segment is a curve when
//! either end has a handle on that side. Handles are stored relative to their node.

use kurbo::{BezPath, PathEl, Point, Vec2};
use thiserror::Error;

/// Most nodes accepted in one path, so hostile files cannot make editing run away.
pub const MAX_NODES: usize = 200_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub p: Point,
    /// Handle toward the previous node, relative to `p`.
    pub in_h: Option<Vec2>,
    /// Handle toward the next node, relative to `p`.
    pub out_h: Option<Vec2>,
    /// A smooth node keeps its two handles on one line when either is moved.
    pub smooth: bool,
}

impl Node {
    pub fn corner(p: Point) -> Node {
        Node { p, in_h: None, out_h: None, smooth: false }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubPath {
    pub nodes: Vec<Node>,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodePath {
    pub subpaths: Vec<SubPath>,
}

#[derive(Debug, Error, PartialEq)]
pub enum NodeError {
    #[error("there is no sub-path {0}")]
    NoSubpath(usize),
    #[error("there is no node {0} in that sub-path")]
    NoNode(usize),
    #[error("there is no segment {0} in that sub-path")]
    NoSegment(usize),
    #[error("a coordinate is not a finite number")]
    NotFinite,
    #[error("a path needs at least {0} nodes")]
    TooFewNodes(usize),
    #[error("too many nodes (limit {MAX_NODES})")]
    TooMany,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

/// Where a point lies on a path: the nearest sub-path, segment, curve parameter and distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnPath {
    pub subpath: usize,
    pub segment: usize,
    pub t: f64,
    pub distance: f64,
}

/// Turns a closed or open polyline into curves: vertices where the line bends gently become smooth
/// nodes whose handles follow the line (a third of the way to each neighbour); sharper bends
/// (turning more than `corner_deg` degrees) stay corners.
pub fn smooth_polyline(points: &[Point], closed: bool, corner_deg: f64) -> SubPath {
    let n = points.len();
    let limit = corner_deg.clamp(0.0, 180.0).to_radians();
    let mut nodes = Vec::with_capacity(n);
    for (i, p) in points.iter().enumerate() {
        let prev = if i > 0 {
            Some(points[i - 1])
        } else if closed {
            points.last().copied()
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(points[i + 1])
        } else if closed {
            points.first().copied()
        } else {
            None
        };
        let (Some(a), Some(b)) = (prev, next) else {
            nodes.push(Node::corner(*p));
            continue;
        };
        let (d_in, d_out) = (*p - a, b - *p);
        let (l_in, l_out) = (d_in.hypot(), d_out.hypot());
        if l_in < 1e-12 || l_out < 1e-12 {
            nodes.push(Node::corner(*p));
            continue;
        }
        let turn = (d_in.x * d_out.y - d_in.y * d_out.x).atan2(d_in.x * d_out.x + d_in.y * d_out.y).abs();
        if turn > limit {
            nodes.push(Node::corner(*p));
            continue;
        }
        let tangent = (b - a) / (b - a).hypot().max(1e-12);
        nodes.push(Node { p: *p, in_h: Some(-tangent * (l_in / 3.0)), out_h: Some(tangent * (l_out / 3.0)), smooth: true });
    }
    SubPath { nodes, closed }
}

impl NodePath {
    /// Builds the editable form of `path`. Quadratics become cubics; a closing curve back to the
    /// start folds into the first node's handle.
    pub fn from_bez(path: &BezPath) -> Result<NodePath, NodeError> {
        let mut out = NodePath::default();
        let mut cur: Option<SubPath> = None;
        let finish = |cur: &mut Option<SubPath>, out: &mut NodePath, closed: bool| {
            if let Some(mut sp) = cur.take() {
                if closed && sp.nodes.len() >= 2 {
                    let (first, last) = (sp.nodes[0].clone(), sp.nodes[sp.nodes.len() - 1].clone());
                    if first.p.distance(last.p) < 1e-9 {
                        sp.nodes[0].in_h = last.in_h;
                        sp.nodes.pop();
                    }
                    sp.closed = true;
                }
                if sp.nodes.len() >= 2 {
                    out.subpaths.push(sp);
                }
            }
        };
        let mut count = 0usize;
        for el in path.elements() {
            count += 1;
            if count > MAX_NODES {
                return Err(NodeError::TooMany);
            }
            match *el {
                PathEl::MoveTo(p) => {
                    finish(&mut cur, &mut out, false);
                    cur = Some(SubPath { nodes: vec![Node::corner(check(p)?)], closed: false });
                }
                PathEl::LineTo(p) => {
                    if let Some(sp) = cur.as_mut() {
                        sp.nodes.push(Node::corner(check(p)?));
                    }
                }
                PathEl::QuadTo(q, p) => {
                    if let Some(sp) = cur.as_mut() {
                        let (q, p) = (check(q)?, check(p)?);
                        let prev = sp.nodes.last().map_or(p, |n| n.p);
                        let c1 = prev + (q - prev) * (2.0 / 3.0);
                        let c2 = p + (q - p) * (2.0 / 3.0);
                        if let Some(last) = sp.nodes.last_mut() {
                            last.out_h = Some(c1 - prev);
                        }
                        sp.nodes.push(Node { p, in_h: Some(c2 - p), out_h: None, smooth: false });
                    }
                }
                PathEl::CurveTo(c1, c2, p) => {
                    if let Some(sp) = cur.as_mut() {
                        let (c1, c2, p) = (check(c1)?, check(c2)?, check(p)?);
                        let prev = sp.nodes.last().map_or(p, |n| n.p);
                        if let Some(last) = sp.nodes.last_mut() {
                            last.out_h = Some(c1 - prev);
                        }
                        sp.nodes.push(Node { p, in_h: Some(c2 - p), out_h: None, smooth: false });
                    }
                }
                PathEl::ClosePath => finish(&mut cur, &mut out, true),
            }
        }
        finish(&mut cur, &mut out, false);
        for sp in &mut out.subpaths {
            for n in &mut sp.nodes {
                n.smooth = is_smooth(n);
            }
        }
        Ok(out)
    }

    /// The path these nodes describe.
    pub fn to_bez(&self) -> BezPath {
        let mut path = BezPath::new();
        for sp in &self.subpaths {
            let n = sp.nodes.len();
            if n < 2 {
                continue;
            }
            path.move_to(sp.nodes[0].p);
            let segments = if sp.closed { n } else { n - 1 };
            for i in 0..segments {
                let (a, b) = (&sp.nodes[i], &sp.nodes[(i + 1) % n]);
                let closing = sp.closed && i == n - 1;
                match controls(a, b) {
                    Some((c1, c2)) => path.curve_to(c1, c2, b.p),
                    // A straight closing segment is implied by ClosePath.
                    None if closing => {}
                    None => path.line_to(b.p),
                }
            }
            if sp.closed {
                path.close_path();
            }
        }
        path
    }

    pub fn node_count(&self) -> usize {
        self.subpaths.iter().map(|s| s.nodes.len()).sum()
    }

    /// Moves a node (its handles come with it).
    pub fn move_node(&mut self, subpath: usize, index: usize, to: Point) -> Result<(), NodeError> {
        let to = check(to)?;
        self.node_mut(subpath, index)?.p = to;
        Ok(())
    }

    /// Moves a handle so its tip lies at `tip`. On a smooth node the opposite handle turns to stay
    /// in line, keeping its own length.
    pub fn move_handle(&mut self, subpath: usize, index: usize, side: Side, tip: Point) -> Result<(), NodeError> {
        let tip = check(tip)?;
        let node = self.node_mut(subpath, index)?;
        let h = tip - node.p;
        match side {
            Side::In => node.in_h = Some(h),
            Side::Out => node.out_h = Some(h),
        }
        if node.smooth && h.hypot() > 1e-12 {
            let other = match side {
                Side::In => &mut node.out_h,
                Side::Out => &mut node.in_h,
            };
            let len = other.map_or(h.hypot(), |o| o.hypot());
            *other = Some(-h / h.hypot() * len);
        }
        Ok(())
    }

    /// Makes a node smooth (handles collinear, created from its neighbours if it has none) or a
    /// corner (handles kept, but independent).
    pub fn set_smooth(&mut self, subpath: usize, index: usize, smooth: bool) -> Result<(), NodeError> {
        let sp = self.subpath(subpath)?;
        let n = sp.nodes.len();
        if index >= n {
            return Err(NodeError::NoNode(index));
        }
        let closed = sp.closed;
        let prev = if index > 0 || closed { Some(sp.nodes[(index + n - 1) % n].p) } else { None };
        let next = if index + 1 < n || closed { Some(sp.nodes[(index + 1) % n].p) } else { None };
        let node = &mut self.subpaths[subpath].nodes[index];
        node.smooth = smooth;
        if smooth {
            if node.in_h.is_none() && node.out_h.is_none() {
                // Tangent along prev -> next, handles a third of the way to each neighbour.
                let tangent = match (prev, next) {
                    (Some(a), Some(b)) => (b - a) / 6.0,
                    (None, Some(b)) => (b - node.p) / 3.0,
                    (Some(a), None) => (node.p - a) / 3.0,
                    (None, None) => Vec2::ZERO,
                };
                if tangent.hypot() > 1e-12 {
                    node.in_h = prev.map(|_| -tangent);
                    node.out_h = next.map(|_| tangent);
                }
            } else if let (Some(i), Some(o)) = (node.in_h, node.out_h) {
                // Keep the outgoing direction, align the incoming one with it.
                if o.hypot() > 1e-12 {
                    node.in_h = Some(-o / o.hypot() * i.hypot());
                }
            }
        }
        Ok(())
    }

    /// Inserts a node on segment `segment` at parameter `t`, keeping the shape of the path exactly.
    /// Returns the new node's index.
    pub fn insert(&mut self, subpath: usize, segment: usize, t: f64) -> Result<usize, NodeError> {
        let t = if t.is_finite() { t.clamp(0.001, 0.999) } else { return Err(NodeError::NotFinite) };
        if self.node_count() >= MAX_NODES {
            return Err(NodeError::TooMany);
        }
        let sp = self.subpath(subpath)?;
        let n = sp.nodes.len();
        if segment >= segment_count(sp) {
            return Err(NodeError::NoSegment(segment));
        }
        let (a, b) = (sp.nodes[segment].clone(), sp.nodes[(segment + 1) % n].clone());
        let sp = &mut self.subpaths[subpath];
        let new = match controls(&a, &b) {
            None => Node::corner(a.p.lerp(b.p, t)),
            Some((c1, c2)) => {
                let (p01, p12, p23) = (a.p.lerp(c1, t), c1.lerp(c2, t), c2.lerp(b.p, t));
                let (p012, p123) = (p01.lerp(p12, t), p12.lerp(p23, t));
                let mid = p012.lerp(p123, t);
                sp.nodes[segment].out_h = Some(p01 - a.p);
                let after = (segment + 1) % n;
                sp.nodes[after].in_h = Some(p23 - b.p);
                Node { p: mid, in_h: Some(p012 - mid), out_h: Some(p123 - mid), smooth: true }
            }
        };
        sp.nodes.insert(segment + 1, new);
        Ok(segment + 1)
    }

    /// Removes a node; the path closes over the gap.
    pub fn delete(&mut self, subpath: usize, index: usize) -> Result<(), NodeError> {
        let sp = self.subpath(subpath)?;
        if index >= sp.nodes.len() {
            return Err(NodeError::NoNode(index));
        }
        let minimum = if sp.closed { 3 } else { 2 };
        if sp.nodes.len() <= minimum {
            return Err(NodeError::TooFewNodes(minimum + 1));
        }
        self.subpaths[subpath].nodes.remove(index);
        Ok(())
    }

    /// Turns a segment into a curve (handles a third of the way along) or a straight line.
    pub fn set_segment_curve(&mut self, subpath: usize, segment: usize, curve: bool) -> Result<(), NodeError> {
        let sp = self.subpath(subpath)?;
        let n = sp.nodes.len();
        if segment >= segment_count(sp) {
            return Err(NodeError::NoSegment(segment));
        }
        let next = (segment + 1) % n;
        let (pa, pb) = (sp.nodes[segment].p, sp.nodes[next].p);
        let sp = &mut self.subpaths[subpath];
        if curve {
            if controls(&sp.nodes[segment], &sp.nodes[next]).is_none() {
                sp.nodes[segment].out_h = Some((pb - pa) / 3.0);
                sp.nodes[next].in_h = Some((pa - pb) / 3.0);
            }
        } else {
            sp.nodes[segment].out_h = None;
            sp.nodes[next].in_h = None;
        }
        Ok(())
    }

    pub fn set_closed(&mut self, subpath: usize, closed: bool) -> Result<(), NodeError> {
        let sp = self.subpath(subpath)?;
        if closed && sp.nodes.len() < 3 {
            return Err(NodeError::TooFewNodes(3));
        }
        self.subpaths[subpath].closed = closed;
        Ok(())
    }

    /// The nearest point on the path to `p` (within the curve's flattened approximation).
    pub fn nearest(&self, p: Point) -> Option<OnPath> {
        let mut best: Option<OnPath> = None;
        for (si, sp) in self.subpaths.iter().enumerate() {
            let n = sp.nodes.len();
            for seg in 0..segment_count(sp) {
                let (a, b) = (&sp.nodes[seg], &sp.nodes[(seg + 1) % n]);
                let steps = if controls(a, b).is_some() { 32 } else { 1 };
                for k in 0..steps {
                    let (t0, t1) = (f64::from(k) / f64::from(steps), f64::from(k + 1) / f64::from(steps));
                    let (p0, p1) = (point_at(a, b, t0), point_at(a, b, t1));
                    let d = p1 - p0;
                    let len2 = d.hypot2();
                    let local = if len2 == 0.0 { 0.0 } else { ((p - p0).dot(d) / len2).clamp(0.0, 1.0) };
                    let q = p0 + d * local;
                    let dist = p.distance(q);
                    if best.is_none_or(|b| dist < b.distance) {
                        best = Some(OnPath { subpath: si, segment: seg, t: t0 + (t1 - t0) * local, distance: dist });
                    }
                }
            }
        }
        best
    }

    fn subpath(&self, i: usize) -> Result<&SubPath, NodeError> {
        self.subpaths.get(i).ok_or(NodeError::NoSubpath(i))
    }

    fn node_mut(&mut self, subpath: usize, index: usize) -> Result<&mut Node, NodeError> {
        self.subpaths.get_mut(subpath).ok_or(NodeError::NoSubpath(subpath))?.nodes.get_mut(index).ok_or(NodeError::NoNode(index))
    }
}

/// Number of segments in a sub-path.
pub fn segment_count(sp: &SubPath) -> usize {
    match sp.nodes.len() {
        0 | 1 => 0,
        n if sp.closed => n,
        n => n - 1,
    }
}

/// The two control points of the segment from `a` to `b`, if it is a curve.
pub fn controls(a: &Node, b: &Node) -> Option<(Point, Point)> {
    if a.out_h.is_none() && b.in_h.is_none() {
        return None;
    }
    Some((a.p + a.out_h.unwrap_or(Vec2::ZERO), b.p + b.in_h.unwrap_or(Vec2::ZERO)))
}

/// The point at parameter `t` on the segment from `a` to `b`.
pub fn point_at(a: &Node, b: &Node, t: f64) -> Point {
    match controls(a, b) {
        None => a.p.lerp(b.p, t),
        Some((c1, c2)) => {
            let u = 1.0 - t;
            Point::new(
                u * u * u * a.p.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * b.p.x,
                u * u * u * a.p.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * b.p.y,
            )
        }
    }
}

fn is_smooth(n: &Node) -> bool {
    match (n.in_h, n.out_h) {
        (Some(i), Some(o)) => {
            let (li, lo) = (i.hypot(), o.hypot());
            li > 1e-9 && lo > 1e-9 && (i.x * o.y - i.y * o.x).abs() / (li * lo) < 1e-3 && i.dot(o) < 0.0
        }
        _ => false,
    }
}

fn check(p: Point) -> Result<Point, NodeError> {
    if p.x.is_finite() && p.y.is_finite() { Ok(p) } else { Err(NodeError::NotFinite) }
}

#[cfg(test)]
mod tests;
