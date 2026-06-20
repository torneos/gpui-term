//! Color mapping: alacritty ANSI colors → GPUI [`Hsla`].

use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui::Hsla;

// =========================================================================
// TerminalColors — ANSI color palette
// =========================================================================

/// Theme-defined ANSI terminal colors.
///
/// Maps [`NamedColor`] to [`Hsla`] for text and background rendering.
/// Create one from your theme system, or use [`TerminalColors::default_dark`]
/// for a basic dark xterm-like palette.
#[derive(Clone, Debug)]
pub struct TerminalColors {
    /// Default background color.
    pub background: Hsla,
    /// Default foreground (text) color.
    pub foreground: Hsla,
    /// Bright/bold foreground variant (used for cursor, bold text).
    pub bright_foreground: Hsla,
    /// Dimmed foreground variant (used for unfocused cursor).
    pub dim_foreground: Hsla,

    pub black: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub white: Hsla,

    pub bright_black: Hsla,
    pub bright_red: Hsla,
    pub bright_green: Hsla,
    pub bright_yellow: Hsla,
    pub bright_blue: Hsla,
    pub bright_magenta: Hsla,
    pub bright_cyan: Hsla,
    pub bright_white: Hsla,

    pub dim_black: Hsla,
    pub dim_red: Hsla,
    pub dim_green: Hsla,
    pub dim_yellow: Hsla,
    pub dim_blue: Hsla,
    pub dim_magenta: Hsla,
    pub dim_cyan: Hsla,
    pub dim_white: Hsla,
}

impl TerminalColors {
    /// A basic dark terminal palette (xterm-like).
    pub fn default_dark() -> Self {
        Self {
            background: Hsla::default(),
            foreground: hsl(0.0, 0.0, 0.85),
            bright_foreground: hsl(0.0, 0.0, 0.95),
            dim_foreground: hsl(0.0, 0.0, 0.5),

            black: hsl(0.0, 0.0, 0.0),
            red: hsl(0.0, 0.8, 0.45),
            green: hsl(0.33, 0.8, 0.45),
            yellow: hsl(0.16, 0.8, 0.45),
            blue: hsl(0.6, 0.8, 0.45),
            magenta: hsl(0.8, 0.8, 0.45),
            cyan: hsl(0.5, 0.8, 0.45),
            white: hsl(0.0, 0.0, 0.75),

            bright_black: hsl(0.0, 0.0, 0.35),
            bright_red: hsl(0.0, 1.0, 0.65),
            bright_green: hsl(0.33, 1.0, 0.65),
            bright_yellow: hsl(0.16, 1.0, 0.65),
            bright_blue: hsl(0.6, 1.0, 0.65),
            bright_magenta: hsl(0.8, 1.0, 0.65),
            bright_cyan: hsl(0.5, 1.0, 0.65),
            bright_white: hsl(0.0, 0.0, 0.95),

            dim_black: hsl(0.0, 0.0, 0.2),
            dim_red: hsl(0.0, 0.5, 0.3),
            dim_green: hsl(0.33, 0.5, 0.3),
            dim_yellow: hsl(0.16, 0.5, 0.3),
            dim_blue: hsl(0.6, 0.5, 0.3),
            dim_magenta: hsl(0.8, 0.5, 0.3),
            dim_cyan: hsl(0.5, 0.5, 0.3),
            dim_white: hsl(0.0, 0.0, 0.5),
        }
    }
}

fn hsl(h: f32, s: f32, l: f32) -> Hsla {
    Hsla { h, s, l, a: 1.0 }
}

// =========================================================================
// Color conversion — NamedColor → Hsla
// =========================================================================

/// Map a VTE [`NamedColor`] to an Hsla using the provided palette.
pub fn named_hsla(n: NamedColor, tc: &TerminalColors) -> Hsla {
    use NamedColor::*;
    match n {
        Black => tc.black,
        Red => tc.red,
        Green => tc.green,
        Yellow => tc.yellow,
        Blue => tc.blue,
        Magenta => tc.magenta,
        Cyan => tc.cyan,
        White => tc.white,
        BrightBlack => tc.bright_black,
        BrightRed => tc.bright_red,
        BrightGreen => tc.bright_green,
        BrightYellow => tc.bright_yellow,
        BrightBlue => tc.bright_blue,
        BrightMagenta => tc.bright_magenta,
        BrightCyan => tc.bright_cyan,
        BrightWhite => tc.bright_white,
        Foreground => tc.foreground,
        Background => tc.background,
        Cursor => tc.bright_foreground,
        DimBlack => tc.dim_black,
        DimRed => tc.dim_red,
        DimGreen => tc.dim_green,
        DimYellow => tc.dim_yellow,
        DimBlue => tc.dim_blue,
        DimMagenta => tc.dim_magenta,
        DimCyan => tc.dim_cyan,
        DimWhite => tc.dim_white,
        BrightForeground => tc.bright_foreground,
        DimForeground => tc.dim_foreground,
    }
}

// =========================================================================
// Color conversion — any Color → Hsla
// =========================================================================

/// Convert an arbitrary terminal [`Color`] to Hsla.
///
/// Handles the three alacritty color variants:
/// - [`Named`](Color::Named) — theme palette
/// - [`Indexed`](Color::Indexed) — 256-color table
/// - [`Spec`](Color::Spec) — direct 24-bit RGB
pub fn cell_fg(c: Color, tc: &TerminalColors) -> Hsla {
    match c {
        Color::Named(n) => named_hsla(n, tc),
        Color::Indexed(i) => indexed_to_hsla(i),
        Color::Spec(r) => rgba_to_hsla(r),
    }
}

/// Convert a terminal background [`Color`] to Hsla.
///
/// Identical to [`cell_fg`] in conversion logic; separate function for
/// clarity at call sites.
pub fn cell_bg(c: Color, tc: &TerminalColors) -> Hsla {
    match c {
        Color::Named(n) => named_hsla(n, tc),
        Color::Indexed(i) => indexed_to_hsla(i),
        Color::Spec(r) => rgba_to_hsla(r),
    }
}

/// Whether a terminal color is an app-chosen value (not inherited default).
///
/// Returns `true` for explicit RGB or indexed colors set by the TUI
/// application, as opposed to theme-inherited Named foreground/background.
/// Used by the contrast engine to avoid over-correcting intentional colors.
pub fn is_explicit_color(c: Color) -> bool {
    matches!(c, Color::Spec(_) | Color::Indexed(_))
}

// =========================================================================
// RGB → Hsla
// =========================================================================

/// Convert an sRGB triple (0–255) to GPUI [`Hsla`].
pub(crate) fn rgba_to_hsla(r: Rgb) -> Hsla {
    gpui::Rgba {
        r: r.r as f32 / 255.0,
        g: r.g as f32 / 255.0,
        b: r.b as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

// =========================================================================
// 256-color indexed palette (XTerm)
// =========================================================================

/// Map a 256-color index (0–255) to Hsla using the standard XTerm palette.
///
/// - 0–15:   Standard VGA colors (re-uses the 16 NamedColor values).
/// - 16–231: 6×6×6 color cube.
/// - 232–255: Grayscale ramp.
fn indexed_to_hsla(index: u8) -> Hsla {
    match index {
        0..=15 => ansi16_to_hsla(index),
        16..=231 => color_cube(index - 16),
        232..=255 => gray_ramp(index - 232),
    }
}

/// 16 standard ANSI/VGA colors (0–7 normal, 8–15 bright).
fn ansi16_to_hsla(index: u8) -> Hsla {
    // Standard VGA palette — matches xterm defaults.
    let (r, g, b) = match index {
        0 => (0, 0, 0),         // Black
        1 => (205, 0, 0),       // Red
        2 => (0, 205, 0),       // Green
        3 => (205, 205, 0),     // Yellow
        4 => (0, 0, 238),       // Blue
        5 => (205, 0, 205),     // Magenta
        6 => (0, 205, 205),     // Cyan
        7 => (229, 229, 229),   // White
        8 => (127, 127, 127),   // Bright Black
        9 => (255, 0, 0),       // Bright Red
        10 => (0, 255, 0),      // Bright Green
        11 => (255, 255, 0),    // Bright Yellow
        12 => (92, 92, 255),    // Bright Blue
        13 => (255, 0, 255),    // Bright Magenta
        14 => (0, 255, 255),    // Bright Cyan
        15 => (255, 255, 255),  // Bright White
        _ => unreachable!(),
    };
    rgba_to_hsla(Rgb { r, g, b })
}

/// 6×6×6 RGB color cube (indices 16–231).
fn color_cube(offset: u8) -> Hsla {
    let r = cube_value(offset / 36);
    let g = cube_value((offset % 36) / 6);
    let b = cube_value(offset % 6);
    rgba_to_hsla(Rgb { r, g, b })
}

fn cube_value(index: u8) -> u8 {
    match index {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        5 => 255,
        _ => 0,
    }
}

/// Grayscale ramp (indices 232–255).
fn gray_ramp(step: u8) -> Hsla {
    let v = 8 + step * 10;
    rgba_to_hsla(Rgb { r: v, g: v, b: v })
}
