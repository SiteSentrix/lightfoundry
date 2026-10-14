use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{DynamicImage, ImageReader, Limits};
use lightfoundry_doc::{Document, LAYER_COLORS, LayerId, RasterImage, Shape, ShapeKind};
use lightfoundry_geom::Affine;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CommandSpec, parse, shapes::finite};
use crate::{EngineError, Session};

/// Largest decoded image we accept, in pixels.
const MAX_IMAGE_PIXELS: u64 = 50_000_000;
/// Default engraving resolution for imported images: 254 dpi = 0.1 mm per pixel.
const DEFAULT_MM_PER_PX: f64 = 0.1;

pub(super) const COMMANDS: &[CommandSpec] = &[
    CommandSpec { id: "file.new", label: "New", menu: "File", shortcut: Some("Ctrl+N"), params_doc: "{}", mutates: false, run: new },
    CommandSpec { id: "file.open", label: "Open…", menu: "File", shortcut: Some("Ctrl+O"), params_doc: "{path}", mutates: false, run: open },
    CommandSpec {
        id: "file.save",
        label: "Save",
        menu: "File",
        shortcut: Some("Ctrl+S"),
        params_doc: "{path?}  (default: the current file)",
        mutates: false,
        run: save,
    },
    CommandSpec {
        id: "file.import_svg",
        label: "Import SVG…",
        menu: "File/Import",
        shortcut: None,
        params_doc: "{path | data, x?, y?, layer_by_color?: true}",
        mutates: true,
        run: import_svg,
    },
    CommandSpec {
        id: "file.import_dxf",
        label: "Import DXF…",
        menu: "File/Import",
        shortcut: None,
        params_doc: "{path | data, x?, y?}",
        mutates: true,
        run: import_dxf,
    },
    CommandSpec {
        id: "file.import_image",
        label: "Import Image…",
        menu: "File/Import",
        shortcut: None,
        params_doc: "{path, x?, y?, width? (mm), layer?}",
        mutates: true,
        run: import_image,
    },
    CommandSpec {
        id: "file.autosave",
        label: "Autosave",
        menu: "",
        shortcut: None,
        params_doc: "{path}  — writes a recovery copy; does not change the document's path or unsaved state",
        mutates: false,
        run: autosave,
    },
    CommandSpec {
        id: "file.recover",
        label: "Recover",
        menu: "",
        shortcut: None,
        params_doc: "{path}  — opens a recovery copy as an unsaved, untitled document",
        mutates: false,
        run: recover,
    },
    CommandSpec {
        id: "file.export_gcode",
        label: "Save G-code…",
        menu: "File/Export",
        shortcut: None,
        params_doc: "{path}",
        mutates: false,
        run: export_gcode,
    },
];

fn new(s: &mut Session, _: &Value) -> Result<Value, EngineError> {
    let machine = s.doc.machine.clone();
    s.replace_document(Document { machine, ..Document::default() }, None);
    Ok(json!({}))
}

#[derive(Deserialize)]
struct PathParams {
    path: PathBuf,
}

fn open(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: PathParams = parse(params)?;
    let doc = lightfoundry_format::load(&p.path)?;
    let shapes = doc.shapes.len();
    s.replace_document(doc, Some(p.path));
    Ok(json!({ "shapes": shapes }))
}

#[derive(Deserialize, Default)]
struct OptionalPath {
    path: Option<PathBuf>,
}

fn save(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: OptionalPath = parse(params)?;
    let path =
        p.path.or_else(|| s.path.clone()).ok_or_else(|| EngineError::BadParams("no path given and the document has never been saved".into()))?;
    lightfoundry_format::save(&path, &s.doc)?;
    s.path = Some(path.clone());
    s.dirty = false;
    Ok(json!({ "path": path }))
}

#[derive(Deserialize)]
struct ImportParams {
    path: Option<PathBuf>,
    data: Option<String>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default = "yes")]
    layer_by_color: bool,
}

fn yes() -> bool {
    true
}

fn import_svg(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: ImportParams = parse(params)?;
    finite(&[p.x, p.y])?;
    let bytes = source_bytes(&p)?;
    let svg = lightfoundry_svg::import_svg(&bytes).map_err(|e| EngineError::Import(e.to_string()))?;
    let mut ids = Vec::new();
    for ip in svg.paths {
        let layer = if p.layer_by_color { ip.color.as_deref().map_or(0, layer_for_color) } else { 0 };
        let mut shape = Shape::path(ip.name, layer, ip.path);
        shape.transform = Affine::translate((p.x, p.y));
        ids.push(s.doc.add_shape(shape));
    }
    s.selection = ids.clone();
    Ok(json!({ "ids": ids, "width_mm": svg.width_mm, "height_mm": svg.height_mm }))
}

fn import_dxf(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: ImportParams = parse(params)?;
    finite(&[p.x, p.y])?;
    let bytes = source_bytes(&p)?;
    let dxf = lightfoundry_dxf::import_dxf(&bytes).map_err(|e| EngineError::Import(e.to_string()))?;
    // DXF layer names map to cut layers in order of first appearance.
    let mut names: Vec<String> = Vec::new();
    let mut ids = Vec::new();
    for dp in dxf.paths {
        let idx = names.iter().position(|n| *n == dp.layer).unwrap_or_else(|| {
            names.push(dp.layer.clone());
            names.len() - 1
        });
        let mut shape = Shape::path(dp.layer, (idx % LAYER_COLORS.len()) as LayerId, dp.path);
        shape.transform = Affine::translate((p.x, p.y));
        ids.push(s.doc.add_shape(shape));
    }
    s.selection = ids.clone();
    Ok(json!({ "ids": ids, "width_mm": dxf.width_mm, "height_mm": dxf.height_mm, "skipped": dxf.skipped }))
}

#[derive(Deserialize)]
struct ImageParams {
    path: PathBuf,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    width: Option<f64>,
    #[serde(default)]
    layer: LayerId,
}

fn import_image(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: ImageParams = parse(params)?;
    finite(&[p.x, p.y])?;
    let mut reader = ImageReader::open(&p.path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_IMAGE_PIXELS * 8);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| EngineError::Import(e.to_string()))?;
    let raster = to_gray(&decoded)?;
    let aspect = f64::from(raster.height) / f64::from(raster.width);
    let width_mm = match p.width {
        Some(w) if w.is_finite() && w > 0.0 && w < 1e5 => w,
        Some(_) => return Err(EngineError::BadParams("width must be positive".into())),
        None => f64::from(raster.width) * DEFAULT_MM_PER_PX,
    };
    let name = file_name(&p.path);
    let shape = Shape {
        id: 0,
        name,
        layer: p.layer,
        transform: Affine::translate((p.x, p.y)),
        kind: ShapeKind::Image { image: Arc::new(raster), width_mm, height_mm: width_mm * aspect, adjust: Default::default() },
    };
    let id = s.doc.add_shape(shape);
    s.selection = vec![id];
    Ok(json!({ "id": id, "width_mm": width_mm, "height_mm": width_mm * aspect }))
}

fn autosave(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: PathParams = parse(params)?;
    lightfoundry_format::save(&p.path, &s.doc)?;
    Ok(json!({ "path": p.path, "revision": s.revision }))
}

fn recover(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: PathParams = parse(params)?;
    let doc = lightfoundry_format::load(&p.path)?;
    let shapes = doc.shapes.len();
    s.replace_document(doc, None);
    // Recovered work was never saved: keep it marked as changed so closing asks first.
    s.dirty = true;
    Ok(json!({ "shapes": shapes }))
}

fn export_gcode(s: &mut Session, params: &Value) -> Result<Value, EngineError> {
    let p: PathParams = parse(params)?;
    let program = super::job::generate(s)?;
    std::fs::write(&p.path, &program)?;
    Ok(json!({ "path": p.path, "bytes": program.len() }))
}

fn source_bytes(p: &ImportParams) -> Result<Vec<u8>, EngineError> {
    match (&p.path, &p.data) {
        (Some(path), _) => Ok(std::fs::read(path)?),
        (None, Some(data)) => Ok(data.clone().into_bytes()),
        (None, None) => Err(EngineError::BadParams("give either path or data".into())),
    }
}

/// Exact colour match against the layer palette; anything else lands on layer 0.
fn layer_for_color(hex: &str) -> LayerId {
    LAYER_COLORS.iter().position(|c| c.eq_ignore_ascii_case(hex)).map_or(0, |i| i as LayerId)
}

fn to_gray(img: &DynamicImage) -> Result<RasterImage, EngineError> {
    let la = img.to_luma_alpha8();
    let (w, h) = la.dimensions();
    if u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
        return Err(EngineError::Import("image is too large".into()));
    }
    // Composite over white: transparent pixels must not burn.
    let gray = la.pixels().map(|px| ((u32::from(px[0]) * u32::from(px[1]) + 255 * (255 - u32::from(px[1]))) / 255) as u8).collect();
    RasterImage::new(w, h, gray).ok_or_else(|| EngineError::Import("empty image".into()))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into())
}
