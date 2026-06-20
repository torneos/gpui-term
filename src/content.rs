//! Data types for terminal rendering.
//!
//! These types bridge alacritty's terminal state and GPUI's rendering system.

use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Cell as AlacrittyCell;
use alacritty_terminal::term::{RenderableCursor, TermMode};
use gpui::{Bounds, Pixels};

// =========================================================================
// TerminalBounds — pixel dimensions of the terminal grid
// =========================================================================

/// Pixel dimensions of a terminal grid.
#[derive(Clone, Copy, Debug)]
pub struct TerminalBounds {
    /// Width of a single character cell in pixels.
    pub cell_width: Pixels,
    /// Height of a single line in pixels.
    pub line_height: Pixels,
    /// The available pixel area for the grid.
    pub bounds: Bounds<Pixels>,
}

impl TerminalBounds {
    /// Create bounds from a full pixel rectangle and cell metrics.
    pub fn new(line_height: Pixels, cell_width: Pixels, bounds: Bounds<Pixels>) -> Self {
        Self {
            cell_width,
            line_height,
            bounds,
        }
    }

    /// Number of columns that fit in the available width.
    pub fn num_columns(&self) -> usize {
        (f32::from(self.bounds.size.width) / f32::from(self.cell_width))
            .floor()
            .max(1.0) as usize
    }

    /// Number of lines that fit in the available height.
    pub fn num_lines(&self) -> usize {
        (f32::from(self.bounds.size.height) / f32::from(self.line_height))
            .floor()
            .max(1.0) as usize
    }
}

// =========================================================================
// Content — renderable terminal state at a point in time
// =========================================================================

/// The complete renderable state of a terminal at a point in time.
///
/// Built by [`TerminalCore::build_content`](crate::core::TerminalCore::build_content)
/// from alacritty's [`RenderableContent`].
#[derive(Clone)]
pub struct Content {
    /// All visible cells with their grid positions.
    pub cells: Vec<(Point, AlacrittyCell)>,
    /// Current terminal mode flags (cursor, mouse, etc.).
    pub mode: TermMode,
    /// Scroll offset from the bottom of scrollback history.
    pub display_offset: usize,
    /// Current text selection range, if any.
    pub selection: Option<alacritty_terminal::selection::SelectionRange>,
    /// Plain-text content of the current selection.
    pub selection_text: Option<String>,
    /// Cursor position and shape.
    pub cursor: RenderableCursor,
    /// The character under the cursor.
    pub cursor_char: char,
    /// OSC 12 cursor colour (raw RGB). `None` = use theme default.
    pub cursor_color: Option<(u8, u8, u8)>,
    /// Pixel dimensions at capture time.
    pub bounds: TerminalBounds,
    /// Whether the viewport is at the top of scrollback.
    pub scrolled_to_top: bool,
    /// Whether the viewport is at the bottom (following output).
    pub scrolled_to_bottom: bool,
}

// Manual Debug impl since RenderableCursor doesn't derive Debug.
impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Content")
            .field("cells.len", &self.cells.len())
            .field("mode", &self.mode)
            .field("display_offset", &self.display_offset)
            .field("selection", &self.selection)
            .field("cursor_char", &self.cursor_char)
            .field("bounds", &self.bounds)
            .field("scrolled_to_top", &self.scrolled_to_top)
            .field("scrolled_to_bottom", &self.scrolled_to_bottom)
            .finish()
    }
}
