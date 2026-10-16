//! Fonts made of ready-drawn glyph outlines: the format of the `stitchfoundry-fonts` library, a folder
//! holding `font.json` (name, advances, leading) and `glyphs.stitchfoundry` (one closed path per glyph,
//! each a list of anchors with absolute in/out Bézier handles; y points down, the baseline is y = 0).
//!
//! The files come from outside, so everything is bounded and checked: sizes, counts, nesting depth and
//! every number. Layout is plain left to right (no shaping, bidi or kerning): these are Latin lettering
//! fonts. The result is filled outlines, like a TrueType font, to be engraved with a Fill layer.

use std::collections::HashMap;

use kurbo::{BezPath, Point, Shape};
use serde_json::Value;

use crate::{Align, MAX_CHARS, MAX_HEIGHT_MM, MIN_HEIGHT_MM, TextError, TextLayout, TextStyle};

/// Largest `font.json` accepted, in bytes.
pub const MAX_FONT_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Largest glyph file accepted, in bytes.
pub const MAX_GLYPH_FILE_BYTES: usize = 32 * 1024 * 1024;
/// Most glyphs kept, anchors in one glyph, and anchors in the whole font.
const MAX_GLYPHS: usize = 2_000;
const MAX_ANCHORS_PER_GLYPH: usize = 20_000;
const MAX_ANCHORS: usize = 400_000;
/// Deepest JSON nesting walked when looking for glyph paths.
const MAX_DEPTH: usize = 24;
/// Used when the font does not say: the space as a share of the cap height, and the line height likewise.
const FALLBACK_SPACE: f64 = 0.35;
const FALLBACK_LEADING: f64 = 1.4;

#[derive(Debug, Clone, PartialEq)]
pub struct GlyphFont {
    pub name: String,
    glyphs: HashMap<char, Glyph>,
    space_advance: Option<f64>,
    leading: Option<f64>,
    /// Height of a capital, in the font's own units.
    cap_height: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Glyph {
    path: BezPath,
    advance: f64,
}

/// One closed outline as anchors: position and, where curved, the handles into and out of it.
#[derive(Debug, Clone)]
struct Contour {
    anchors: Vec<(Point, Option<Point>, Option<Point>)>,
}

impl GlyphFont {
    /// Reads an Ink/Stitch font: its `font.json` and `ltr.svg` (see the `inkstitch` module). Glyphs are
    /// placed on a baseline at y = 0 and scaled by nothing here (layout scales from the capital height);
    /// advances come from `font.json` where it has them, else the glyph's width plus a small gap.
    pub fn parse_inkstitch(font_json: &str, svg: &str) -> Result<GlyphFont, TextError> {
        let bad = |why: &str| TextError::BadFont(why.to_string());
        if font_json.len() > MAX_FONT_JSON_BYTES {
            return Err(bad("the font files are too large"));
        }
        let info: Value = serde_json::from_str(font_json).map_err(|e| TextError::BadFont(format!("font.json: {e}")))?;
        let number = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0);
        let drawn = crate::inkstitch::read_glyphs(svg)?;
        let (baseline, cap_height) = crate::inkstitch::baseline_and_cap(&drawn).ok_or_else(|| bad("cannot tell how tall a letter is"))?;
        let advances: HashMap<char, f64> = info
            .get("horiz_adv_x")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        let mut chars = k.chars();
                        let c = chars.next().filter(|_| chars.next().is_none())?;
                        Some((c, number(Some(v))?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut glyphs = HashMap::new();
        for g in drawn {
            let bounds = g.path.bounding_box();
            if !(bounds.x0.is_finite() && bounds.x1.is_finite()) {
                continue;
            }
            // Left edge to x = 0 and the baseline to y = 0 (up is negative), as in the other glyph fonts.
            let mut path = g.path;
            path.apply_affine(kurbo::Affine::translate((-bounds.x0, -baseline)));
            let width = bounds.x1 - bounds.x0;
            let advance = advances.get(&g.ch).map_or(width + cap_height * 0.12, |a| a.max(width));
            glyphs.entry(g.ch).or_insert(Glyph { path, advance });
        }
        if glyphs.is_empty() {
            return Err(bad("the drawing has no usable glyphs"));
        }
        Ok(GlyphFont {
            name: info.get("name").and_then(Value::as_str).unwrap_or("").chars().take(80).collect(),
            glyphs,
            space_advance: number(info.get("horiz_adv_x_space")).filter(|n| *n > 0.0),
            leading: number(info.get("leading")).filter(|n| *n > 0.0),
            cap_height,
        })
    }

    /// Reads the two files' text. `Err` says what is wrong; nothing here panics on any input.
    pub fn parse(font_json: &str, glyphs_json: &str) -> Result<GlyphFont, TextError> {
        let bad = |why: &str| TextError::BadFont(why.to_string());
        if font_json.len() > MAX_FONT_JSON_BYTES || glyphs_json.len() > MAX_GLYPH_FILE_BYTES {
            return Err(bad("the font files are too large"));
        }
        let info: Value = serde_json::from_str(font_json).map_err(|e| TextError::BadFont(format!("font.json: {e}")))?;
        let doc: Value = serde_json::from_str(glyphs_json).map_err(|e| TextError::BadFont(format!("glyphs.stitchfoundry: {e}")))?;
        let number = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0);
        let advances: HashMap<char, f64> = info
            .get("horiz_adv_x")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| {
                        let mut chars = k.chars();
                        let c = chars.next().filter(|_| chars.next().is_none())?;
                        Some((c, number(Some(v))?))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut found: Vec<(char, Vec<Contour>)> = Vec::new();
        let mut anchors = 0usize;
        collect(&doc, 0, &mut found, &mut anchors)?;
        if found.is_empty() {
            return Err(bad("the file has no glyph outlines"));
        }
        let mut glyphs = HashMap::new();
        for (c, contours) in found {
            let path = build_path(&contours);
            let width = path.bounding_box().x1;
            let advance = advances.get(&c).copied().unwrap_or(if width.is_finite() { width.max(0.0) } else { 0.0 });
            glyphs.entry(c).or_insert(Glyph { path, advance });
        }
        let cap_height = ['H', 'E', 'I', 'T', 'L', 'Z']
            .iter()
            .find_map(|c| glyphs.get(c).map(|g| -g.path.bounding_box().y0))
            .or_else(|| glyphs.iter().filter(|(c, _)| c.is_uppercase()).map(|(_, g)| -g.path.bounding_box().y0).reduce(f64::max))
            .filter(|h| h.is_finite() && *h > 0.0)
            .ok_or_else(|| bad("cannot tell how tall a capital is (no usable capital letters)"))?;
        Ok(GlyphFont {
            name: info.get("name").and_then(Value::as_str).unwrap_or("").chars().take(80).collect(),
            glyphs,
            space_advance: number(info.get("horiz_adv_x_space")).filter(|n| *n > 0.0),
            leading: number(info.get("leading")).filter(|n| *n > 0.0),
            cap_height,
        })
    }

    /// Whether the font has a drawing for `c` (a space always counts).
    pub fn has(&self, c: char) -> bool {
        c.is_whitespace() || self.glyphs.contains_key(&c)
    }
}

/// Lays out `text` with `font`, capitals `style.height` mm tall; y down, the top of the first line's
/// capitals at y = 0, like [`crate::layout_outline`].
pub fn layout_glyph_font(font: &GlyphFont, text: &str, style: &TextStyle) -> Result<TextLayout, TextError> {
    if !(style.height.is_finite() && (MIN_HEIGHT_MM..=MAX_HEIGHT_MM).contains(&style.height)) {
        return Err(TextError::BadHeight);
    }
    let ok = |v: f64| v.is_finite() && (0.0..=10.0).contains(&v);
    if !(ok(style.letter_spacing) && ok(style.line_spacing)) {
        return Err(TextError::BadSpacing);
    }
    if text.chars().count() > MAX_CHARS {
        return Err(TextError::TooLong);
    }
    let scale = style.height / font.cap_height;
    let space = font.space_advance.unwrap_or(font.cap_height * FALLBACK_SPACE) * scale;
    let line_height = font.leading.unwrap_or(font.cap_height * FALLBACK_LEADING) * scale * style.line_spacing;

    let mut unsupported: Vec<char> = Vec::new();
    let mut rows: Vec<(Vec<(f64, &Glyph)>, f64)> = Vec::new();
    for line in text.lines() {
        let (mut x, mut row) = (0.0, Vec::new());
        for c in line.chars() {
            match font.glyphs.get(&c) {
                Some(g) => {
                    row.push((x, g));
                    x += g.advance * scale * style.letter_spacing;
                }
                None => {
                    if !c.is_whitespace() && !unsupported.contains(&c) {
                        unsupported.push(c);
                    }
                    x += space * style.letter_spacing;
                }
            }
        }
        rows.push((row, x));
    }
    let widest = rows.iter().map(|(_, w)| *w).fold(0.0, f64::max);
    let mut path = BezPath::new();
    for (i, (row, width)) in rows.iter().enumerate() {
        let shift = match style.align {
            Align::Left => 0.0,
            Align::Center => (widest - width) / 2.0,
            Align::Right => widest - width,
        };
        let baseline = style.height + i as f64 * line_height;
        for (x, glyph) in row {
            let mut g = glyph.path.clone();
            g.apply_affine(kurbo::Affine::translate((x + shift, baseline)) * kurbo::Affine::scale(scale));
            path.extend(g);
        }
    }
    Ok(TextLayout { path, unsupported })
}

/// Finds every object that is a path named by one character, anywhere in the document.
fn collect(node: &Value, depth: usize, out: &mut Vec<(char, Vec<Contour>)>, anchors: &mut usize) -> Result<(), TextError> {
    if depth > MAX_DEPTH {
        return Err(TextError::BadFont("the glyph file is nested too deeply".into()));
    }
    match node {
        Value::Object(map) => {
            let is_path = map.get("kind").and_then(|k| k.get("type")).and_then(Value::as_str) == Some("path");
            if is_path
                && let Some(name) = map.get("name").and_then(Value::as_str)
                && let mut chars = name.chars()
                && let Some(c) = chars.next()
                && chars.next().is_none()
            {
                if out.len() >= MAX_GLYPHS {
                    return Err(TextError::BadFont(format!("more than {MAX_GLYPHS} glyphs")));
                }
                let contours = contours_of(map.get("kind").and_then(|k| k.get("path")), anchors)?;
                if !contours.is_empty() {
                    out.push((c, contours));
                }
                return Ok(());
            }
            for v in map.values() {
                collect(v, depth + 1, out, anchors)?;
            }
        }
        Value::Array(items) => {
            for v in items {
                collect(v, depth + 1, out, anchors)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn contours_of(path: Option<&Value>, total: &mut usize) -> Result<Vec<Contour>, TextError> {
    let point = |v: Option<&Value>| -> Option<Point> {
        let a = v?.as_array()?;
        let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
        (x.is_finite() && y.is_finite() && x.abs() < 1e7 && y.abs() < 1e7).then(|| Point::new(x, y))
    };
    let mut contours = Vec::new();
    let mut in_glyph = 0usize;
    for sub in path.and_then(|p| p.get("subpaths")).and_then(Value::as_array).into_iter().flatten() {
        let mut anchors = Vec::new();
        for a in sub.get("anchors").and_then(Value::as_array).into_iter().flatten() {
            in_glyph += 1;
            *total += 1;
            if in_glyph > MAX_ANCHORS_PER_GLYPH || *total > MAX_ANCHORS {
                return Err(TextError::BadFont("the glyph file has too many points".into()));
            }
            // An anchor without a usable position is skipped, not trusted.
            if let Some(p) = point(a.get("p")) {
                anchors.push((p, point(a.get("in")), point(a.get("out"))));
            }
        }
        if anchors.len() >= 3 {
            contours.push(Contour { anchors });
        }
    }
    Ok(contours)
}

/// The glyph's outline with outer shapes and counters wound in opposite directions, so a filled
/// glyph keeps its holes under either fill rule.
fn build_path(contours: &[Contour]) -> BezPath {
    // Nesting depth of each contour: how many other contours contain its first point.
    let polygons: Vec<Vec<Point>> = contours.iter().map(|c| c.anchors.iter().map(|a| a.0).collect()).collect();
    let mut path = BezPath::new();
    for (i, contour) in contours.iter().enumerate() {
        let first = polygons[i][0];
        let depth = polygons.iter().enumerate().filter(|(j, poly)| *j != i && inside(poly, first)).count();
        let area = signed_area(&polygons[i]);
        // Outer shapes (even depth) run one way, counters the other.
        let want_positive = depth % 2 == 0;
        let reverse = (area > 0.0) != want_positive;
        append(&mut path, contour, reverse);
    }
    path
}

fn append(path: &mut BezPath, contour: &Contour, reverse: bool) {
    let mut anchors = contour.anchors.clone();
    if reverse {
        anchors.reverse();
        for a in &mut anchors {
            std::mem::swap(&mut a.1, &mut a.2);
        }
    }
    let Some(first) = anchors.first() else { return };
    path.move_to(first.0);
    for i in 0..anchors.len() {
        let (a, b) = (&anchors[i], &anchors[(i + 1) % anchors.len()]);
        if a.2.is_none() && b.1.is_none() {
            if i + 1 < anchors.len() {
                path.line_to(b.0);
            }
        } else {
            path.curve_to(a.2.unwrap_or(a.0), b.1.unwrap_or(b.0), b.0);
        }
    }
    path.close_path();
}

fn signed_area(poly: &[Point]) -> f64 {
    poly.iter().zip(poly.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() / 2.0
}

/// Even-odd point in polygon.
fn inside(poly: &[Point], p: Point) -> bool {
    let mut inside = false;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_tests {
        use super::*;

        #[test]
        fn a_small_font_is_read_with_its_advances_and_cap_height() {
            let f = GlyphFont::parse(&font_json(), &glyph_file()).unwrap();
            assert_eq!(f.name, "Test Sans");
            assert!((f.cap_height - 70.0).abs() < 1e-9);
            assert!(f.has('H') && f.has('o') && f.has(' ') && !f.has('Q'));
            assert_eq!(f.glyphs[&'H'].advance, 50.0);
        }

        #[test]
        fn hostile_files_are_errors_not_panics() {
            let good = glyph_file();
            for (font, glyphs) in [
                ("", ""),
                ("{}", "{}"),
                ("[]", "[]"),
                ("{}", r#"{"document":{"layers":[]}}"#),
                (&font_json()[..], "not json"),
                ("not json", &good[..]),
                ("{}", r#"{"a":{"kind":{"type":"path","path":{"subpaths":[{"anchors":[{"p":[1,2]}]}]}},"name":"H"}}"#),
                (
                    "{}",
                    r#"{"a":{"kind":{"type":"path","path":{"subpaths":[{"anchors":[{"p":["x",null]},{"p":[1e308,1e308]},{"p":[1,1]}]}]}},"name":"H"}}"#,
                ),
            ] {
                assert!(GlyphFont::parse(font, glyphs).is_err(), "{font:?} {glyphs:?}");
            }
            // Deep nesting and a flood of glyphs or points are refused.
            let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
            assert!(GlyphFont::parse("{}", &deep).is_err());
            let anchors: Vec<String> = (0..30_000).map(|i| format!(r#"{{"p":[{i},{}]}}"#, i % 7)).collect();
            let many = format!(r#"{{"kind":{{"type":"path","path":{{"subpaths":[{{"anchors":[{}]}}]}}}},"name":"H"}}"#, anchors.join(","));
            assert!(GlyphFont::parse("{}", &many).is_err());
            let glyphs: Vec<String> = (0..2_100)
                .map(|i| format!(r#"{{"kind":{{"type":"path","path":{{"subpaths":[{{"anchors":[{{"p":[0,0]}},{{"p":[1,0]}},{{"p":[1,-{}]}}]}}]}}}},"name":"{}"}}"#, i + 1, char::from_u32(0x4e00 + i).unwrap_or('x')))
                .collect();
            assert!(GlyphFont::parse("{}", &format!("[{}]", glyphs.join(","))).is_err());
            assert!(GlyphFont::parse(&" ".repeat(MAX_FONT_JSON_BYTES + 1), &good).is_err());
        }
    }

    mod layout_tests {
        use super::*;

        fn font() -> GlyphFont {
            GlyphFont::parse(&font_json(), &glyph_file()).unwrap()
        }

        #[test]
        fn capitals_come_out_at_the_requested_height_with_the_top_of_the_first_line_at_zero() {
            let l = layout_glyph_font(&font(), "H", &TextStyle { height: 10.0, ..TextStyle::default() }).unwrap();
            let b = l.path.bounding_box();
            assert!((b.y0 - 0.0).abs() < 1e-9 && (b.y1 - 10.0).abs() < 1e-9, "{b:?}");
            assert!(b.x0 >= 0.0 && b.x1 <= 8.0, "50 of 70 units wide: {b:?}");
            assert!(l.unsupported.is_empty());
        }

        #[test]
        fn letters_advance_spacing_alignment_and_lines_work_and_missing_letters_are_reported() {
            let f = font();
            let one = layout_glyph_font(&f, "HH", &TextStyle::default()).unwrap().path.bounding_box();
            assert!((one.x1 - (50.0 + 40.0) * 10.0 / 70.0 - 0.0).abs() < 1.5, "{one:?}");
            let wide = layout_glyph_font(&f, "HH", &TextStyle { letter_spacing: 2.0, ..TextStyle::default() }).unwrap().path.bounding_box();
            assert!(wide.x1 > one.x1);
            let two = layout_glyph_font(&f, "H\nH", &TextStyle::default()).unwrap().path.bounding_box();
            assert!(two.y1 > 10.0 * 1.4, "{two:?}");
            let right = layout_glyph_font(&f, "HHH\nH", &TextStyle { align: Align::Right, ..TextStyle::default() }).unwrap();
            assert!(right.path.bounding_box().x1 > 0.0);
            let missing = layout_glyph_font(&f, "H Q Q?", &TextStyle::default()).unwrap();
            assert_eq!(missing.unsupported, vec!['Q', '?']);
        }

        #[test]
        fn bad_styles_and_overlong_text_are_errors() {
            let f = font();
            assert_eq!(layout_glyph_font(&f, "H", &TextStyle { height: 0.0, ..TextStyle::default() }).unwrap_err(), TextError::BadHeight);
            assert_eq!(layout_glyph_font(&f, "H", &TextStyle { height: f64::NAN, ..TextStyle::default() }).unwrap_err(), TextError::BadHeight);
            assert_eq!(
                layout_glyph_font(&f, "H", &TextStyle { letter_spacing: f64::INFINITY, ..TextStyle::default() }).unwrap_err(),
                TextError::BadSpacing
            );
            assert_eq!(layout_glyph_font(&f, &"H".repeat(MAX_CHARS + 1), &TextStyle::default()).unwrap_err(), TextError::TooLong);
            assert!(layout_glyph_font(&f, "", &TextStyle::default()).unwrap().path.elements().is_empty());
        }

        #[test]
        fn counters_are_wound_against_their_outer_shape_whatever_the_file_says() {
            // 'o': an outer square and an inner square, both written the same way round.
            let outer = square(0.0, -50.0, 40.0, 0.0, true);
            let inner = square(10.0, -40.0, 30.0, -10.0, true);
            let o = Contour { anchors: outer.iter().map(|p| (*p, None, None)).collect() };
            let i = Contour { anchors: inner.iter().map(|p| (*p, None, None)).collect() };
            let path = build_path(&[o, i]);
            let mut areas = Vec::new();
            let mut poly = Vec::new();
            for el in path.elements() {
                match el {
                    kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => poly.push(*p),
                    kurbo::PathEl::ClosePath => areas.push(signed_area(&std::mem::take(&mut poly))),
                    _ => {}
                }
            }
            assert_eq!(areas.len(), 2);
            assert!(areas[0] * areas[1] < 0.0, "opposite directions: {areas:?}");
        }
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64, clockwise_in_y_down: bool) -> Vec<Point> {
        let pts = vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)];
        if clockwise_in_y_down { pts } else { pts.into_iter().rev().collect() }
    }

    fn font_json() -> String {
        r#"{"name":"Test Sans","horiz_adv_x":{"H":50,"o":40,"i":20,"?":"bad","ab":3},"horiz_adv_x_space":30,"leading":98}"#.into()
    }

    /// Glyphs H (a plain bar shape), o (outer square with a counter written the same way round) and i,
    /// nested the way the real files are (a document with layers of children).
    fn glyph_file() -> String {
        let anchors = |pts: &[(f64, f64)]| pts.iter().map(|(x, y)| format!(r#"{{"p":[{x},{y}]}}"#)).collect::<Vec<_>>().join(",");
        let glyph = |name: &str, subs: &[&[(f64, f64)]]| {
            let subpaths: Vec<String> = subs.iter().map(|s| format!(r#"{{"anchors":[{}],"closed":true}}"#, anchors(s))).collect();
            format!(r#"{{"id":1,"name":"{name}","kind":{{"type":"path","path":{{"subpaths":[{}]}}}}}}"#, subpaths.join(","))
        };
        let h: &[(f64, f64)] = &[(0.0, 0.0), (0.0, -70.0), (50.0, -70.0), (50.0, 0.0)];
        let o_out: &[(f64, f64)] = &[(0.0, 0.0), (0.0, -50.0), (40.0, -50.0), (40.0, 0.0)];
        let o_in: &[(f64, f64)] = &[(10.0, -10.0), (10.0, -40.0), (30.0, -40.0), (30.0, -10.0)];
        let i: &[(f64, f64)] = &[(0.0, 0.0), (0.0, -50.0), (20.0, -50.0), (20.0, 0.0)];
        format!(
            r#"{{"format":"x","document":{{"layers":[{{"kind":{{"type":"layer","children":[{},{},{}]}}}}]}}}}"#,
            glyph("H", &[h]),
            glyph("o", &[o_out, o_in]),
            glyph("i", &[i])
        )
    }
}
