//! Reading the Ink/Stitch font collection (https://github.com/inkstitch/embroidery-fonts, with its own
//! licence per font): a folder with `font.json` and `ltr.svg`, an Inkscape drawing with one layer per
//! glyph labelled `GlyphLayer-<character>`. The drawings hold embroidery structure, which is read as
//! shapes: a satin column (a path whose first two subpaths are its rails) becomes its outline, filled areas
//! stay closed shapes, and running-stitch lines stay open lines. Everything else in the file (stitch
//! settings, guides, symbols) is ignored.
//!
//! The file comes from outside, so size, node count, nesting, path counts and every number are bounded.

use kurbo::{Affine, BezPath, ParamCurve, PathEl, PathSeg, Shape};
use roxmltree::{Document, Node, ParsingOptions};

use crate::TextError;

/// Largest drawing read, in bytes.
pub const MAX_SVG_BYTES: usize = 24 * 1024 * 1024;
/// Most XML nodes accepted.
const MAX_NODES: u32 = 3_000_000;
/// Deepest group nesting walked.
const MAX_DEPTH: usize = 64;
/// Most glyphs, and most paths in one glyph.
const MAX_GLYPHS: usize = 2_000;
const MAX_PATHS_PER_GLYPH: usize = 4_000;
/// Most path elements read in the whole file.
const MAX_PATHS: usize = 200_000;
/// Longest path data read, in bytes.
const MAX_D_BYTES: usize = 2 * 1024 * 1024;
const LAYER_PREFIX: &str = "GlyphLayer-";

/// A glyph as drawn: its shapes in the drawing's own units (y down, no baseline applied yet).
pub(crate) struct DrawnGlyph {
    pub ch: char,
    pub path: BezPath,
}

/// Every glyph layer of the drawing.
pub(crate) fn read_glyphs(svg: &str) -> Result<Vec<DrawnGlyph>, TextError> {
    if svg.len() > MAX_SVG_BYTES {
        return Err(TextError::BadFont("the font drawing is too large".into()));
    }
    let options = ParsingOptions { allow_dtd: false, nodes_limit: MAX_NODES, ..ParsingOptions::default() };
    let doc = Document::parse_with_options(svg, options).map_err(|e| TextError::BadFont(format!("the font drawing: {e}")))?;
    let mut glyphs = Vec::new();
    let mut paths_seen = 0usize;
    visit(doc.root_element(), Affine::IDENTITY, 0, &mut glyphs, &mut paths_seen)?;
    if glyphs.is_empty() {
        return Err(TextError::BadFont("the drawing has no glyph layers".into()));
    }
    Ok(glyphs)
}

fn visit(node: Node, parent: Affine, depth: usize, out: &mut Vec<DrawnGlyph>, paths_seen: &mut usize) -> Result<(), TextError> {
    if depth > MAX_DEPTH {
        return Err(TextError::BadFont("the font drawing is nested too deeply".into()));
    }
    for child in node.children().filter(Node::is_element) {
        let name = child.tag_name().name();
        if matches!(name, "defs" | "symbol" | "metadata" | "namedview" | "clipPath" | "mask" | "title" | "desc") {
            continue;
        }
        let transform = parent * transform_of(child);
        if name == "g"
            && let Some(ch) = layer_char(child)
        {
            if out.len() >= MAX_GLYPHS {
                return Err(TextError::BadFont(format!("more than {MAX_GLYPHS} glyphs")));
            }
            let mut path = BezPath::new();
            let mut count = 0usize;
            collect_paths(child, transform, 0, &mut path, &mut count, paths_seen)?;
            if !path.elements().is_empty() {
                out.push(DrawnGlyph { ch, path });
            }
        } else if name == "g" {
            visit(child, transform, depth + 1, out, paths_seen)?;
        }
    }
    Ok(())
}

/// The character of a glyph layer: `inkscape:label="GlyphLayer-A"` is `A` (exactly one character).
fn layer_char(node: Node) -> Option<char> {
    let label = node.attributes().find(|a| a.name() == "label")?.value();
    let mut chars = label.strip_prefix(LAYER_PREFIX)?.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

fn collect_paths(
    node: Node,
    transform: Affine,
    depth: usize,
    glyph: &mut BezPath,
    count: &mut usize,
    paths_seen: &mut usize,
) -> Result<(), TextError> {
    if depth > MAX_DEPTH {
        return Err(TextError::BadFont("the font drawing is nested too deeply".into()));
    }
    for child in node.children().filter(Node::is_element) {
        let name = child.tag_name().name();
        let here = transform * transform_of(child);
        match name {
            "g" => collect_paths(child, here, depth + 1, glyph, count, paths_seen)?,
            "path" => {
                *paths_seen += 1;
                *count += 1;
                if *paths_seen > MAX_PATHS || *count > MAX_PATHS_PER_GLYPH {
                    return Err(TextError::BadFont("the font drawing has too many paths".into()));
                }
                if hidden(child) {
                    continue;
                }
                let Some(d) = child.attribute("d").filter(|d| d.len() <= MAX_D_BYTES) else { continue };
                let Ok(path) = BezPath::from_svg(d) else { continue };
                let satin = child.attributes().any(|a| a.name() == "satin_column" && a.value().eq_ignore_ascii_case("true"));
                let mut shape = if satin { satin_outline(&path) } else { path };
                shape.apply_affine(here);
                if finite(&shape) {
                    glyph.extend(shape);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn hidden(node: Node) -> bool {
    node.attribute("style").is_some_and(|s| s.replace(' ', "").contains("display:none"))
}

fn finite(path: &BezPath) -> bool {
    path.elements().iter().all(|el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7,
        PathEl::QuadTo(a, b) => [a, b].iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7),
        PathEl::CurveTo(a, b, c) => [a, b, c].iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.x.abs() < 1e7 && p.y.abs() < 1e7),
        PathEl::ClosePath => true,
    })
}

/// A satin column's outline: its first rail, then the second rail backwards. A path with fewer than two
/// subpaths is returned as it is.
fn satin_outline(path: &BezPath) -> BezPath {
    let mut subpaths: Vec<Vec<PathSeg>> = Vec::new();
    let mut current: Vec<PathSeg> = Vec::new();
    let mut started = false;
    for seg in path.segments() {
        // `segments()` starts a new run wherever the path jumps (a new MoveTo): detect it by a gap.
        if let Some(prev) = current.last()
            && prev.end().distance(seg.start()) > 1e-9
        {
            subpaths.push(std::mem::take(&mut current));
        }
        current.push(seg);
        started = true;
    }
    if started {
        subpaths.push(current);
    }
    let (Some(first), Some(second)) = (subpaths.first(), subpaths.get(1)) else { return path.clone() };
    let (Some(a_start), Some(b_end)) = (first.first().map(PathSeg::start), second.last().map(PathSeg::end)) else {
        return path.clone();
    };
    let mut out = BezPath::new();
    out.move_to(a_start);
    for seg in first {
        push_seg(&mut out, *seg);
    }
    out.line_to(b_end);
    for seg in second.iter().rev() {
        push_seg(&mut out, seg.reverse());
    }
    out.close_path();
    out
}

fn push_seg(path: &mut BezPath, seg: PathSeg) {
    match seg {
        PathSeg::Line(l) => path.line_to(l.p1),
        PathSeg::Quad(q) => path.quad_to(q.p1, q.p2),
        PathSeg::Cubic(c) => path.curve_to(c.p1, c.p2, c.p3),
    }
}

/// The element's `transform` attribute: matrix, translate, scale, rotate, skewX and skewY, in order.
/// Anything unreadable counts as no transform.
fn transform_of(node: Node) -> Affine {
    let Some(text) = node.attribute("transform").filter(|t| t.len() <= 1024) else { return Affine::IDENTITY };
    parse_transform(text).unwrap_or(Affine::IDENTITY)
}

fn parse_transform(text: &str) -> Option<Affine> {
    let mut total = Affine::IDENTITY;
    let mut rest = text.trim();
    let mut steps = 0;
    while !rest.is_empty() {
        steps += 1;
        if steps > 32 {
            return None;
        }
        let open = rest.find('(')?;
        let close = rest.find(')')?;
        if close < open {
            return None;
        }
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let args: Vec<f64> =
            rest[open + 1..close].split([',', ' ', '\t', '\n', '\r']).filter(|s| !s.is_empty()).map(|s| s.parse().ok()).collect::<Option<_>>()?;
        if args.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let step = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => Affine::new([*a, *b, *c, *d, *e, *f]),
            ("translate", [x]) => Affine::translate((*x, 0.0)),
            ("translate", [x, y]) => Affine::translate((*x, *y)),
            ("scale", [s]) => Affine::scale(*s),
            ("scale", [x, y]) => Affine::scale_non_uniform(*x, *y),
            ("rotate", [a]) => Affine::rotate(a.to_radians()),
            ("rotate", [a, cx, cy]) => Affine::translate((*cx, *cy)) * Affine::rotate(a.to_radians()) * Affine::translate((-*cx, -*cy)),
            ("skewX", [a]) => Affine::new([1.0, 0.0, a.to_radians().tan(), 1.0, 0.0, 0.0]),
            ("skewY", [a]) => Affine::new([1.0, a.to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            _ => return None,
        };
        total *= step;
        rest = rest[close + 1..].trim_start_matches([',', ' ', '\t', '\n', '\r']);
    }
    let c = total.as_coeffs();
    c.iter().all(|v| v.is_finite()).then_some(total)
}

/// The tallest-looking baseline and capital height of a set of drawn glyphs, from the letters that have
/// flat bottoms (so no guide conventions are needed): the drawing's units, y down.
pub(crate) fn baseline_and_cap(glyphs: &[DrawnGlyph]) -> Option<(f64, f64)> {
    let measure = |g: &DrawnGlyph| {
        let b = g.path.bounding_box();
        (b.y1, b.y1 - b.y0)
    };
    let capital = ['H', 'E', 'I', 'L', 'T', 'Z', 'N', 'M', 'X', 'F']
        .iter()
        .find_map(|c| glyphs.iter().find(|g| g.ch == *c))
        .or_else(|| glyphs.iter().find(|g| g.ch.is_uppercase()))
        .or_else(|| glyphs.first())?;
    let (baseline, cap) = measure(capital);
    (baseline.is_finite() && cap.is_finite() && cap > 0.0).then_some((baseline, cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod read_tests {
        use super::*;

        fn drawing(layers: &str) -> String {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" xmlns:inkstitch="http://inkstitch.org/namespace" viewBox="0 0 30 30">
                   <defs><symbol id="s"><path d="M 0 0 L 99 99"/></symbol></defs>
                   <sodipodi:namedview xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"/>{layers}</svg>"#
            )
        }

        fn layer(ch: &str, body: &str) -> String {
            format!(r#"<g inkscape:label="GlyphLayer-{ch}" inkscape:groupmode="layer" style="display:none">{body}</g>"#)
        }

        #[test]
        fn glyph_layers_are_found_even_when_hidden_and_nested_and_definitions_are_ignored() {
            let svg = drawing(&format!(
                "{}<g transform=\"translate(100 0)\">{}</g>{}",
                layer("H", r#"<path d="M 0 0 L 10 0 L 10 20 Z"/>"#),
                layer("i", r#"<g transform="translate(5,0)"><path d="M 0 0 L 4 0"/></g>"#),
                layer("ab", r#"<path d="M 0 0 L 1 1"/>"#),
            ));
            let glyphs = read_glyphs(&svg).unwrap();
            let chars: Vec<char> = glyphs.iter().map(|g| g.ch).collect();
            assert_eq!(chars, ['H', 'i'], "a two-character label is not a glyph; defs are not read");
            let i = glyphs.iter().find(|g| g.ch == 'i').unwrap();
            let b = i.path.bounding_box();
            assert!((b.x0 - 105.0).abs() < 1e-9 && (b.x1 - 109.0).abs() < 1e-9, "transforms compose through groups: {b:?}");
        }

        #[test]
        fn a_satin_column_becomes_the_outline_between_its_rails_and_other_paths_stay_as_drawn() {
            let satin = r#"<path inkstitch:satin_column="True" d="M 0 0 L 10 0 M 0 4 L 10 4 M 5 0 L 5 4"/>"#;
            let line = r#"<path d="M 0 10 L 10 10"/>"#;
            let glyphs = read_glyphs(&drawing(&layer("A", &format!("{satin}{line}")))).unwrap();
            let els = glyphs[0].path.elements();
            // Outline: move, line along rail 1, line across, back along rail 2, close - then the plain line.
            assert!(matches!(els[0], PathEl::MoveTo(_)));
            assert!(els.iter().any(|e| matches!(e, PathEl::ClosePath)));
            let b = glyphs[0].path.bounding_box();
            assert!((b.y0 - 0.0).abs() < 1e-9 && (b.y1 - 10.0).abs() < 1e-9 && (b.x1 - 10.0).abs() < 1e-9, "{b:?}");
            // A satin column with a single subpath is left alone.
            let one = read_glyphs(&drawing(&layer("B", r#"<path inkstitch:satin_column="True" d="M 0 0 L 5 5"/>"#))).unwrap();
            assert!(!one[0].path.elements().iter().any(|e| matches!(e, PathEl::ClosePath)));
        }

        #[test]
        fn hidden_paths_and_bad_numbers_are_skipped_and_transforms_that_make_no_sense_are_ignored() {
            let body = r#"<path style="display: none" d="M 0 0 L 100 100"/><path d="M 0 0 L 1e999 5"/><path d="not a path"/><path d="M 0 0 L 7 0"/>"#;
            let glyphs = read_glyphs(&drawing(&layer("T", body))).unwrap();
            let b = glyphs[0].path.bounding_box();
            assert!((b.x1 - 7.0).abs() < 1e-9 && b.y1.abs() < 1e-9, "{b:?}");
            assert_eq!(parse_transform("rotate(90)").map(|t| t.as_coeffs()[1].round() as i32), Some(1));
            assert_eq!(parse_transform("scale(2) translate(1,1)").map(|t| t.as_coeffs()[4].round() as i32), Some(2));
            for bad in ["foo(1)", "matrix(1,2)", "translate(NaN)", "translate(1", "translate(1e999)", &"translate(1) ".repeat(40)] {
                assert_eq!(parse_transform(bad), None, "{bad}");
            }
        }

        #[test]
        fn hostile_drawings_are_errors_not_panics() {
            for text in [
                "",
                "not xml",
                "<svg/>",
                "<svg><g/></svg>",
                "<!DOCTYPE svg [<!ENTITY a \"b\">]><svg>&a;</svg>",
                "<svg><g inkscape:label=\"GlyphLayer-A\"/></svg>",
            ] {
                assert!(read_glyphs(text).is_err(), "{text:?}");
            }
            let deep = format!("<svg>{}</svg>", "<g>".repeat(200) + &"</g>".repeat(200));
            assert!(read_glyphs(&deep).is_err());
            let many = (0..4_100).map(|_| r#"<path d="M 0 0 L 1 1"/>"#).collect::<String>();
            assert!(read_glyphs(&drawing(&layer("A", &many))).is_err());
            assert!(read_glyphs(&" ".repeat(MAX_SVG_BYTES + 1)).is_err());
        }
    }

    mod measure_tests {
        use super::*;

        #[test]
        fn the_baseline_and_capital_height_come_from_a_flat_bottomed_capital() {
            let mut h = BezPath::new();
            h.move_to((0.0, 6.0));
            h.line_to((8.0, 6.0));
            h.line_to((8.0, 25.0));
            h.line_to((0.0, 25.0));
            h.close_path();
            let glyphs = vec![DrawnGlyph { ch: 'H', path: h }];
            assert_eq!(baseline_and_cap(&glyphs), Some((25.0, 19.0)));
            assert_eq!(baseline_and_cap(&[]), None);
        }
    }
}
