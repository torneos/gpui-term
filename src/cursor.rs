//! Cursor rendering — shape-aware block, underline, bar, and hollow cursors.

use alacritty_terminal::index::Point;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui::{
    App, BorderStyle, Bounds, Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels,
    ShapedLine, SharedString, TextAlign, Window, fill, outline, point, px, size,
};

/// Width of the bar cursor (I-beam line).
const CURSOR_BAR_WIDTH: f32 = 2.0;
/// Vertical offset for underline cursor from bottom.
const CURSOR_UNDERLINE_OFFSET: f32 = 2.0;
/// Text scale factor inside block cursor.
const CURSOR_TEXT_SCALE: f32 = 0.8;

/// Cursor layout with shape-specific rendering.
#[derive(Clone)]
pub struct CursorLayout {
    pub origin: gpui::Point<Pixels>,
    pub block_width: Pixels,
    pub line_height: Pixels,
    pub color: Hsla,
    pub shape: CursorShape,
    pub block_text: Option<ShapedLine>,
}

impl CursorLayout {
    fn bounds(&self) -> Bounds<Pixels> {
        match self.shape {
            CursorShape::Beam => Bounds {
                origin: self.origin,
                size: size(px(CURSOR_BAR_WIDTH), self.line_height),
            },
            CursorShape::Underline => Bounds {
                origin: self.origin
                    + gpui::Point::new(
                        Pixels::ZERO,
                        self.line_height - px(CURSOR_UNDERLINE_OFFSET),
                    ),
                size: size(self.block_width, px(CURSOR_UNDERLINE_OFFSET)),
            },
            CursorShape::Block | CursorShape::HollowBlock => Bounds {
                origin: self.origin,
                size: size(self.block_width, self.line_height),
            },
            CursorShape::Hidden => Bounds {
                origin: self.origin,
                size: size(Pixels::ZERO, Pixels::ZERO),
            },
        }
    }

    pub fn paint(&self, window: &mut Window, cx: &mut App) {
        if matches!(self.shape, CursorShape::Hidden) {
            return;
        }
        let bounds = self.bounds();

        let quad = if matches!(self.shape, CursorShape::HollowBlock) {
            outline(bounds, self.color, BorderStyle::Solid)
        } else {
            fill(bounds, self.color)
        };
        window.paint_quad(quad);

        if let Some(block_text) = &self.block_text {
            let _ = block_text.paint(
                self.origin,
                self.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
}

/// Compute cursor layout with shape-specific rendering.
///
/// Does NOT set the color or adjust shape for unfocused — caller is
/// responsible for focus-aware overrides.
#[allow(clippy::too_many_arguments)]
pub fn compute_cursor(
    cursor_point: Point,
    cursor_shape: CursorShape,
    cursor_char: char,
    cell_width: Pixels,
    line_height: Pixels,
    origin: gpui::Point<Pixels>,
    font_family: SharedString,
    window: &mut Window,
    text_color: Hsla,
) -> CursorLayout {
    let o = point(
        origin.x + cursor_point.column.0 as f32 * cell_width,
        origin.y + cursor_point.line.0 as f32 * line_height,
    );

    let block_text =
        if matches!(cursor_shape, CursorShape::Block | CursorShape::HollowBlock)
            && !cursor_char.is_whitespace()
        {
            let font = Font {
                family: font_family,
                weight: FontWeight::BOLD,
                style: FontStyle::Normal,
                features: FontFeatures::default(),
                fallbacks: None,
            };
            let text_run = gpui::TextRun {
                len: cursor_char.len_utf8(),
                font,
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            Some(window.text_system().shape_line(
                cursor_char.to_string().into(),
                line_height * CURSOR_TEXT_SCALE,
                &[text_run],
                Some(cell_width),
            ))
        } else {
            None
        };

    CursorLayout {
        origin: o,
        block_width: cell_width,
        line_height,
        color: Hsla::default(),
        block_text,
        shape: cursor_shape,
    }
}
