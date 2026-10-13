//! The LightFoundry document model: cut layers, shapes, images and the machine profile.
//! Pure data + serde; no I/O, no UI.

use std::sync::Arc;

use lightfoundry_geom::{Affine, BezPath, Polyline, Rect, bounds_of_polylines, flatten};
use serde::{Deserialize, Serialize};

pub const LAYER_COLORS: [&str; 16] = [
    "#000000", "#0000ff", "#ff0000", "#00e000", "#d0d000", "#ff8000", "#00e0e0", "#ff00ff", "#b4b4b4", "#0000a0", "#a00000", "#00a000", "#a0a000",
    "#c08000", "#00a0ff", "#a000a0",
];

/// Flattening tolerance (mm) used for planning and previews.
pub const FLATTEN_TOLERANCE_MM: f64 = 0.02;

pub type ShapeId = u64;
pub type LayerId = u32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub layers: Vec<Layer>,
    pub shapes: Vec<Shape>,
    pub machine: Machine,
    pub next_id: ShapeId,
}

impl Default for Document {
    fn default() -> Self {
        Self { layers: (0..LAYER_COLORS.len() as u32).map(Layer::numbered).collect(), shapes: Vec::new(), machine: Machine::default(), next_id: 1 }
    }
}

impl Document {
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn shape(&self, id: ShapeId) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    pub fn shape_mut(&mut self, id: ShapeId) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }

    /// Adds a shape, assigning it a fresh id. Unknown layers fall back to layer 0.
    pub fn add_shape(&mut self, mut shape: Shape) -> ShapeId {
        shape.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        if self.layer(shape.layer).is_none() {
            shape.layer = 0;
        }
        let id = shape.id;
        self.shapes.push(shape);
        id
    }

    pub fn remove_shapes(&mut self, ids: &[ShapeId]) {
        self.shapes.retain(|s| !ids.contains(&s.id));
    }

    /// Bounds of the given shapes in workspace mm.
    pub fn bounds_of(&self, ids: &[ShapeId]) -> Option<Rect> {
        ids.iter().filter_map(|id| self.shape(*id)).filter_map(Shape::world_bounds).reduce(|a, b| a.union(b))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub color: String,
    pub mode: LayerMode,
    /// mm/min
    pub speed: f64,
    /// Percent of the machine's S range, 0..=100.
    pub min_power: f64,
    pub max_power: f64,
    pub passes: u32,
    /// Fill line spacing in mm.
    pub interval: f64,
    /// Fill scan angle in degrees.
    pub scan_angle: f64,
    pub bidirectional: bool,
    pub dither: Dither,
    pub output: bool,
    pub air_assist: bool,
}

impl Layer {
    pub fn numbered(id: LayerId) -> Layer {
        Layer {
            id,
            name: format!("C{id:02}"),
            color: LAYER_COLORS.get(id as usize % LAYER_COLORS.len()).copied().unwrap_or("#000000").to_string(),
            mode: LayerMode::Line,
            speed: 1000.0,
            min_power: 10.0,
            max_power: 60.0,
            passes: 1,
            interval: 0.1,
            scan_angle: 0.0,
            bidirectional: true,
            dither: Dither::FloydSteinberg,
            output: true,
            air_assist: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    Line,
    Fill,
    FillAndLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dither {
    Threshold,
    FloydSteinberg,
    Grayscale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub id: ShapeId,
    pub name: String,
    pub layer: LayerId,
    /// Maps local geometry to workspace mm (y down, origin top-left of the bed).
    pub transform: Affine,
    pub kind: ShapeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShapeKind {
    Path {
        path: BezPath,
    },
    /// Raster occupying local `0..width_mm × 0..height_mm`.
    Image {
        image: Arc<RasterImage>,
        width_mm: f64,
        height_mm: f64,
    },
}

impl Shape {
    pub fn path(name: impl Into<String>, layer: LayerId, path: BezPath) -> Shape {
        Shape { id: 0, name: name.into(), layer, transform: Affine::IDENTITY, kind: ShapeKind::Path { path } }
    }

    /// Flattened outlines in workspace mm. Images yield their bounding rectangle.
    pub fn world_polylines(&self) -> Vec<Polyline> {
        match &self.kind {
            ShapeKind::Path { path } => {
                let mut p = path.clone();
                p.apply_affine(self.transform);
                flatten(&p, FLATTEN_TOLERANCE_MM)
            }
            ShapeKind::Image { width_mm, height_mm, .. } => {
                let p = lightfoundry_geom::rect_path(0.0, 0.0, *width_mm, *height_mm);
                let mut p = p;
                p.apply_affine(self.transform);
                flatten(&p, FLATTEN_TOLERANCE_MM)
            }
        }
    }

    pub fn world_bounds(&self) -> Option<Rect> {
        bounds_of_polylines(&self.world_polylines())
    }
}

/// 8-bit grayscale raster, 255 = white (no burn).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    #[serde(with = "base64_bytes")]
    pub gray: Vec<u8>,
}

impl RasterImage {
    /// Returns `None` when the buffer length does not match the dimensions.
    pub fn new(width: u32, height: u32, gray: Vec<u8>) -> Option<RasterImage> {
        let expected = (width as usize).checked_mul(height as usize)?;
        (expected == gray.len() && expected > 0).then_some(RasterImage { width, height, gray })
    }
}

/// Serde adapter storing byte buffers as standard base64 strings.
mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in text.bytes().filter(|c| *c != b'=') {
            let v = ALPHABET.iter().position(|a| *a == c).ok_or_else(|| D::Error::custom("invalid base64"))? as u32;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((acc >> bits) & 0xff) as u8);
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    pub bed_width: f64,
    pub bed_height: f64,
    pub origin: Origin,
    /// GRBL `$30` maximum spindle speed.
    pub max_s: u32,
    /// `M4` dynamic power (laser mode) instead of `M3`.
    pub dynamic_power: bool,
    /// mm/min used for travel-time estimates (G0).
    pub rapid_speed: f64,
    /// Extra lead-in/out for raster rows in mm so the head is at speed.
    pub overscan: f64,
}

impl Default for Machine {
    fn default() -> Self {
        Machine {
            name: "Generic GRBL 400x400".into(),
            bed_width: 400.0,
            bed_height: 400.0,
            origin: Origin::BottomLeft,
            max_s: 1000,
            dynamic_power: true,
            rapid_speed: 6000.0,
            overscan: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    BottomLeft,
    TopLeft,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightfoundry_geom::rect_path;

    #[test]
    fn add_shape_assigns_ids_and_falls_back_to_layer_zero() {
        let mut doc = Document::default();
        let a = doc.add_shape(Shape::path("a", 99, rect_path(0.0, 0.0, 1.0, 1.0)));
        let b = doc.add_shape(Shape::path("b", 2, rect_path(0.0, 0.0, 1.0, 1.0)));
        assert_ne!(a, b);
        assert_eq!(doc.shape(a).map(|s| s.layer), Some(0));
        assert_eq!(doc.shape(b).map(|s| s.layer), Some(2));
    }

    #[test]
    fn world_bounds_apply_transform() {
        let mut s = Shape::path("r", 0, rect_path(0.0, 0.0, 10.0, 10.0));
        s.transform = Affine::translate((5.0, 7.0));
        let b = s.world_bounds();
        assert_eq!(b, Some(Rect::new(5.0, 7.0, 15.0, 17.0)));
    }

    #[test]
    fn raster_rejects_mismatched_buffers() {
        assert!(RasterImage::new(2, 2, vec![0; 3]).is_none());
        assert!(RasterImage::new(0, 0, vec![]).is_none());
        assert!(RasterImage::new(2, 2, vec![0; 4]).is_some());
    }

    #[test]
    fn document_serde_round_trip_including_images() {
        let mut doc = Document::default();
        doc.add_shape(Shape::path("r", 1, rect_path(0.0, 0.0, 3.0, 4.0)));
        for len in 1..=7u8 {
            let image = Arc::new(RasterImage::new(u32::from(len), 1, (0..len).map(|i| i * 37).collect()).unwrap());
            doc.add_shape(Shape {
                id: 0,
                name: "i".into(),
                layer: 0,
                transform: Affine::IDENTITY,
                kind: ShapeKind::Image { image, width_mm: 1.0, height_mm: 1.0 },
            });
        }
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
    }
}
