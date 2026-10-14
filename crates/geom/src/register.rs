//! Two-point registration: the move, turn and (optionally) stretch that carries two marks where the
//! design has them onto where they really are, as in print-and-cut or camera alignment.

use kurbo::{Affine, Point};
use thiserror::Error;

/// Marks closer together than this (mm) cannot define a direction.
pub const MIN_SEPARATION_MM: f64 = 1e-3;
/// Largest change of size accepted; beyond this the marks were almost certainly mixed up.
pub const MAX_SCALE: f64 = 100.0;

#[derive(Debug, Error, PartialEq)]
pub enum RegisterError {
    #[error("marks must be finite numbers")]
    NonFinite,
    #[error("the two marks are too close together to give a direction")]
    TooClose,
    #[error("the marks imply a change of size of {0:.3}x, which is not believable")]
    BadScale(f64),
}

/// What the registration does, for showing to the person.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Registration {
    pub transform: Affine,
    /// Change of size (1 = none).
    pub scale: f64,
    /// Turn, degrees (y points down, so positive is clockwise on screen).
    pub rotation_deg: f64,
}

/// The similarity that maps `design[0]`, `design[1]` onto `actual[0]`, `actual[1]` as well as it
/// can: the error is shared between the two marks (their midpoints coincide). With `lock_scale`
/// the size is kept and only the turn and move are applied.
pub fn two_point(design: [Point; 2], actual: [Point; 2], lock_scale: bool) -> Result<Registration, RegisterError> {
    if !design.iter().chain(actual.iter()).all(|p| p.x.is_finite() && p.y.is_finite()) {
        return Err(RegisterError::NonFinite);
    }
    let (dv, av) = (design[1] - design[0], actual[1] - actual[0]);
    if dv.hypot() < MIN_SEPARATION_MM || av.hypot() < MIN_SEPARATION_MM {
        return Err(RegisterError::TooClose);
    }
    // Complex numbers: rotation and scale are the quotient of the two directions.
    let scale = av.hypot() / dv.hypot();
    if !(1.0 / MAX_SCALE..=MAX_SCALE).contains(&scale) {
        return Err(RegisterError::BadScale(scale));
    }
    let angle = av.atan2() - dv.atan2();
    let used = if lock_scale { 1.0 } else { scale };
    let (c, s) = (angle.cos() * used, angle.sin() * used);
    let linear = Affine::new([c, s, -s, c, 0.0, 0.0]);
    let (dm, am) = (design[0].midpoint(design[1]), actual[0].midpoint(actual[1]));
    let moved = linear * dm;
    let transform = Affine::translate((am.x - moved.x, am.y - moved.y)) * linear;
    Ok(Registration { transform, scale: used, rotation_deg: angle.to_degrees() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn a_pure_shift_moves_both_marks_exactly() {
        let r = two_point([p(10.0, 10.0), p(110.0, 10.0)], [p(13.0, 8.0), p(113.0, 8.0)], true).unwrap();
        assert!(close(r.transform * p(10.0, 10.0), p(13.0, 8.0)) && close(r.transform * p(110.0, 10.0), p(113.0, 8.0)));
        assert!((r.scale - 1.0).abs() < 1e-12 && r.rotation_deg.abs() < 1e-9);
    }

    #[test]
    fn a_turned_sheet_is_undone_by_the_measured_angle() {
        // The paper is rotated 90 degrees about the origin: (x, y) became (-y, x).
        let r = two_point([p(10.0, 0.0), p(30.0, 0.0)], [p(0.0, 10.0), p(0.0, 30.0)], true).unwrap();
        assert!((r.rotation_deg - 90.0).abs() < 1e-9);
        assert!(close(r.transform * p(20.0, 5.0), p(-5.0, 20.0)));
    }

    #[test]
    fn scale_is_applied_only_when_asked_for_and_error_is_shared_when_locked() {
        let design = [p(0.0, 0.0), p(100.0, 0.0)];
        let actual = [p(0.0, 0.0), p(101.0, 0.0)];
        let free = two_point(design, actual, false).unwrap();
        assert!((free.scale - 1.01).abs() < 1e-12);
        assert!(close(free.transform * design[1], actual[1]));
        let locked = two_point(design, actual, true).unwrap();
        assert_eq!(locked.scale, 1.0);
        let (a, b) = (locked.transform * design[0], locked.transform * design[1]);
        assert!((a.x - 0.5).abs() < 1e-9 && (b.x - 100.5).abs() < 1e-9, "half a millimetre short at each mark: {a:?} {b:?}");
    }

    #[test]
    fn unusable_marks_are_refused() {
        assert_eq!(two_point([p(0.0, 0.0), p(0.0, 0.0)], [p(0.0, 0.0), p(5.0, 0.0)], true), Err(RegisterError::TooClose));
        assert_eq!(two_point([p(0.0, 0.0), p(5.0, 0.0)], [p(1.0, 1.0), p(1.0, 1.0)], true), Err(RegisterError::TooClose));
        assert_eq!(two_point([p(f64::NAN, 0.0), p(5.0, 0.0)], [p(0.0, 0.0), p(5.0, 0.0)], true), Err(RegisterError::NonFinite));
        assert!(matches!(two_point([p(0.0, 0.0), p(1.0, 0.0)], [p(0.0, 0.0), p(500.0, 0.0)], false), Err(RegisterError::BadScale(_))));
    }
}
