//! Per-revision caches of derived data: flattened shape outlines for drawing and hit-testing,
//! and the planned toolpath for the preview.

use lightfoundry_doc::{LayerId, ShapeId};
use lightfoundry_engine::Session;
use lightfoundry_geom::{Point, Polyline, Rect, bounds_of_polylines};
use lightfoundry_toolpath::{Move, plan};

/// Preview segments drawn at most; huge raster jobs are truncated for display only.
pub const MAX_PREVIEW_SEGMENTS: usize = 400_000;

pub struct Item {
    pub id: ShapeId,
    pub layer: LayerId,
    pub lines: Vec<Polyline>,
    pub bounds: Rect,
    pub is_image: bool,
}

#[derive(Default)]
pub struct GeometryCache {
    revision: Option<u64>,
    pub items: Vec<Item>,
}

impl GeometryCache {
    pub fn refresh(&mut self, s: &Session) {
        if self.revision == Some(s.revision) {
            return;
        }
        self.revision = Some(s.revision);
        self.items = s
            .doc
            .shapes
            .iter()
            .filter_map(|sh| {
                let lines = sh.world_polylines();
                let bounds = bounds_of_polylines(&lines)?;
                Some(Item { id: sh.id, layer: sh.layer, lines, bounds, is_image: matches!(sh.kind, lightfoundry_doc::ShapeKind::Image { .. }) })
            })
            .collect();
    }

    /// Topmost shape whose outline is within `tol` mm of `p`, or whose closed outline contains it.
    pub fn pick(&self, p: Point, tol: f64) -> Option<ShapeId> {
        self.items.iter().rev().find(|it| it.lines.iter().any(|l| l.contains(p) || l.distance_to(p) <= tol)).map(|it| it.id)
    }
}

pub struct PreviewSegment {
    pub a: Point,
    pub b: Point,
    pub layer: LayerId,
    /// 0 for travel and zero-power moves.
    pub power: f32,
}

#[derive(Default)]
pub struct PreviewCache {
    revision: Option<u64>,
    pub segments: Vec<PreviewSegment>,
    pub error: Option<String>,
    pub truncated: bool,
}

impl PreviewCache {
    pub fn refresh(&mut self, s: &Session) {
        if self.revision == Some(s.revision) {
            return;
        }
        self.revision = Some(s.revision);
        self.segments.clear();
        self.error = None;
        self.truncated = false;
        match plan(&s.doc) {
            Err(e) => self.error = Some(e.to_string()),
            Ok(job) => {
                // The head's real start position is unknown, so the first move draws nothing.
                let mut at: Option<Point> = None;
                'ops: for op in &job.ops {
                    for m in &op.moves {
                        if self.segments.len() >= MAX_PREVIEW_SEGMENTS {
                            self.truncated = true;
                            break 'ops;
                        }
                        let power = match m {
                            Move::Travel(_) => 0.0,
                            Move::Cut { power, .. } => *power as f32,
                        };
                        if let Some(a) = at {
                            self.segments.push(PreviewSegment { a, b: m.target(), layer: op.layer, power });
                        }
                        at = Some(m.target());
                    }
                }
            }
        }
    }
}
