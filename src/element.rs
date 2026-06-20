//! TerminalElement — GPUI element that paints a terminal grid.
//!
//! Converts alacritty terminal cells to batched text runs and background
//! rectangles, then paints them via GPUI's rendering primitives.

use gpui::{
    fill, point, px, relative, size, App, Bounds, CursorStyle, Element, ElementId, FocusHandle,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, LayoutId, Pixels, ShapedLine, SharedString, Size,
    StatefulInteractiveElement, Style, TextAlign, Window,
};
use std::hash::{Hash, Hasher};

use alacritty_terminal::vte::ansi::Color;

use crate::colors::TerminalColors;
use crate::content::{Content, TerminalBounds};
use crate::cursor::{self, CursorLayout};
use crate::entity::TerminalEntity;
use crate::highlight::{to_highlighted_range_lines, HighlightedRange};
use crate::painter::{self, BatchedTextRun, LayoutRect};

// =========================================================================
// TerminalElement
// =========================================================================

/// A GPUI element that renders a full terminal grid.
///
/// Handles font metrics, cell processing (batching), cursor, scrollbar,
/// mouse listeners, and selection highlights.
pub struct TerminalElement {
    /// The terminal backend (PTY or SSH).
    pub terminal: TerminalEntity,
    /// ANSI color palette.
    pub colors: TerminalColors,
    /// Font family name (e.g. "Fira Code").
    pub font_family: SharedString,
    /// Font size in pixels.
    pub font_size_px: Pixels,
    /// Line height multiplier (e.g. 1.2 = 120% of font size).
    pub line_height_ratio: f32,
    /// Focus handle for keyboard input.
    pub focus: FocusHandle,

    interactivity: Interactivity,
    focused: bool,

    cached_font_key: Option<(SharedString, Pixels, f32)>,
    cached_cell_width: Pixels,
    cached_line_height: Pixels,
    cached_bounds: Bounds<Pixels>,

    /// Content hash cache: skip expensive cell processing when idle.
    cached_content_hash: u64,
    /// Cached pre-processed content for idle-frame reuse.
    cached_layout: Option<CachedLayout>,
    /// True when resized but shell hasn't redrawn yet —
    /// skip cell rendering until content changes.
    resize_pending: bool,
}

/// Minimum interval between resize events (60fps).
// ── Cached layout for idle-frame skipping ──

#[derive(Clone)]
struct CachedLayout {
    runs: Vec<BatchedTextRun>,
    shaped: Vec<ShapedLine>,
    rects: Vec<LayoutRect>,
    highlighted_ranges: Vec<(alacritty_terminal::selection::SelectionRange, Hsla)>,
    cursor: Option<CursorLayout>,
    dimensions: TerminalBounds,
    display_offset: usize,
    total_lines: usize,
}

// ── Construction ──

impl TerminalElement {
    pub fn new(
        terminal: TerminalEntity,
        colors: TerminalColors,
        font_family: SharedString,
        font_size_px: Pixels,
        line_height_ratio: f32,
        focus: FocusHandle,
        focused: bool,
    ) -> Self {
        Self {
            terminal,
            colors,
            font_family,
            font_size_px,
            line_height_ratio,
            focus,
            interactivity: Interactivity::new(),
            focused,
            cached_font_key: None,
            cached_cell_width: px(8.),
            cached_line_height: px(16.),
            cached_bounds: Bounds::default(),
            cached_content_hash: 0,
            cached_layout: None,
            resize_pending: false,
        }
    }

    // ── Font Metrics ──

    fn font_metrics(&mut self, w: &Window) -> (Pixels, Pixels) {
        let key = (
            self.font_family.clone(),
            self.font_size_px,
            self.line_height_ratio,
        );
        if self.cached_font_key.as_ref() == Some(&key) {
            return (self.cached_cell_width, self.cached_line_height);
        }
        let (cw, lh) = crate::font::compute_cell_dimensions(
            w,
            self.font_family.clone(),
            self.font_size_px,
            self.line_height_ratio,
        );
        self.cached_font_key = Some(key);
        self.cached_cell_width = cw;
        self.cached_line_height = lh;
        (cw, lh)
    }

    // ── Decorative character check ──

    fn is_decorative_character(ch: char) -> bool {
        matches!(
            ch as u32,
            0x2500..=0x257F
                | 0x2580..=0x259F
                | 0x25A0..=0x25FF
                | 0xE0B0..=0xE0B7
                | 0xE0B8..=0xE0BF
                | 0xE0C0..=0xE0CA
                | 0xE0CC..=0xE0D7
        )
    }

    // ── Content hash ──

    fn compute_hash(&self, content: &Content, search_match_count: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        content.cells.len().hash(&mut h);
        content.cursor.point.line.0.hash(&mut h);
        content.cursor.point.column.0.hash(&mut h);
        (content.cursor.shape as u8).hash(&mut h);
        content.display_offset.hash(&mut h);
        search_match_count.hash(&mut h);
        if let Some(ref sel) = content.selection {
            sel.start.line.0.hash(&mut h);
            sel.start.column.0.hash(&mut h);
            sel.end.line.0.hash(&mut h);
            sel.end.column.0.hash(&mut h);
        }
        // Hash full cell data: character + flags + colors + hyperlink.
        for (_, cell) in &content.cells {
            (cell.c as u32).hash(&mut h);
            cell.flags.bits().hash(&mut h);
            cell.hyperlink().is_some().hash(&mut h);
            hash_color(&cell.fg, &mut h);
            hash_color(&cell.bg, &mut h);
        }
        h.finish()
    }

    // ── Cursor building ──

    #[allow(clippy::too_many_arguments)]
    fn build_cursor(
        &self,
        content: &Content,
        rows: usize,
        cols: usize,
        cell_width: Pixels,
        line_height: Pixels,
        origin: gpui::Point<Pixels>,
        w: &mut Window,
    ) -> Option<CursorLayout> {
        let cur = content.cursor;
        if matches!(
            cur.shape,
            alacritty_terminal::vte::ansi::CursorShape::Hidden
        ) {
            return None;
        }
        let cursor_line = (cur.point.line.0 + content.display_offset as i32) as usize;
        let cursor_col = cur.point.column.0;
        if cursor_line >= rows || cursor_col >= cols {
            return None;
        }
        let vp_point = alacritty_terminal::index::Point::new(
            alacritty_terminal::index::Line(cursor_line as i32),
            alacritty_terminal::index::Column(cursor_col),
        );

        let cursor_color = if let Some((r, g, b)) = content.cursor_color {
            crate::colors::rgba_to_hsla(alacritty_terminal::vte::ansi::Rgb { r, g, b })
        } else if self.focused {
            self.colors.bright_foreground
        } else {
            self.colors.dim_foreground
        };
        let cursor_text_color = if self.focused {
            self.colors.background
        } else {
            self.colors.bright_foreground
        };
        let mut cl = cursor::compute_cursor(
            vp_point,
            cur.shape,
            content.cursor_char,
            cell_width,
            line_height,
            origin,
            self.font_family.clone(),
            w,
            cursor_text_color,
        );
        cl.color = cursor_color;
        if !self.focused {
            cl.shape = alacritty_terminal::vte::ansi::CursorShape::HollowBlock;
        }
        Some(cl)
    }

    // ── Highlighted ranges ──

    fn compute_highlighted_ranges(
        layout: &LayoutState,
        origin: gpui::Point<Pixels>,
    ) -> Vec<HighlightedRange> {
        let mut result = Vec::new();
        for (range, color) in &layout.highlighted_ranges {
            if let Some((start_y, lines)) = to_highlighted_range_lines(
                range.start,
                range.end,
                layout.display_offset,
                layout.dimensions.num_columns(),
                layout.dimensions.num_lines(),
                layout.dimensions.cell_width,
                layout.dimensions.line_height,
                origin,
            ) {
                result.push(HighlightedRange {
                    start_y,
                    line_height: layout.dimensions.line_height,
                    lines,
                    color: *color,
                });
            }
        }
        result
    }
}

// =========================================================================
// GPUI Element trait
// =========================================================================

/// Layout state built in prepaint, consumed in paint.
pub struct LayoutState {
    hitbox: Hitbox,
    scrollbar_hitbox: Hitbox,
    cell_width: Pixels,
    line_height: Pixels,
    runs: Vec<BatchedTextRun>,
    shaped: Vec<ShapedLine>,
    rects: Vec<LayoutRect>,
    bg_color: Hsla,
    cursor: Option<CursorLayout>,
    highlighted_ranges: Vec<(alacritty_terminal::selection::SelectionRange, Hsla)>,
    dimensions: TerminalBounds,
    display_offset: usize,
    total_lines: usize,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = LayoutState;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::from((
            "term",
            self.terminal.entity_id().as_u64(),
        )))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        w: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = w.request_layout(
            Style {
                size: Size {
                    width: relative(1.).into(),
                    height: relative(1.).into(),
                },
                ..Default::default()
            },
            None,
            cx,
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        w: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let fpx = self.font_size_px;
        let (cw, lh) = self.font_metrics(w);

        // Resize grid immediately — SIGWINCH debounce is in PtyTerminal/SshTerminal
        if self.cached_bounds != bounds {
            self.cached_bounds = bounds;
            self.cached_content_hash = 0;
            self.cached_layout = None;
            self.terminal
                .set_size(TerminalBounds::new(lh, cw, bounds), cx);
            // Skip rendering until shell redraws (not on initial resize)
            self.resize_pending = bounds != Bounds::default();
        }

        self.terminal.sync(w, cx);

        // Content hash — skip processing if unchanged.
        let content = self.terminal.last_content(cx);
        let hash = self.compute_hash(&content, self.terminal.matches_count(cx));

        // During resize: skip cell rendering until shell outputs new content.
        if self.resize_pending {
            if self.terminal.had_sync_output(cx) {
                self.resize_pending = false;
            }
        }

        if self.resize_pending {
            let hitbox = w.insert_hitbox(bounds, HitboxBehavior::Normal);
            let sb = Bounds::new(
                point(
                    bounds.origin.x + bounds.size.width - px(crate::scrollbar::SCROLLBAR_WIDTH),
                    bounds.origin.y,
                ),
                size(px(crate::scrollbar::SCROLLBAR_WIDTH), bounds.size.height),
            );
            let scrollbar_hitbox = w.insert_hitbox(sb, HitboxBehavior::Normal);
            return LayoutState {
                hitbox,
                scrollbar_hitbox,
                cell_width: cw,
                line_height: lh,
                runs: Vec::new(),
                shaped: Vec::new(),
                rects: Vec::new(),
                bg_color: self.colors.background,
                cursor: None,
                highlighted_ranges: Vec::new(),
                dimensions: TerminalBounds::new(lh, cw, bounds),
                display_offset: 0,
                total_lines: 0,
            };
        }

        if hash == self.cached_content_hash {
            if let Some(ref cached) = self.cached_layout {
                let hitbox = w.insert_hitbox(bounds, HitboxBehavior::Normal);
                let sb = Bounds::new(
                    point(
                        bounds.origin.x + bounds.size.width - px(crate::scrollbar::SCROLLBAR_WIDTH),
                        bounds.origin.y,
                    ),
                    size(px(crate::scrollbar::SCROLLBAR_WIDTH), bounds.size.height),
                );
                let scrollbar_hitbox = w.insert_hitbox(sb, HitboxBehavior::Normal);
                return LayoutState {
                    hitbox,
                    scrollbar_hitbox,
                    cell_width: cw,
                    line_height: lh,
                    runs: cached.runs.clone(),
                    shaped: cached.shaped.clone(),
                    rects: cached.rects.clone(),
                    bg_color: self.colors.background,
                    cursor: cached.cursor.clone(),
                    highlighted_ranges: cached.highlighted_ranges.clone(),
                    dimensions: cached.dimensions,
                    display_offset: cached.display_offset,
                    total_lines: cached.total_lines,
                };
            }
        }
        self.cached_content_hash = hash;

        let total_lines_from_term = self.terminal.total_lines(cx);

        let cols = content.bounds.num_columns().max(1);
        let rows = content.bounds.num_lines().max(1);
        let total_lines = total_lines_from_term.max(rows);

        // Build highlighted ranges from search matches and selection.
        let mut highlighted_ranges: Vec<(alacritty_terminal::selection::SelectionRange, Hsla)> =
            Vec::new();
        // Search matches
        let matches = self.terminal.matches_clone(cx);
        if !matches.is_empty() {
            let mut match_color = self.colors.bright_foreground;
            match_color.a *= 0.5;
            for range in &matches {
                highlighted_ranges.push((*range, match_color));
            }
        }
        // Selection
        if let Some(sel) = &content.selection {
            let mut sel_color = self.colors.bright_foreground;
            sel_color.a *= 0.3;
            highlighted_ranges.push((*sel, sel_color));
        }

        let default_fg = self.colors.foreground;
        let font_family_name = self.font_family.clone();

        let (runs, rects) = painter::process_cells(
            &content.cells,
            cols,
            rows,
            &self.colors,
            &font_family_name,
            fpx,
            default_fg,
            Self::is_decorative_character,
        );

        let shaped: Vec<ShapedLine> = runs
            .iter()
            .map(|run| {
                w.text_system().shape_line(
                    run.text.clone(),
                    run.font_size,
                    std::slice::from_ref(&run.style),
                    None,
                )
            })
            .collect();

        let cursor = self.build_cursor(&content, rows, cols, cw, lh, bounds.origin, w);

        let hitbox = w.insert_hitbox(bounds, HitboxBehavior::Normal);
        let sb = Bounds::new(
            point(
                bounds.origin.x + bounds.size.width - px(crate::scrollbar::SCROLLBAR_WIDTH),
                bounds.origin.y,
            ),
            size(px(crate::scrollbar::SCROLLBAR_WIDTH), bounds.size.height),
        );
        let scrollbar_hitbox = w.insert_hitbox(sb, HitboxBehavior::Normal);

        self.cached_layout = Some(CachedLayout {
            runs: runs.clone(),
            shaped: shaped.clone(),
            rects: rects.clone(),
            highlighted_ranges: highlighted_ranges.clone(),
            cursor: cursor.clone(),
            dimensions: content.bounds,
            display_offset: content.display_offset,
            total_lines,
        });

        LayoutState {
            hitbox,
            scrollbar_hitbox,
            cell_width: cw,
            line_height: lh,
            runs,
            shaped,
            rects,
            bg_color: self.colors.background,
            cursor,
            highlighted_ranges,
            dimensions: content.bounds,
            display_offset: content.display_offset,
            total_lines,
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        layout: &mut Self::PrepaintState,
        w: &mut Window,
        cx: &mut App,
    ) {
        let hitbox = layout.hitbox.clone();
        let scrollbar_hitbox = layout.scrollbar_hitbox.clone();
        let origin = w
            .pixel_snap_bounds(Bounds::new(bounds.origin, size(Pixels::ZERO, Pixels::ZERO)))
            .origin;

        // Background fill.
        w.paint_quad(fill(bounds, layout.bg_color));

        // Colored cell backgrounds.
        for r in &layout.rects {
            w.paint_quad(fill(
                Bounds::new(
                    point(
                        origin.x + r.point.column as f32 * layout.cell_width,
                        origin.y + r.point.line as f32 * layout.line_height,
                    ),
                    size(
                        r.num_of_cells as f32 * layout.cell_width,
                        layout.line_height,
                    ),
                ),
                r.color,
            ));
        }

        // Selection/search highlights.
        let highlighted = Self::compute_highlighted_ranges(layout, origin);
        for hr in &highlighted {
            hr.paint(bounds, w);
        }

        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            Some(&hitbox),
            w,
            cx,
            |_, w, cx| {
                // Cursor.
                if let Some(ref cur) = layout.cursor {
                    cur.paint(w, cx);
                }

                // Text runs.
                for (run, shaped) in layout.runs.iter().zip(layout.shaped.iter()) {
                    let pos = point(
                        origin.x + run.start_point.column as f32 * layout.cell_width,
                        origin.y + run.start_point.line as f32 * layout.line_height,
                    );
                    let _ = shaped.paint(pos, layout.line_height, TextAlign::Left, None, w, cx);
                }
            },
        );

        // Mouse listeners — outside interactivity.paint(), matching pulse-render.
        {
            let terminal = self.terminal.clone();
            let focus = self.focus.clone();
            let mode = self.terminal.mode(cx);
            crate::mouse::register_mouse_listeners(terminal, focus, mode, &hitbox, w);
        }

        // Scrollbar.
        crate::scrollbar::render_scrollbar(
            bounds,
            layout.bg_color,
            layout.display_offset,
            layout.total_lines,
            layout.dimensions.num_lines(),
            w,
        );

        // Cursor style.
        w.set_cursor_style(CursorStyle::IBeam, &hitbox);
        if layout.total_lines > layout.dimensions.num_lines() {
            w.set_cursor_style(CursorStyle::Arrow, &scrollbar_hitbox);
        }
    }
}

impl InteractiveElement for TerminalElement {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl StatefulInteractiveElement for TerminalElement {}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

// ── Helpers ──

fn hash_color(color: &Color, h: &mut impl Hasher) {
    std::mem::discriminant(color).hash(h);
    match color {
        Color::Named(n) => {
            std::mem::discriminant(n).hash(h);
        }
        Color::Spec(rgb) => {
            rgb.r.hash(h);
            rgb.g.hash(h);
            rgb.b.hash(h);
        }
        Color::Indexed(i) => {
            i.hash(h);
        }
    }
}
