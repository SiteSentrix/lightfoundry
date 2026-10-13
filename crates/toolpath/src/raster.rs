use lightfoundry_doc::{Dither, Document, Layer, RasterImage, Shape, ShapeKind};
use lightfoundry_geom::{Point, bounds_of_polylines};

use crate::{MAX_RASTER_CELLS, Move, PlanError};

/// Burn intensity per cell, 0 (skip) ..= 1 (full), for a `w`×`h` grid of 0..=255 gray samples.
pub fn dither(gray: &[f32], w: usize, h: usize, mode: Dither) -> Vec<f32> {
    let mut out = vec![0.0f32; gray.len()];
    match mode {
        Dither::Grayscale => {
            for (o, g) in out.iter_mut().zip(gray) {
                *o = 1.0 - g / 255.0;
            }
        }
        Dither::Threshold => {
            for (o, g) in out.iter_mut().zip(gray) {
                *o = if *g < 128.0 { 1.0 } else { 0.0 };
            }
        }
        Dither::FloydSteinberg => {
            let mut buf = gray.to_vec();
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    let old = buf.get(i).copied().unwrap_or(255.0);
                    let new = if old < 128.0 { 0.0 } else { 255.0 };
                    if let Some(o) = out.get_mut(i) {
                        *o = if new == 0.0 { 1.0 } else { 0.0 };
                    }
                    let err = old - new;
                    let mut spread = |dx: isize, dy: usize, k: f32| {
                        let nx = x as isize + dx;
                        if nx < 0 || nx as usize >= w || y + dy >= h {
                            return;
                        }
                        if let Some(c) = buf.get_mut((y + dy) * w + nx as usize) {
                            *c += err * k;
                        }
                    };
                    spread(1, 0, 7.0 / 16.0);
                    spread(-1, 1, 3.0 / 16.0);
                    spread(0, 1, 5.0 / 16.0);
                    spread(1, 1, 1.0 / 16.0);
                }
            }
        }
    }
    out
}

/// A world-space sampling grid: `nx`×`ny` cells of `pitch` mm with its top-left corner at `(x0, y0)`.
#[derive(Debug, Clone, Copy)]
pub struct Grid {
    pub x0: f64,
    pub y0: f64,
    pub nx: usize,
    pub ny: usize,
    pub pitch: f64,
}

/// Samples an image shape on `grid` (bilinear). Cells outside the image are white (255); a
/// non-image shape yields an all-white grid.
pub fn sample_grid(shape: &Shape, grid: &Grid) -> Vec<f32> {
    let mut out = vec![255.0f32; grid.nx.saturating_mul(grid.ny)];
    let ShapeKind::Image { image: img, width_mm: w_mm, height_mm: h_mm } = &shape.kind else { return out };
    let inv = shape.transform.inverse();
    for iy in 0..grid.ny {
        for ix in 0..grid.nx {
            let world = Point::new(grid.x0 + (ix as f64 + 0.5) * grid.pitch, grid.y0 + (iy as f64 + 0.5) * grid.pitch);
            let local = inv * world;
            if local.x < 0.0 || local.y < 0.0 || local.x >= *w_mm || local.y >= *h_mm {
                continue;
            }
            let fx = local.x / w_mm * f64::from(img.width) - 0.5;
            let fy = local.y / h_mm * f64::from(img.height) - 0.5;
            if let Some(c) = out.get_mut(iy * grid.nx + ix) {
                *c = bilinear(img, fx, fy);
            }
        }
    }
    out
}

fn bilinear(img: &RasterImage, fx: f64, fy: f64) -> f32 {
    let px = |x: i64, y: i64| -> f32 {
        let x = x.clamp(0, img.width as i64 - 1) as usize;
        let y = y.clamp(0, img.height as i64 - 1) as usize;
        img.gray.get(y * img.width as usize + x).copied().unwrap_or(255) as f32
    };
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let top = px(x0, y0) * (1.0 - tx) + px(x0 + 1, y0) * tx;
    let bottom = px(x0, y0 + 1) * (1.0 - tx) + px(x0 + 1, y0 + 1) * tx;
    top * (1.0 - ty) + bottom * ty
}

pub(crate) fn plan_image(doc: &Document, layer: &Layer, shape: &Shape) -> Result<Vec<Move>, PlanError> {
    if !matches!(shape.kind, ShapeKind::Image { .. }) {
        return Ok(Vec::new());
    }
    let Some(b) = bounds_of_polylines(&shape.world_polylines()) else { return Ok(Vec::new()) };
    let pitch = layer.interval;
    let nx = ((b.x1 - b.x0) / pitch).ceil();
    let ny = ((b.y1 - b.y0) / pitch).ceil();
    if !(nx.is_finite() && ny.is_finite()) || nx < 1.0 || ny < 1.0 {
        return Ok(Vec::new());
    }
    let cells = (nx as usize).saturating_mul(ny as usize);
    if cells > MAX_RASTER_CELLS {
        return Err(PlanError::RasterTooLarge(cells));
    }
    let (nx, ny) = (nx as usize, ny as usize);
    let gray = sample_grid(shape, &Grid { x0: b.x0, y0: b.y0, nx, ny, pitch });
    let intensity = dither(&gray, nx, ny, layer.dither);

    let over = if doc.machine.overscan.is_finite() { doc.machine.overscan.clamp(0.0, 50.0) } else { 0.0 };
    let (min_p, max_p) = (layer.min_power.clamp(0.0, 100.0), layer.max_power.clamp(0.0, 100.0));
    let x_at = |i: usize| b.x0 + i as f64 * pitch;
    let mut moves = Vec::new();
    for (iy, row) in intensity.chunks_exact(nx).enumerate() {
        let Some(first) = row.iter().position(|v| *v > 0.0) else { continue };
        let last = row.iter().rposition(|v| *v > 0.0).unwrap_or(first);
        let (from, to) = (first, last + 1);
        let y = b.y0 + (iy as f64 + 0.5) * pitch;
        let reverse = layer.bidirectional && iy % 2 == 1;
        let (start_x, end_x, dir) = if reverse { (x_at(to), x_at(from), -1.0) } else { (x_at(from), x_at(to), 1.0) };

        moves.push(Move::Travel(Point::new(start_x - dir * over, y)));
        if over > 0.0 {
            moves.push(Move::Cut { to: Point::new(start_x, y), power: 0.0 });
        }
        let order: Vec<usize> = if reverse { (from..to).rev().collect() } else { (from..to).collect() };
        let mut run_power: Option<f64> = None;
        for c in order {
            let v = row.get(c).copied().unwrap_or(0.0) as f64;
            // Quarter-percent steps keep equal-looking runs merged.
            let power = if v > 0.0 { ((min_p + (max_p - min_p) * v) * 4.0).round() / 4.0 } else { 0.0 };
            let end = if reverse { x_at(c) } else { x_at(c + 1) };
            match moves.last_mut() {
                Some(Move::Cut { to, power: p }) if run_power == Some(power) && *p == power => to.x = end,
                _ => moves.push(Move::Cut { to: Point::new(end, y), power }),
            }
            run_power = Some(power);
        }
        if over > 0.0 {
            moves.push(Move::Cut { to: Point::new(end_x + dir * over, y), power: 0.0 });
        }
    }
    Ok(moves)
}
