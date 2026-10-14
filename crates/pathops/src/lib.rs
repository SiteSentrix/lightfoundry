//! Boolean combine and offset on flattened shapes (millimetres).
//!
//! A *region* is a list of closed polylines read with the even-odd rule, so a polyline nested
//! inside another is a hole. Heavy lifting is done by `i_overlay` (MIT OR Apache-2.0).

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, OutlineStyle, StrokeStyle};
use lightfoundry_geom::{Point, Polyline};
use thiserror::Error;

/// Points accepted per operation.
pub const MAX_POINTS: usize = 1_000_000;
/// Largest offset distance, mm.
pub const MAX_OFFSET_MM: f64 = 10_000.0;
/// Coordinates beyond this are refused (the integer clipper works on a bounded range).
pub const MAX_COORD_MM: f64 = 1.0e6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Intersect,
    /// Subject minus clip.
    Difference,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Join {
    /// Rounded corners.
    Round,
    /// Sharp corners (limited: very acute corners are bevelled).
    Miter,
    /// Corners cut off flat.
    Bevel,
}

#[derive(Debug, Error, PartialEq)]
pub enum PathOpsError {
    #[error("shapes contain a non-finite or out-of-range coordinate")]
    BadCoordinate,
    #[error("too many points (limit {MAX_POINTS})")]
    TooLarge,
    #[error("offset distance must be a finite number within ±{MAX_OFFSET_MM} mm")]
    BadDistance,
    #[error("an open path needs a positive offset distance (it becomes the outline around the line)")]
    OpenPathNeedsPositiveOffset,
}

/// Combines two regions.
pub fn boolean(subject: &[Polyline], clip: &[Polyline], op: BoolOp) -> Result<Vec<Polyline>, PathOpsError> {
    let (s, c) = (contours(subject)?, contours(clip)?);
    check_size(&[&s, &c])?;
    let rule = match op {
        BoolOp::Union => OverlayRule::Union,
        BoolOp::Intersect => OverlayRule::Intersect,
        BoolOp::Difference => OverlayRule::Difference,
        BoolOp::Xor => OverlayRule::Xor,
    };
    Ok(to_polylines(s.overlay(&c, rule, FillRule::EvenOdd)))
}

/// Merges a region with itself: resolves overlaps and self-intersections under the even-odd rule.
pub fn normalize(region: &[Polyline]) -> Result<Vec<Polyline>, PathOpsError> {
    boolean(region, &[], BoolOp::Union)
}

/// Grows (`distance > 0`) or shrinks (`distance < 0`) a region. Closed polylines are offset as
/// shapes (holes shrink when the outside grows); open polylines become the outline of a stroke
/// `2 × distance` wide and need a positive distance. Results are independent closed polylines.
pub fn offset(lines: &[Polyline], distance: f64, join: Join) -> Result<Vec<Polyline>, PathOpsError> {
    if !(distance.is_finite() && distance.abs() <= MAX_OFFSET_MM) {
        return Err(PathOpsError::BadDistance);
    }
    let closed: Vec<Polyline> = lines.iter().filter(|l| l.closed).cloned().collect();
    let open: Vec<&Polyline> = lines.iter().filter(|l| !l.closed).collect();
    if !open.is_empty() && distance <= 0.0 {
        return Err(PathOpsError::OpenPathNeedsPositiveOffset);
    }
    let mut out: Vec<Polyline> = Vec::new();

    if !closed.is_empty() && distance != 0.0 {
        let region = contours(&closed)?;
        check_size(&[&region])?;
        // Resolve overlaps first so the offset sees proper shapes with holes.
        let shapes = region.overlay(&Vec::<Vec<[f64; 2]>>::new(), OverlayRule::Union, FillRule::EvenOdd);
        let style = OutlineStyle::new(distance).line_join(join_style(join, distance));
        for shape in shapes {
            out.extend(to_polylines(shape.outline(&style)));
        }
    } else {
        out.extend(normalize(&closed)?);
    }

    for l in open {
        let pts = points(l)?;
        let style =
            StrokeStyle::new(distance * 2.0).line_join(join_style(join, distance)).start_cap(LineCap::Round(0.1)).end_cap(LineCap::Round(0.1));
        out.extend(to_polylines(pts.stroke(style, false)));
    }
    Ok(out)
}

fn join_style(join: Join, distance: f64) -> LineJoin<f64> {
    match join {
        Join::Round => LineJoin::Round(0.1_f64.min(0.5 / distance.abs().max(0.5))),
        Join::Miter => LineJoin::Miter(0.2),
        Join::Bevel => LineJoin::Bevel,
    }
}

fn contours(lines: &[Polyline]) -> Result<Vec<Vec<[f64; 2]>>, PathOpsError> {
    lines.iter().filter(|l| l.closed && l.points.len() >= 3).map(points).collect()
}

fn points(l: &Polyline) -> Result<Vec<[f64; 2]>, PathOpsError> {
    l.points
        .iter()
        .map(|p| {
            if p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD_MM && p.y.abs() <= MAX_COORD_MM {
                Ok([p.x, p.y])
            } else {
                Err(PathOpsError::BadCoordinate)
            }
        })
        .collect()
}

fn check_size(sets: &[&Vec<Vec<[f64; 2]>>]) -> Result<(), PathOpsError> {
    let n: usize = sets.iter().flat_map(|s| s.iter()).map(Vec::len).sum();
    if n > MAX_POINTS { Err(PathOpsError::TooLarge) } else { Ok(()) }
}

fn to_polylines(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Polyline> {
    shapes
        .into_iter()
        .flatten()
        .filter(|c| c.len() >= 3)
        .map(|c| Polyline::new(c.into_iter().map(|p| Point::new(p[0], p[1])).collect(), true))
        .collect()
}

#[cfg(test)]
mod tests;
