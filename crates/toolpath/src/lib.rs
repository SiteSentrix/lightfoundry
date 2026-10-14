//! Toolpath planning: turns a [`Document`] into a [`Job`] of machine-independent moves.
//!
//! Layering: knows geometry and the document, nothing about G-code dialects or devices.

mod fill;
mod order;
mod raster;

pub use fill::fill_segments;
pub use order::order_polylines;
pub use raster::{Grid, dither, sample_grid};

use lightfoundry_doc::{Document, Layer, LayerId, LayerMode, Shape, ShapeKind};
use lightfoundry_geom::{Point, Polyline, Rect, bounds_of_polylines};
use thiserror::Error;

/// Cap on raster cells per image, so a hostile document cannot exhaust memory.
pub const MAX_RASTER_CELLS: usize = 25_000_000;
/// Cap on total moves in a job.
pub const MAX_MOVES: usize = 20_000_000;

/// A planned job, in layer order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Job {
    pub ops: Vec<Op>,
}

/// All moves for one layer (or one image) and the settings they run with.
#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    pub layer: LayerId,
    pub kind: OpKind,
    /// mm/min
    pub speed: f64,
    pub passes: u32,
    pub air_assist: bool,
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Line,
    Fill,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Rapid, laser off.
    Travel(Point),
    /// Feed move; `power` is a percentage of full power (0 = laser off, used for overscan and gaps).
    Cut { to: Point, power: f64 },
}

impl Move {
    pub fn target(&self) -> Point {
        match self {
            Move::Travel(p) => *p,
            Move::Cut { to, .. } => *to,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum PlanError {
    #[error("layer {name}: {what} must be a positive finite number")]
    BadLayerSetting { name: String, what: &'static str },
    #[error("image raster would need {0} cells (limit {MAX_RASTER_CELLS}); increase the line interval")]
    RasterTooLarge(usize),
    #[error("job has more than {MAX_MOVES} moves")]
    TooManyMoves,
    #[error("layer {name}: kerf offset failed: {reason}")]
    Kerf { name: String, reason: String },
}

/// Plans every output-enabled layer. Layers are cut in id order; within a layer fills run before lines.
pub fn plan(doc: &Document) -> Result<Job, PlanError> {
    let mut job = Job::default();
    let mut total = 0usize;
    let mut at = Point::ORIGIN;
    for layer in doc.layers.iter().filter(|l| l.output) {
        validate(layer)?;
        let shapes: Vec<&Shape> = doc.shapes.iter().filter(|s| s.layer == layer.id).collect();
        if shapes.is_empty() {
            continue;
        }
        let mut ops = plan_layer(doc, layer, &shapes, at)?;
        for op in &mut ops {
            total = total.saturating_add(op.moves.len());
            if let Some(last) = op.moves.last() {
                at = last.target();
            }
        }
        if total > MAX_MOVES {
            return Err(PlanError::TooManyMoves);
        }
        job.ops.append(&mut ops);
    }
    Ok(job)
}

/// Offsets closed outlines by the layer's kerf compensation; open paths are left as they are.
fn apply_kerf(layer: &Layer, lines: &[Polyline]) -> Result<Vec<Polyline>, PlanError> {
    if layer.kerf_offset == 0.0 {
        return Ok(lines.to_vec());
    }
    let (closed, open): (Vec<&Polyline>, Vec<&Polyline>) = lines.iter().partition(|l| l.closed);
    let closed: Vec<Polyline> = closed.into_iter().cloned().collect();
    let mut out = lightfoundry_pathops::offset(&closed, layer.kerf_offset, lightfoundry_pathops::Join::Round)
        .map_err(|e| PlanError::Kerf { name: layer.name.clone(), reason: e.to_string() })?;
    out.extend(open.into_iter().cloned());
    Ok(out)
}

/// Bounds of everything that will be output (the "frame"), in workspace mm.
pub fn job_bounds(doc: &Document) -> Option<Rect> {
    let lines: Vec<Polyline> = doc.shapes.iter().filter(|s| doc.layer(s.layer).is_some_and(|l| l.output)).flat_map(Shape::world_polylines).collect();
    bounds_of_polylines(&lines)
}

/// Rough time estimate (no acceleration model).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Estimate {
    pub seconds: f64,
    pub cut_mm: f64,
    pub travel_mm: f64,
}

pub fn estimate(job: &Job, rapid_speed: f64) -> Estimate {
    let rapid = if rapid_speed.is_finite() && rapid_speed > 0.0 { rapid_speed } else { 6000.0 };
    let mut e = Estimate::default();
    for op in &job.ops {
        let mut at = Point::ORIGIN;
        for _ in 0..op.passes.max(1) {
            for m in &op.moves {
                let d = at.distance(m.target());
                match m {
                    Move::Travel(_) => {
                        e.travel_mm += d;
                        e.seconds += d / (rapid / 60.0);
                    }
                    Move::Cut { .. } => {
                        e.cut_mm += d;
                        e.seconds += d / (op.speed.max(1.0) / 60.0);
                    }
                }
                at = m.target();
            }
        }
    }
    e
}

fn validate(layer: &Layer) -> Result<(), PlanError> {
    let bad = |what| PlanError::BadLayerSetting { name: layer.name.clone(), what };
    if !(layer.speed.is_finite() && layer.speed > 0.0) {
        return Err(bad("speed"));
    }
    if !(layer.interval.is_finite() && layer.interval > 0.0) {
        return Err(bad("interval"));
    }
    if !(layer.max_power.is_finite() && layer.min_power.is_finite()) {
        return Err(bad("power"));
    }
    if !(layer.kerf_offset.is_finite() && layer.kerf_offset.abs() <= 50.0) {
        return Err(bad("kerf offset"));
    }
    Ok(())
}

fn plan_layer(doc: &Document, layer: &Layer, shapes: &[&Shape], start: Point) -> Result<Vec<Op>, PlanError> {
    let mut ops = Vec::new();
    let vector: Vec<Polyline> = shapes.iter().filter(|s| s.vector_path().is_some()).flat_map(|s| s.world_polylines()).collect();
    let outlines = apply_kerf(layer, &vector)?;
    let power = layer.max_power.clamp(0.0, 100.0);
    let passes = layer.passes.clamp(1, 1000);
    let mk = |kind, moves| Op { layer: layer.id, kind, speed: layer.speed, passes, air_assist: layer.air_assist, moves };

    if matches!(layer.mode, LayerMode::Fill | LayerMode::FillAndLine) {
        let segs = fill_segments(&vector, layer.interval, layer.scan_angle, layer.bidirectional);
        if !segs.is_empty() {
            let mut moves = Vec::with_capacity(segs.len() * 2);
            for (a, b) in segs {
                moves.push(Move::Travel(a));
                moves.push(Move::Cut { to: b, power });
            }
            ops.push(mk(OpKind::Fill, moves));
        }
    }
    if matches!(layer.mode, LayerMode::Line | LayerMode::FillAndLine) && !outlines.is_empty() {
        let mut moves = Vec::new();
        for pl in order_polylines(outlines, start) {
            let Some(first) = pl.points.first() else { continue };
            moves.push(Move::Travel(*first));
            for p in pl.points.iter().skip(1) {
                moves.push(Move::Cut { to: *p, power });
            }
            if pl.closed {
                moves.push(Move::Cut { to: *first, power });
            }
        }
        ops.push(mk(OpKind::Line, moves));
    }
    for shape in shapes {
        if let ShapeKind::Image { .. } = shape.kind {
            let moves = raster::plan_image(doc, layer, shape)?;
            if !moves.is_empty() {
                ops.push(mk(OpKind::Image, moves));
            }
        }
    }
    Ok(ops)
}

#[cfg(test)]
mod tests;
