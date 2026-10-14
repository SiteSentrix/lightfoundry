//! Walking along a polyline by distance: where it is, and which way it points, after `s` mm.

use kurbo::{Point, Vec2};

use crate::Polyline;

/// A polyline measured by arc length.
#[derive(Debug, Clone)]
pub struct Along {
    points: Vec<Point>,
    /// `cum[i]` is the distance from the start to `points[i]`.
    cum: Vec<f64>,
    closed: bool,
}

impl Along {
    /// `None` for fewer than two distinct finite points.
    pub fn new(line: &Polyline) -> Option<Along> {
        let mut points: Vec<Point> = Vec::with_capacity(line.points.len() + 1);
        for p in &line.points {
            if !(p.x.is_finite() && p.y.is_finite()) {
                return None;
            }
            if points.last().is_none_or(|q| q.distance(*p) > 1e-9) {
                points.push(*p);
            }
        }
        if line.closed
            && let Some(first) = points.first().copied()
            && points.last().is_some_and(|q| q.distance(first) > 1e-9)
        {
            points.push(first);
        }
        if points.len() < 2 {
            return None;
        }
        let mut cum = Vec::with_capacity(points.len());
        let mut total = 0.0;
        cum.push(0.0);
        for w in points.windows(2) {
            total += w[0].distance(w[1]);
            cum.push(total);
        }
        Some(Along { points, cum, closed: line.closed })
    }

    pub fn length(&self) -> f64 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    /// The point `s` mm along, and the unit direction there. Closed lines wrap around; open ones
    /// continue straight beyond their ends.
    pub fn at(&self, s: f64) -> (Point, Vec2) {
        let total = self.length();
        let s = if !s.is_finite() {
            0.0
        } else if self.closed && total > 0.0 {
            s.rem_euclid(total)
        } else {
            s
        };
        let last = self.points.len() - 1;
        let i = if s <= 0.0 {
            0
        } else if s >= total {
            last - 1
        } else {
            self.cum.partition_point(|c| *c <= s).saturating_sub(1).min(last - 1)
        };
        let (a, b) = (self.points[i], self.points[i + 1]);
        let dir = (b - a) / a.distance(b);
        (a + dir * (s - self.cum[i]), dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(pts: &[(f64, f64)], closed: bool) -> Polyline {
        Polyline::new(pts.iter().map(|&(x, y)| Point::new(x, y)).collect(), closed)
    }

    #[test]
    fn positions_and_directions_follow_the_corners() {
        let a = Along::new(&line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false)).unwrap();
        assert_eq!(a.length(), 20.0);
        let (p, d) = a.at(5.0);
        assert!(p.distance(Point::new(5.0, 0.0)) < 1e-9 && (d.x - 1.0).abs() < 1e-9);
        let (p, d) = a.at(15.0);
        assert!(p.distance(Point::new(10.0, 5.0)) < 1e-9 && (d.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn open_lines_continue_straight_and_closed_ones_wrap() {
        let open = Along::new(&line(&[(0.0, 0.0), (10.0, 0.0)], false)).unwrap();
        assert!(open.at(-5.0).0.distance(Point::new(-5.0, 0.0)) < 1e-9);
        assert!(open.at(25.0).0.distance(Point::new(25.0, 0.0)) < 1e-9);
        let square = Along::new(&line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true)).unwrap();
        assert_eq!(square.length(), 40.0);
        assert!(square.at(45.0).0.distance(Point::new(5.0, 0.0)) < 1e-9);
        assert!(square.at(-5.0).0.distance(Point::new(0.0, 5.0)) < 1e-9, "backwards from the start is the last edge");
    }

    #[test]
    fn degenerate_and_hostile_lines_are_refused_or_survived() {
        assert!(Along::new(&line(&[], false)).is_none());
        assert!(Along::new(&line(&[(1.0, 1.0)], false)).is_none());
        assert!(Along::new(&line(&[(1.0, 1.0), (1.0, 1.0)], true)).is_none(), "all points coincide");
        assert!(Along::new(&line(&[(0.0, 0.0), (f64::NAN, 1.0)], false)).is_none());
        let a = Along::new(&line(&[(0.0, 0.0), (0.0, 0.0), (3.0, 4.0)], false)).unwrap();
        assert_eq!(a.length(), 5.0, "repeated points are ignored");
        for s in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300] {
            let (p, d) = a.at(s);
            assert!(d.hypot() > 0.99 && d.hypot() < 1.01, "{s}: {p:?}");
        }
    }
}
