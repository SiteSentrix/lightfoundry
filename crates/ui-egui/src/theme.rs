//! Colours come from tokens, never literals scattered through panels.

use egui::Color32;

pub struct Tokens {
    /// Whether these are the dark colours (layer colours are adjusted to stay visible either way).
    pub dark: bool,
    pub workspace: Color32,
    pub bed: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub origin: Color32,
    pub selection: Color32,
    pub handle: Color32,
    pub travel: Color32,
    pub guide: Color32,
    /// Equal-spacing measurements while moving.
    pub spacing: Color32,
    pub ruler: Color32,
    pub ruler_text: Color32,
    pub error: Color32,
    pub ok: Color32,
}

/// Which colour scheme to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow the system.
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Auto, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Auto => "Follow system",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::Auto => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

impl Tokens {
    pub const LIGHT: Tokens = Tokens {
        dark: false,
        workspace: Color32::from_rgb(226, 229, 234),
        bed: Color32::from_rgb(250, 251, 252),
        grid_minor: Color32::from_rgb(232, 235, 239),
        grid_major: Color32::from_rgb(205, 210, 218),
        origin: Color32::from_rgb(200, 60, 40),
        selection: Color32::from_rgb(20, 110, 220),
        handle: Color32::from_rgb(255, 255, 255),
        travel: Color32::from_rgba_premultiplied(90, 90, 100, 110),
        guide: Color32::from_rgb(0, 150, 160),
        spacing: Color32::from_rgb(225, 100, 0),
        ruler: Color32::from_rgb(238, 240, 243),
        ruler_text: Color32::from_rgb(90, 96, 106),
        error: Color32::from_rgb(190, 40, 30),
        ok: Color32::from_rgb(30, 140, 70),
    };

    pub const DARK: Tokens = Tokens {
        dark: true,
        workspace: Color32::from_rgb(30, 32, 36),
        bed: Color32::from_rgb(48, 52, 58),
        grid_minor: Color32::from_rgb(58, 62, 69),
        grid_major: Color32::from_rgb(78, 84, 93),
        origin: Color32::from_rgb(230, 90, 70),
        selection: Color32::from_rgb(90, 170, 255),
        handle: Color32::from_rgb(255, 255, 255),
        travel: Color32::from_rgba_premultiplied(120, 120, 130, 90),
        guide: Color32::from_rgb(70, 200, 210),
        spacing: Color32::from_rgb(255, 160, 60),
        ruler: Color32::from_rgb(36, 39, 44),
        ruler_text: Color32::from_rgb(150, 156, 166),
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
impl Tokens {
    /// A layer's colour, adjusted so it shows against this scheme's canvas.
    pub fn layer_color(&self, hex: &str) -> Color32 {
        let c = parse_hex(hex);
        if self.dark { on_dark(c) } else { on_light(c) }
    }
}

/// Darkens colours too pale to see on a light canvas (white, pale yellow), keeping their hue.
pub fn on_light(c: Color32) -> Color32 {
    let luma = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    const LIMIT: f32 = 170.0;
    if luma <= LIMIT {
        return c;
    }
    let k = LIMIT / luma;
    Color32::from_rgb((f32::from(c.r()) * k) as u8, (f32::from(c.g()) * k) as u8, (f32::from(c.b()) * k) as u8)
}

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
    fn pale_colours_are_darkened_on_a_light_canvas_and_strong_ones_are_left_alone() {
        assert!(on_light(Color32::WHITE).r() < 200);
        assert_eq!(on_light(Color32::from_rgb(200, 0, 0)), Color32::from_rgb(200, 0, 0));
        let yellow = on_light(Color32::from_rgb(255, 255, 0));
        assert!(yellow.r() < 255 && yellow.b() == 0, "the hue is kept: {yellow:?}");
        assert_eq!(Tokens::LIGHT.layer_color("#ffffff"), on_light(Color32::WHITE));
        assert_eq!(Tokens::DARK.layer_color("#000000"), on_dark(Color32::BLACK));
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
