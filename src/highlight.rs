//! Highlight rendering — search matches and text selection.

use alacritty_terminal::index::Point;
use gpui::{Bounds, Hsla, Pixels, Window, fill, point, size};

/// Layout for a single highlighted range line.
#[derive(Debug, Clone)]
pub(crate) struct HighlightedRangeLine {
    pub start_x: Pixels,
    pub end_x: Pixels,
}

/// A highlighted range (selection or search match) to paint.
#[derive(Debug, Clone)]
pub(crate) struct HighlightedRange {
    pub start_y: Pixels,
    pub line_height: Pixels,
    pub lines: Vec<HighlightedRangeLine>,
    pub color: Hsla,
}

impl HighlightedRange {
    pub fn paint(&self, _bounds: Bounds<Pixels>, window: &mut Window) {
        for (ix, line) in self.lines.iter().enumerate() {
            let y = self.start_y + ix as f32 * self.line_height;
            let x = line.start_x;
            let w = line.end_x - line.start_x;
            if w > Pixels::ZERO {
                window.paint_quad(fill(
                    Bounds::new(point(x, y), size(w, self.line_height)),
                    self.color,
                ));
            }
        }
    }
}

/// Convert a terminal point range to viewport-relative highlighted lines.
///
/// `display_offset` is the current scroll position in lines;
/// `num_columns` and `num_lines` are the visible viewport dimensions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn to_highlighted_range_lines(
    start: Point,
    end: Point,
    display_offset: usize,
    num_columns: usize,
    num_lines: usize,
    cell_width: Pixels,
    line_height: Pixels,
    origin: gpui::Point<Pixels>,
) -> Option<(Pixels, Vec<HighlightedRangeLine>)> {
    // Normalize: ensure start <= end
    let (norm_start, norm_end) = if start > end { (end, start) } else { (start, end) };

    let offset: i32 = display_offset.try_into().unwrap_or(i32::MAX);

    let start_line = norm_start.line.0.saturating_add(offset);
    let start_col = norm_start.column.0 as i32;
    let end_line = norm_end.line.0.saturating_add(offset);
    let end_col = norm_end.column.0 as i32;

    if end_line < 0 || start_line > num_lines as i32 {
        return None;
    }

    let clamped_start = start_line.max(0) as usize;
    let clamped_end = end_line.min(num_lines as i32) as usize;

    let start_y = origin.y + clamped_start as f32 * line_height;

    let mut lines = Vec::new();
    for line in clamped_start..=clamped_end {
        let mut l_start: i32 = 0;
        let mut l_end = num_columns as i32;

        if line == clamped_start && start_line >= 0 {
            l_start = start_col;
        }
        if line == clamped_end && end_line <= num_lines as i32 {
            l_end = end_col + 1;
        }

        if l_start > l_end {
            std::mem::swap(&mut l_start, &mut l_end);
        }

        lines.push(HighlightedRangeLine {
            start_x: origin.x + l_start as f32 * cell_width,
            end_x: origin.x + l_end as f32 * cell_width,
        });
    }

    Some((start_y, lines))
}
