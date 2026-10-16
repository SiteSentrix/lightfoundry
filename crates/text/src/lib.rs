//! Text for laser work: an original single-line ("stroke") font drawn in code, and layout.
//!
//! A stroke font traces every letter with one pass of the laser, which is what engraving wants;
//! outline fonts (for Fill mode) come from a font file the person supplies, see [`layout_outline`].
//! The stroke glyph shapes below are our own design.

mod glyphfont;
mod glyphs;
mod inkstitch;
mod outline;

pub use glyphfont::{GlyphFont, MAX_FONT_JSON_BYTES, MAX_GLYPH_FILE_BYTES, layout_glyph_font};
pub use inkstitch::MAX_SVG_BYTES as MAX_INKSTITCH_SVG_BYTES;
#[cfg(any(test, feature = "testfont"))]
pub use outline::testfont;
pub use outline::{MAX_FONT_BYTES, layout_outline, layout_outline_with_fallback};

use kurbo::{BezPath, Point};
use thiserror::Error;

/// Longest text accepted (characters), so a hostile document cannot make layout run away.
pub const MAX_CHARS: usize = 20_000;
pub const MIN_HEIGHT_MM: f64 = 0.1;
pub const MAX_HEIGHT_MM: f64 = 1000.0;

/// Cap height in glyph units; text `height` is the cap height in mm.
const CAP_UNITS: f64 = 6.0;
/// Baseline-to-baseline distance in glyph units at line spacing 1.0.
const LINE_UNITS: f64 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Cap height in mm.
    pub height: f64,
    /// Multiplier on the gap between letters (1.0 = default).
    pub letter_spacing: f64,
    /// Multiplier on the distance between lines (1.0 = default).
    pub line_spacing: f64,
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { height: 10.0, letter_spacing: 1.0, line_spacing: 1.0, align: Align::Left }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum TextError {
    #[error("text height must be between {MIN_HEIGHT_MM} and {MAX_HEIGHT_MM} mm")]
    BadHeight,
    #[error("spacing must be a finite number between 0 and 10")]
    BadSpacing,
    #[error("text is longer than {MAX_CHARS} characters")]
    TooLong,
    #[error("cannot use that font: {0}")]
    BadFont(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    /// y down, origin at the top-left of the first line's cap height (before alignment shifts).
    pub path: BezPath,
    /// Characters with no glyph (drawn as a box so the gap is visible).
    pub unsupported: Vec<char>,
}

/// Lays out `text` as stroke paths in mm, y down.
pub fn layout(text: &str, style: &TextStyle) -> Result<TextLayout, TextError> {
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
    let scale = style.height / CAP_UNITS;
    let mut path = BezPath::new();
    let mut unsupported: Vec<char> = Vec::new();

    let lines: Vec<&str> = text.split('\n').collect();
    let widths: Vec<f64> = lines.iter().map(|l| line_width(l, style.letter_spacing)).collect();
    let widest = widths.iter().copied().fold(0.0, f64::max);

    for (row, line) in lines.iter().enumerate() {
        let shift = match style.align {
            Align::Left => 0.0,
            Align::Center => (widest - widths[row]) / 2.0,
            Align::Right => widest - widths[row],
        };
        let mut x = shift;
        let top = row as f64 * LINE_UNITS * style.line_spacing;
        for ch in line.chars() {
            let (glyph, known) = glyphs::lookup(ch);
            if !known && !unsupported.contains(&ch) && !ch.is_control() {
                unsupported.push(ch);
            }
            for stroke in &glyph.strokes {
                append_stroke(&mut path, stroke, x, top, scale);
            }
            x += glyph.width + GAP * style.letter_spacing;
        }
    }
    Ok(TextLayout { path, unsupported })
}

/// Stacks the letters top to bottom, one column per line of text with the first line rightmost (the
/// way vertical East Asian text runs). `one` lays out a single character, so this works with the
/// stroke font and with outline fonts alike. Each letter is centred in its column; alignment does
/// not apply. `letter_spacing` widens the gap between rows and `line_spacing` that between columns.
pub fn layout_vertical(text: &str, style: &TextStyle, mut one: impl FnMut(&str) -> Result<TextLayout, TextError>) -> Result<TextLayout, TextError> {
    use kurbo::{Affine, Shape};
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
    let columns: Vec<&str> = text.lines().collect();
    let row_step = style.height * (1.0 + 0.5 * style.letter_spacing);
    let col_step = style.height * (0.5 + style.line_spacing);
    let mut path = BezPath::new();
    let mut unsupported: Vec<char> = Vec::new();
    for (c, line) in columns.iter().enumerate() {
        let centre_x = (columns.len() - 1 - c) as f64 * col_step + col_step / 2.0;
        for (r, ch) in line.chars().enumerate() {
            let mut buf = [0u8; 4];
            let laid = one(ch.encode_utf8(&mut buf))?;
            for u in laid.unsupported {
                if !unsupported.contains(&u) {
                    unsupported.push(u);
                }
            }
            if laid.path.elements().is_empty() {
                continue;
            }
            let b = laid.path.bounding_box();
            let mut glyph = laid.path;
            glyph.apply_affine(Affine::translate((centre_x - (b.x0 + b.x1) / 2.0, r as f64 * row_step)));
            path.extend(glyph);
        }
    }
    Ok(TextLayout { path, unsupported })
}

/// Gap between letters in glyph units.
pub(crate) const GAP: f64 = 2.0;

fn line_width(line: &str, spacing: f64) -> f64 {
    let n = line.chars().count();
    let sum: f64 = line.chars().map(|c| glyphs::lookup(c).0.width).sum();
    // No gap after the last letter.
    (sum + GAP * spacing * n.saturating_sub(1) as f64).max(0.0)
}

fn append_stroke(path: &mut BezPath, stroke: &glyphs::Stroke, x0: f64, top: f64, scale: f64) {
    let to_mm = |p: &Point| Point::new((x0 + p.x) * scale, (top + CAP_UNITS - p.y) * scale);
    let pts: Vec<Point> = stroke.points.iter().map(to_mm).collect();
    let Some((first, rest)) = pts.split_first() else { return };
    path.move_to(*first);
    for p in rest {
        path.line_to(*p);
    }
    if stroke.closed {
        path.close_path();
    }
}

#[cfg(test)]
mod tests;
