//! Scrollbar rendering — vertical indicator for terminal scroll position.

use gpui::{Bounds, Hsla, Pixels, Window, fill, point, px, size};

use crate::contrast;

// =========================================================================
// Constants
// =========================================================================

/// Width of the scrollbar track and thumb in pixels.
pub const SCROLLBAR_WIDTH: f32 = 8.0;
/// Minimum thumb height in pixels (prevents thumb from vanishing).
const SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 20.0;
/// Alpha multiplier for scrollbar color relative to the background.
const SCROLLBAR_ALPHA: f32 = 0.4;

/// Render a vertical scrollbar at the right edge of the terminal.
pub fn render_scrollbar(
    bounds: Bounds<Pixels>,
    bg_color: Hsla,
    display_offset: usize,
    total_lines: usize,
    visible_lines: usize,
    window: &mut Window,
) {
    if total_lines <= visible_lines {
        return;
    }
    let bar_w = px(SCROLLBAR_WIDTH);
    let track_h = bounds.size.height;

    let total = total_lines.max(1) as f32;
    let visible = visible_lines as f32;
    let scroll_offset = display_offset as f32;

    let thumb_h = (visible / total * track_h.as_f32()).max(SCROLLBAR_MIN_THUMB_HEIGHT);
    let thumb_y = if total > visible {
        let max_offset = total - visible;
        (max_offset - scroll_offset) / max_offset * (track_h.as_f32() - thumb_h)
    } else {
        0.0
    };

    let mut bar_color = bg_color;
    bar_color.a *= SCROLLBAR_ALPHA;
    bar_color = contrast::ensure_minimum_contrast(bar_color, bg_color, 10.0);

    window.paint_quad(fill(
        Bounds::new(
            point(
                bounds.origin.x + bounds.size.width - bar_w,
                bounds.origin.y + px(thumb_y),
            ),
            size(bar_w, px(thumb_h)),
        ),
        bar_color,
    ));
}
