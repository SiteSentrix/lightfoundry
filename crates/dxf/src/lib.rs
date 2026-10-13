//! ASCII DXF import, written from the public DXF reference. Supports LINE, LWPOLYLINE (with bulge
//! arcs), POLYLINE/VERTEX, CIRCLE and ARC. Unsupported entities are reported, never fatal.

use std::collections::BTreeMap;

use kurbo::{Affine, Arc, Shape, SvgArc, Vec2};
use lightfoundry_geom::{BezPath, Point, bounds_of_polylines, ellipse_path, flatten};
use thiserror::Error;

pub const MAX_DXF_BYTES: usize = 100 * 1024 * 1024;
pub const MAX_ENTITIES: usize = 500_000;

#[derive(Debug, Error, PartialEq)]
pub enum DxfError {
    #[error("DXF is larger than {} MB", MAX_DXF_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("not an ASCII DXF file (binary DXF and DWG are not supported)")]
    NotAscii,
    #[error("the file has no ENTITIES section")]
    NoEntities,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DxfPath {
    pub layer: String,
    /// mm, y down, with the drawing's top-left corner moved to the origin.
    pub path: BezPath,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DxfImport {
    pub paths: Vec<DxfPath>,
    pub width_mm: f64,
    pub height_mm: f64,
    /// Entity types that were present but not imported, with counts.
    pub skipped: BTreeMap<String, usize>,
}

pub fn import_dxf(data: &[u8]) -> Result<DxfImport, DxfError> {
    if data.len() > MAX_DXF_BYTES {
        return Err(DxfError::TooLarge);
    }
    if data.starts_with(b"AutoCAD Binary DXF") || data.contains(&0) {
        return Err(DxfError::NotAscii);
    }
    let text = String::from_utf8_lossy(data);
    let pairs = pairs(&text);
    let scale = unit_scale(&pairs);
    let start = pairs.iter().position(|(c, v)| *c == 2 && v == "ENTITIES").ok_or(DxfError::NoEntities)?;

    let mut entities: Vec<Entity> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    let mut current: Option<Entity> = None;
    for (code, value) in pairs.iter().skip(start + 1) {
        if *code == 0 {
            if let Some(e) = current.take() {
                entities.push(e);
            }
            if value == "ENDSEC" || entities.len() >= MAX_ENTITIES {
                break;
            }
            current = Some(Entity { kind: value.clone(), data: Vec::new() });
            continue;
        }
        if let Some(e) = current.as_mut() {
            e.data.push((*code, value.clone()));
        }
    }

    let mut paths = Vec::new();
    let mut i = 0;
    while let Some(e) = entities.get(i) {
        i += 1;
        let layer = e.text(8).unwrap_or("0").to_string();
        let built = match e.kind.as_str() {
            "LINE" => build_line(e),
            "LWPOLYLINE" => build_lwpolyline(e),
            "CIRCLE" => build_circle(e),
            "ARC" => build_arc(e),
            "POLYLINE" => {
                let closed = e.int(70).unwrap_or(0) & 1 == 1;
                let mut verts = Vec::new();
                while let Some(v) = entities.get(i) {
                    i += 1;
                    if v.kind == "SEQEND" {
                        break;
                    }
                    if v.kind == "VERTEX"
                        && let (Some(x), Some(y)) = (v.num(10), v.num(20))
                    {
                        verts.push((x, y, v.num(42).unwrap_or(0.0)));
                    }
                }
                polyline_from_vertices(&verts, closed)
            }
            "VERTEX" | "SEQEND" | "ENDBLK" => None,
            other => {
                *skipped.entry(other.to_string()).or_default() += 1;
                None
            }
        };
        if let Some(path) = built {
            paths.push(DxfPath { layer, path });
        }
    }

    // DXF is y-up in drawing units: scale to mm, flip to y-down, move the top-left to the origin.
    let flip = Affine::scale_non_uniform(scale, -scale);
    for p in &mut paths {
        p.path.apply_affine(flip);
    }
    let all: Vec<_> = paths.iter().flat_map(|p| flatten(&p.path, 0.05)).collect();
    let (width_mm, height_mm) = match bounds_of_polylines(&all) {
        Some(b) => {
            let shift = Affine::translate((-b.x0, -b.y0));
            for p in &mut paths {
                p.path.apply_affine(shift);
            }
            (b.width(), b.height())
        }
        None => (0.0, 0.0),
    };
    Ok(DxfImport { paths, width_mm, height_mm, skipped })
}

struct Entity {
    kind: String,
    data: Vec<(i32, String)>,
}

impl Entity {
    fn text(&self, code: i32) -> Option<&str> {
        self.data.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }

    fn num(&self, code: i32) -> Option<f64> {
        self.text(code).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }

    fn int(&self, code: i32) -> Option<i64> {
        self.text(code).and_then(|v| v.trim().parse::<i64>().ok())
    }

    fn nums(&self, code: i32) -> impl Iterator<Item = f64> + '_ {
        self.data.iter().filter(move |(c, _)| *c == code).filter_map(|(_, v)| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
}

fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        if let Ok(code) = code.trim().parse::<i32>() {
            out.push((code, value.trim().to_string()));
        }
    }
    out
}

/// `$INSUNITS` → mm per drawing unit. Unitless (0) is treated as mm.
fn unit_scale(pairs: &[(i32, String)]) -> f64 {
    let units = pairs
        .iter()
        .position(|(c, v)| *c == 9 && v == "$INSUNITS")
        .and_then(|i| pairs.get(i + 1))
        .and_then(|(_, v)| v.parse::<i32>().ok())
        .unwrap_or(0);
    match units {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1_000_000.0,
        8 => 25.4e-6,
        9 => 0.0254,
        10 => 914.4,
        _ => 1.0,
    }
}

fn build_line(e: &Entity) -> Option<BezPath> {
    let mut p = BezPath::new();
    p.move_to((e.num(10)?, e.num(20)?));
    p.line_to((e.num(11)?, e.num(21)?));
    Some(p)
}

fn build_circle(e: &Entity) -> Option<BezPath> {
    let r = e.num(40)?;
    if r <= 0.0 {
        return None;
    }
    Some(ellipse_path(e.num(10)?, e.num(20)?, r, r))
}

fn build_arc(e: &Entity) -> Option<BezPath> {
    let r = e.num(40)?;
    if r <= 0.0 {
        return None;
    }
    let start = e.num(50)?.to_radians();
    let mut sweep = (e.num(51)?.to_radians() - start).rem_euclid(std::f64::consts::TAU);
    if sweep == 0.0 {
        sweep = std::f64::consts::TAU;
    }
    let arc = Arc::new((e.num(10)?, e.num(20)?), Vec2::new(r, r), start, sweep, 0.0);
    Some(arc.to_path(0.05))
}

fn build_lwpolyline(e: &Entity) -> Option<BezPath> {
    let closed = e.int(70).unwrap_or(0) & 1 == 1;
    let xs: Vec<f64> = e.nums(10).collect();
    let ys: Vec<f64> = e.nums(20).collect();
    // Bulges (code 42) are per-vertex but only present when non-zero, so rebuild by vertex order.
    let mut bulges = vec![0.0; xs.len().min(ys.len())];
    let mut vertex: isize = -1;
    for (code, v) in &e.data {
        match code {
            10 => vertex += 1,
            42 => {
                if let (Ok(b), Some(slot)) = (v.trim().parse::<f64>(), usize::try_from(vertex).ok().and_then(|i| bulges.get_mut(i))) {
                    *slot = b;
                }
            }
            _ => {}
        }
    }
    let verts: Vec<(f64, f64, f64)> = xs.iter().zip(&ys).zip(&bulges).map(|((x, y), b)| (*x, *y, *b)).collect();
    polyline_from_vertices(&verts, closed)
}

fn polyline_from_vertices(verts: &[(f64, f64, f64)], closed: bool) -> Option<BezPath> {
    if verts.len() < 2 {
        return None;
    }
    let mut path = BezPath::new();
    let (x0, y0, _) = *verts.first()?;
    path.move_to((x0, y0));
    let segments = if closed { verts.len() } else { verts.len() - 1 };
    for i in 0..segments {
        let (ax, ay, bulge) = *verts.get(i)?;
        let (bx, by, _) = *verts.get((i + 1) % verts.len())?;
        append_segment(&mut path, Point::new(ax, ay), Point::new(bx, by), bulge);
    }
    if closed {
        path.close_path();
    }
    Some(path)
}

fn append_segment(path: &mut BezPath, a: Point, b: Point, bulge: f64) {
    let chord = a.distance(b);
    if bulge.abs() < 1e-9 || chord < 1e-12 {
        path.line_to(b);
        return;
    }
    let radius = chord * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let svg = SvgArc { from: a, to: b, radii: Vec2::new(radius, radius), x_rotation: 0.0, large_arc: bulge.abs() > 1.0, sweep: bulge > 0.0 };
    match Arc::from_svg_arc(&svg) {
        Some(arc) => {
            for el in arc.append_iter(0.05) {
                path.push(el);
            }
        }
        None => path.line_to(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(entities: &str, units: i32) -> String {
        format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{units}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    fn bounds(imp: &DxfImport) -> kurbo::Rect {
        bounds_of_polylines(&imp.paths.iter().flat_map(|p| flatten(&p.path, 0.01)).collect::<Vec<_>>()).unwrap()
    }

    mod entity_tests {
        use super::*;

        #[test]
        fn line_and_circle_import_in_mm() {
            let src = dxf("0\nLINE\n8\nCUT\n10\n0\n20\n0\n11\n30\n21\n10\n0\nCIRCLE\n10\n50\n20\n5\n40\n5\n", 4);
            let imp = import_dxf(src.as_bytes()).unwrap();
            assert_eq!(imp.paths.len(), 2);
            assert_eq!(imp.paths[0].layer, "CUT");
            assert!((imp.width_mm - 55.0).abs() < 0.01, "{}", imp.width_mm);
            assert!((imp.height_mm - 10.0).abs() < 0.01, "{}", imp.height_mm);
        }

        #[test]
        fn inches_are_scaled_to_mm() {
            let src = dxf("0\nLINE\n10\n0\n20\n0\n11\n2\n21\n0\n", 1);
            assert!((import_dxf(src.as_bytes()).unwrap().width_mm - 50.8).abs() < 1e-9);
        }

        #[test]
        fn closed_lwpolyline_with_a_bulge_makes_a_half_circle_cap() {
            // Square 10x10 whose top edge bulges into a semicircle (bulge 1 = 180 degrees).
            let src = dxf("0\nLWPOLYLINE\n70\n1\n90\n4\n10\n0\n20\n0\n10\n10\n20\n0\n10\n10\n20\n10\n42\n1\n10\n0\n20\n10\n", 4);
            let imp = import_dxf(src.as_bytes()).unwrap();
            assert!((imp.height_mm - 15.0).abs() < 0.05, "{}", imp.height_mm);
        }

        #[test]
        fn arc_spans_the_requested_angles() {
            let src = dxf("0\nARC\n10\n0\n20\n0\n40\n10\n50\n0\n51\n90\n", 4);
            let b = bounds(&import_dxf(src.as_bytes()).unwrap());
            assert!((b.width() - 10.0).abs() < 0.05 && (b.height() - 10.0).abs() < 0.05, "{b:?}");
        }

        #[test]
        fn old_style_polyline_is_supported() {
            let src = dxf("0\nPOLYLINE\n70\n0\n0\nVERTEX\n10\n0\n20\n0\n0\nVERTEX\n10\n10\n20\n5\n0\nSEQEND\n", 4);
            assert_eq!(import_dxf(src.as_bytes()).unwrap().paths.len(), 1);
        }

        #[test]
        fn unsupported_entities_are_reported_not_fatal() {
            let src = dxf("0\nSPLINE\n8\n0\n0\nTEXT\n1\nhi\n", 4);
            let imp = import_dxf(src.as_bytes()).unwrap();
            assert!(imp.paths.is_empty());
            assert_eq!(imp.skipped.get("SPLINE"), Some(&1));
            assert_eq!(imp.skipped.get("TEXT"), Some(&1));
        }
    }

    mod hostile_tests {
        use super::*;

        #[test]
        fn garbage_does_not_panic() {
            assert_eq!(import_dxf(b"hello"), Err(DxfError::NoEntities));
            assert_eq!(import_dxf(b"\0\0\0"), Err(DxfError::NotAscii));
            assert!(import_dxf(b"AutoCAD Binary DXF\r\n\x1a\0").is_err());
        }

        #[test]
        fn nan_and_missing_fields_are_skipped() {
            let src = dxf("0\nLINE\n10\nNaN\n20\n0\n11\n1\n21\n1\n0\nCIRCLE\n10\n0\n", 4);
            assert!(import_dxf(src.as_bytes()).unwrap().paths.is_empty());
        }

        #[test]
        fn zero_radius_circle_is_skipped() {
            let src = dxf("0\nCIRCLE\n10\n0\n20\n0\n40\n0\n", 4);
            assert!(import_dxf(src.as_bytes()).unwrap().paths.is_empty());
        }
    }
}
