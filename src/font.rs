//! Cross-platform monospace font fallback and cell dimension computation.

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, Window, px};

/// Returns the best available monospace font family for the current OS.
///
/// Uses system fonts that are guaranteed to be installed.
pub fn default_monospace_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Menlo"
    }
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        "monospace"
    }
}

/// Compute terminal cell dimensions from font metrics.
///
/// `cell_width` is the advance width of 'm' (monospace font — all glyphs
/// have the same advance).  `line_height` is `font_size * line_height_ratio`.
///
/// Falls back to `font_size * 0.5` for cell width if advance lookup fails.
pub fn compute_cell_dimensions(
    window: &Window,
    font_family: SharedString,
    font_size_px: Pixels,
    line_height_ratio: f32,
) -> (Pixels, Pixels) {
    let font = Font {
        family: font_family,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
        features: FontFeatures::default(),
        fallbacks: None,
    };
    let fid = window.text_system().resolve_font(&font);
    let cell_width = window
        .text_system()
        .advance(fid, font_size_px, 'm')
        .map(|a| a.width)
        .unwrap_or_else(|_| px(f32::from(font_size_px) * 0.5));
    let line_height = px(f32::from(font_size_px) * line_height_ratio);
    (cell_width, line_height)
}
