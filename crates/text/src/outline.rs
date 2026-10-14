//! Outline text from a font file the person supplies at run time (TrueType or OpenType). No font
//! ships with the program or lives in the repository; the laid-out shapes are stored in the project,
//! so a project opens and cuts without the font, and only re-editing the text needs it again.
//!
//! Glyphs come from `skrifa` (MIT or Apache-2.0). Layout is simple: advance widths and the font's
//! line height, no kerning or shaping, so complex scripts that need shaping are not supported.

use kurbo::{Affine, BezPath};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

use crate::{Align, MAX_CHARS, MAX_HEIGHT_MM, MIN_HEIGHT_MM, TextError, TextLayout, TextStyle};

/// Largest font file read, in bytes.
pub const MAX_FONT_BYTES: usize = 64 * 1024 * 1024;
/// Cap height as a share of the em when the font does not say.
const FALLBACK_CAP: f64 = 0.7;

/// Lays out `text` with the outlines of the font in `data`, scaled so capitals are `style.height`
/// mm tall. The result is y down with the top of the first line's capitals at y = 0.
pub fn layout_outline(data: &[u8], text: &str, style: &TextStyle) -> Result<TextLayout, TextError> {
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
    if data.len() > MAX_FONT_BYTES {
        return Err(TextError::BadFont("the font file is too large".into()));
    }
    let font = FontRef::new(data).map_err(|e| TextError::BadFont(e.to_string()))?;
    let loc = LocationRef::default();
    let metrics = font.metrics(Size::unscaled(), loc);
    let glyph_metrics = font.glyph_metrics(Size::unscaled(), loc);
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();

    let upem = f64::from(metrics.units_per_em.max(1));
    let cap = f64::from(metrics.cap_height.unwrap_or(0.0));
    let cap = if cap > 0.0 { cap } else { upem * FALLBACK_CAP };
    let scale = style.height / cap;
    let line_height = ((f64::from(metrics.ascent) - f64::from(metrics.descent) + f64::from(metrics.leading)).max(cap)) * scale * style.line_spacing;

    let mut path = BezPath::new();
    let mut unsupported: Vec<char> = Vec::new();
    // Widths first, so alignment can shift each line.
    let lines: Vec<&str> = text.lines().collect();
    let mut widths = Vec::with_capacity(lines.len());
    let mut placed: Vec<Vec<(char, skrifa::GlyphId, f64)>> = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut x = 0.0;
        let mut row = Vec::new();
        for ch in line.chars() {
            let gid = match charmap.map(ch) {
                Some(g) => g,
                None => {
                    if !ch.is_whitespace() && !unsupported.contains(&ch) {
                        unsupported.push(ch);
                    }
                    skrifa::GlyphId::new(0)
                }
            };
            let advance = f64::from(glyph_metrics.advance_width(gid).unwrap_or(0.0));
            row.push((ch, gid, x));
            x += advance * scale * style.letter_spacing;
        }
        widths.push(x);
        placed.push(row);
    }
    let widest = widths.iter().copied().fold(0.0, f64::max);
    for (i, row) in placed.iter().enumerate() {
        let shift = match style.align {
            Align::Left => 0.0,
            Align::Center => (widest - widths[i]) / 2.0,
            Align::Right => widest - widths[i],
        };
        let baseline = style.height + i as f64 * line_height;
        for (ch, gid, x) in row {
            if ch.is_whitespace() {
                continue;
            }
            let Some(glyph) = outlines.get(*gid) else { continue };
            let mut pen = PathPen::default();
            if glyph.draw(DrawSettings::unhinted(Size::unscaled(), loc), &mut pen).is_err() {
                continue;
            }
            // Font units are y up; flip, scale and move to the pen position.
            let m = Affine::translate((x + shift, baseline)) * Affine::scale_non_uniform(scale, -scale);
            let mut g = pen.path;
            g.apply_affine(m);
            path.extend(g);
        }
    }
    Ok(TextLayout { path, unsupported })
}

#[derive(Default)]
struct PathPen {
    path: BezPath,
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to((f64::from(cx0), f64::from(cy0)), (f64::from(x), f64::from(y)));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to((f64::from(cx0), f64::from(cy0)), (f64::from(cx1), f64::from(cy1)), (f64::from(x), f64::from(y)));
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

#[cfg(any(test, feature = "testfont"))]
pub mod testfont;

#[cfg(test)]
mod tests;
