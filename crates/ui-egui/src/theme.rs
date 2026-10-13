//! Colours come from tokens, never literals scattered through panels.

use egui::Color32;

pub struct Tokens {
    pub workspace: Color32,
    pub bed: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub origin: Color32,
    pub selection: Color32,
    pub handle: Color32,
    pub travel: Color32,
    pub error: Color32,
    pub ok: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        workspace: Color32::from_rgb(30, 32, 36),
        bed: Color32::from_rgb(48, 52, 58),
        grid_minor: Color32::from_rgb(58, 62, 69),
        grid_major: Color32::from_rgb(78, 84, 93),
        origin: Color32::from_rgb(230, 90, 70),
        selection: Color32::from_rgb(90, 170, 255),
        handle: Color32::from_rgb(255, 255, 255),
        travel: Color32::from_rgba_premultiplied(120, 120, 130, 90),
        error: Color32::from_rgb(255, 110, 100),
        ok: Color32::from_rgb(120, 210, 140),
    };
}

/// Parses `#rrggbb`; falls back to white so a corrupt colour string cannot hide a shape.
pub fn parse_hex(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    let channel = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    match (channel(0), channel(2), channel(4)) {
        (Some(r), Some(g), Some(b)) if h.len() == 6 => Color32::from_rgb(r, g, b),
        _ => Color32::WHITE,
    }
}

/// Layer colours are chosen for white paper (black is the first layer); on a dark canvas lift dark
/// colours so they stay visible while keeping their hue (pure blue must not look like black's white).
pub fn on_dark(c: Color32) -> Color32 {
    let lum = (u32::from(c.r()) * 3 + u32::from(c.g()) * 6 + u32::from(c.b())) / 10;
    if lum < 8 {
        Color32::from_rgb(235, 235, 235)
    } else if lum < 60 {
        let lift = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * 0.45).round() as u8;
        Color32::from_rgb(lift(c.r()), lift(c.g()), lift(c.b()))
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing_is_forgiving() {
        assert_eq!(parse_hex("#ff8000"), Color32::from_rgb(255, 128, 0));
        assert_eq!(parse_hex("zzzzzz"), Color32::WHITE);
        assert_eq!(parse_hex("#fff"), Color32::WHITE);
        assert_eq!(parse_hex(""), Color32::WHITE);
        assert_eq!(parse_hex("#ff80"), Color32::WHITE);
    }

    #[test]
    fn black_is_lifted_on_dark() {
        assert_ne!(on_dark(Color32::BLACK), Color32::BLACK);
        assert_eq!(on_dark(Color32::from_rgb(255, 0, 0)), Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn dark_hues_keep_their_identity() {
        let blue = on_dark(parse_hex("#0000ff"));
        let navy = on_dark(parse_hex("#0000a0"));
        assert!(blue.b() > blue.r() + 60, "blue must stay blue: {blue:?}");
        assert!(navy.b() > navy.r() + 40 && navy != blue, "{navy:?}");
    }
}
