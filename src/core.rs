//! Shared terminal state machine — used by both [`PtyTerminal`](crate::PtyTerminal)
//! and [`SshTerminal`](crate::SshTerminal).
//!
//! [`TerminalCore`] owns the alacritty [`Term`], ANSI [`Processor`], input buffer,
//! and cached [`Content`]. It implements all terminal logic (mouse, scroll, keyboard,
//! content building) in one place.  I/O is left to the callers via the
//! [`MouseResult`] / [`ScrollResult`] return types.

use std::collections::VecDeque;
use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use gpui::{
    Bounds, Modifiers, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
};

use crate::content::{Content, TerminalBounds};
use crate::keys;

// =========================================================================
// Shared types
// =========================================================================

/// Event listener that forwards alacritty side-band events (title, bell,
/// pty_write, OSC 52, etc.) to [`TerminalCore`] via an mpsc channel.
pub struct CoreEventListener {
    event_tx: mpsc::Sender<Event>,
}

impl EventListener for CoreEventListener {
    fn send_event(&self, event: Event) {
        let _ = self.event_tx.send(event);
    }
}

// ── Dimensions adapter ──

pub struct CoreDimensions {
    pub columns: usize,
    pub screen_lines: usize,
}

impl CoreDimensions {
    pub fn from_bounds(bounds: TerminalBounds) -> Self {
        Self {
            columns: bounds.num_columns().max(1),
            screen_lines: bounds.num_lines().max(1),
        }
    }
}

impl Dimensions for CoreDimensions {
    fn columns(&self) -> usize { self.columns }
    fn screen_lines(&self) -> usize { self.screen_lines }
    fn total_lines(&self) -> usize { self.screen_lines }
}

// ── Result types — the caller sends `report_bytes` to PTY / SSH ──

/// Result of a mouse operation on the terminal grid.
///
/// The caller sends `report_bytes` to the PTY/SSH backend when mouse
/// reporting is active, and triggers a repaint when `needs_paint` is set.
pub struct MouseResult {
    /// Escape-sequence bytes for mouse-reporting mode (None = no reporting).
    pub report_bytes: Option<Vec<u8>>,
    /// The content was rebuilt and a repaint is needed.
    pub needs_paint: bool,
}

/// Result of a scroll-wheel operation.
///
/// Same pattern as [`MouseResult`]: `report_bytes` for mouse-mode reporting,
/// `needs_paint` for content rebuilds.
pub struct ScrollResult {
    /// Escape-sequence bytes for scroll reporting (None = no reporting).
    pub report_bytes: Option<Vec<u8>>,
    /// The content was rebuilt and a repaint is needed.
    pub needs_paint: bool,
}

// =========================================================================
// Coordinate helpers
// =========================================================================

fn pixel_to_col(x: Pixels, origin_x: Pixels, cell_width: Pixels) -> usize {
    ((f32::from(x) - f32::from(origin_x)) / f32::from(cell_width))
        .floor()
        .max(0.0) as usize
}

/// Returns 0-based line index (signed for scrollback).
fn pixel_to_line(y: Pixels, origin_y: Pixels, line_height: Pixels) -> i32 {
    ((f32::from(y) - f32::from(origin_y)) / f32::from(line_height))
        .floor()
        .max(0.0) as i32
}

/// 1-based column for mouse-reporting protocols.
fn pixel_to_col_1based(x: Pixels, origin_x: Pixels, cell_width: Pixels) -> usize {
    pixel_to_col(x, origin_x, cell_width) + 1
}

/// 1-based line for mouse-reporting protocols.
fn pixel_to_line_1based(y: Pixels, origin_y: Pixels, line_height: Pixels) -> usize {
    pixel_to_line(y, origin_y, line_height).max(0) as usize + 1
}

// ── Mouse encoding ──

fn x10_mouse_encode(btn: u8, col: usize, line: usize) -> [u8; 6] {
    let clamp = |v: usize| (v.min(222) as u8).saturating_add(32);
    [0x1b, b'[', b'M', (btn.saturating_add(32)), clamp(col), clamp(line)]
}

fn sgr_mouse_encode(btn: u8, col: usize, line: usize, release: bool) -> Vec<u8> {
    let suffix = if release { "m" } else { "M" };
    format!("\x1b[<{btn};{col};{line}{suffix}").into_bytes()
}

fn utf8_mouse_encode(btn: u8, col: usize, line: usize, release: bool) -> Vec<u8> {
    let mut bytes = vec![0x1b, b'[', b'M'];
    let btn_code = if release { 3 } else { btn }.saturating_add(32);
    bytes.push(btn_code);
    let encode_utf8 = |v: usize| -> [u8; 2] {
        let v = v.min(2015);
        [0xC0 | (v >> 6) as u8, 0x80 | (v & 0x3F) as u8]
    };
    let cx = encode_utf8(col);
    let cy = encode_utf8(line);
    bytes.extend(cx);
    bytes.extend(cy);
    bytes
}

fn mouse_report_bytes(btn: u8, col: usize, line: usize, release: bool, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::SGR_MOUSE) {
        sgr_mouse_encode(btn, col, line, release)
    } else if mode.contains(TermMode::UTF8_MOUSE) {
        utf8_mouse_encode(btn, col, line, release)
    } else {
        x10_mouse_encode(btn, col, line).to_vec()
    }
}

fn mouse_btn_code(button: gpui::MouseButton) -> Option<u8> {
    match button {
        gpui::MouseButton::Left => Some(0),
        gpui::MouseButton::Middle => Some(1),
        gpui::MouseButton::Right => Some(2),
        gpui::MouseButton::Navigate(_) => None,
    }
}

fn in_mouse_mode(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
}

fn in_mouse_drag_mode(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
}

fn scroll_should_report(mode: TermMode) -> bool {
    in_mouse_mode(mode)
        || (mode.contains(TermMode::ALTERNATE_SCROLL) && mode.contains(TermMode::ALT_SCREEN))
}

fn selection_type_from_click(click_count: usize, mods: &Modifiers) -> SelectionType {
    if mods.control && mods.alt {
        return SelectionType::Block;
    }
    match click_count {
        2 => SelectionType::Semantic,
        3.. => SelectionType::Lines,
        _ => SelectionType::Simple,
    }
}

// =========================================================================
// TerminalCore
// =========================================================================

/// Shared terminal state machine — used by both [`PtyTerminal`](crate::PtyTerminal)
/// and [`SshTerminal`](crate::SshTerminal).
///
/// Holds the alacritty [`Term`], ANSI [`Processor`], input buffer, content
/// cache, and event channels. All terminal-logic methods (mouse, scroll,
/// keyboard, content building) live here so both backends share one
/// implementation. PTY/SSH I/O is handled by the wrapper entities.
pub struct TerminalCore {
    /// The alacritty terminal grid and state.
    pub term: Term<CoreEventListener>,
    /// ANSI/VT escape-sequence processor.
    pub processor: Processor,
    /// Buffered incoming bytes not yet fed to the processor.
    pub incoming: VecDeque<u8>,
    /// Receiver for the PTY/SSH output channel.
    pub rx: mpsc::Receiver<Vec<u8>>,
    /// Most recent renderable content snapshot.
    pub cached_content: Content,
    /// Total lines including scrollback history.
    pub total_lines: usize,

    // Event listener channels
    event_rx: mpsc::Receiver<Event>,
    pending_events: Vec<Event>,

    // Accumulated state from events
    /// OSC 0/2 window title (updated each frame).
    pub title_text: String,
    /// True if a BEL was received since the last read.
    pub bell_pending: bool,
    /// True if the last [`sync`](Self::sync) processed new output.
    pub had_output: bool,
    pty_write_queue: Vec<String>,
}

impl TerminalCore {
    // ── Construction ──

    /// Create a new terminal state machine with the given grid dimensions
    /// and a receive channel for PTY/SSH output.
    pub fn new(bounds: TerminalBounds, rx: mpsc::Receiver<Vec<u8>>) -> Self {
        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let listener = CoreEventListener { event_tx };
        let config = Config {
            osc52: Osc52::CopyPaste,
            ..Config::default()
        };
        let size = CoreDimensions::from_bounds(bounds);
        let term = Term::new(config, &size, listener);
        let processor = Processor::new();
        let cached_content = Self::build_content(&term, bounds);
        Self {
            term,
            processor,
            incoming: VecDeque::new(),
            rx,
            cached_content,
            total_lines: bounds.num_lines(),
            event_rx,
            pending_events: Vec::new(),
            title_text: String::new(),
            bell_pending: false,
            had_output: false,
            pty_write_queue: Vec::new(),
        }
    }

    // ── Sync ──

    /// Drain the incoming channel, feed the ANSI processor, rebuild content.
    /// Returns `true` if new output was processed.
    pub fn sync(&mut self) -> bool {
        // Drain PTY output
        while let Ok(bytes) = self.rx.try_recv() {
            self.incoming.extend(bytes);
        }
        let chunk: Vec<u8> = self.incoming.drain(..).collect();
        if !chunk.is_empty() {
            self.processor.advance(&mut self.term, &chunk);
            self.total_lines = self.term.grid().total_lines();
            self.cached_content =
                Self::build_content(&self.term, self.cached_content.bounds);
        }

        // Drain event listener events
        self.had_output = !chunk.is_empty();
        while let Ok(event) = self.event_rx.try_recv() {
            self.had_output = true;
            match event {
                Event::Title(title) => { self.title_text = title; }
                Event::ResetTitle => { self.title_text.clear(); }
                Event::Bell => { self.bell_pending = true; }
                Event::PtyWrite(text) => { self.pty_write_queue.push(text); }
                Event::ColorRequest(index, formatter) => {
                    let colors = self.term.colors();
                    if let Some(color) = &colors[index] {
                        self.pty_write_queue.push(formatter(*color));
                    }
                }
                _ => { self.pending_events.push(event); }
            }
        }

        self.had_output
    }

    /// Take accumulated PtyWrite strings (DA responses, DSR, color queries, etc.)
    /// to be sent to the PTY/SSH backend.
    pub fn drain_pty_writes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pty_write_queue)
    }

    /// Take accumulated pending events (clipboard, cursor blink, etc.) for
    /// the caller to handle with GPUI context.
    pub fn take_pending_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
    }

    // ── Resize ──

    /// Resize the terminal grid to new pixel bounds.
    pub fn set_size(&mut self, bounds: TerminalBounds) {
        let size = CoreDimensions::from_bounds(bounds);
        self.term.resize(size);
        self.total_lines = self.term.grid().total_lines();
        self.cached_content = Self::build_content(&self.term, bounds);
    }

    // ── Content building ──

    fn build_content(term: &Term<CoreEventListener>, bounds: TerminalBounds) -> Content {
        let rc = term.renderable_content();
        let cells: Vec<_> = rc
            .display_iter
            .map(|ic| (ic.point, ic.cell.clone()))
            .collect();
        let cursor_char = term.grid()[rc.cursor.point].c;
        let cursor_color = term.colors()[alacritty_terminal::vte::ansi::NamedColor::Cursor]
            .map(|rgb| (rgb.r, rgb.g, rgb.b));
        Content {
            cells,
            mode: rc.mode,
            display_offset: rc.display_offset,
            selection: rc.selection,
            selection_text: term.selection_to_string(),
            cursor: rc.cursor,
            cursor_char,
            cursor_color,
            bounds,
            scrolled_to_top: rc.display_offset == term.history_size(),
            scrolled_to_bottom: rc.display_offset == 0,
        }
    }

    // ── Mouse: down ──

    /// Mouse button pressed — starts a selection or sends a mouse report.
    pub fn mouse_down(&mut self, event: &MouseDownEvent, hitbox: Bounds<Pixels>) -> MouseResult {
        let mode = self.mode();
        let cell_w = self.cached_content.bounds.cell_width;
        let line_h = self.cached_content.bounds.line_height;
        let origin = hitbox.origin;

        if in_mouse_mode(mode) {
            let btn = match mouse_btn_code(event.button) {
                Some(b) => b,
                None => return MouseResult { report_bytes: None, needs_paint: false },
            };
            let col = pixel_to_col_1based(event.position.x, origin.x, cell_w);
            let line = pixel_to_line_1based(event.position.y, origin.y, line_h);
            let bytes = mouse_report_bytes(btn, col, line, false, mode);
            return MouseResult { report_bytes: Some(bytes), needs_paint: false };
        }

        let col = pixel_to_col(event.position.x, origin.x, cell_w);
        let line = pixel_to_line(event.position.y, origin.y, line_h);
        let point = alacritty_terminal::index::Point::new(
            alacritty_terminal::index::Line(line),
            alacritty_terminal::index::Column(col),
        );
        let sel_type = selection_type_from_click(event.click_count, &event.modifiers);
        self.term.selection = Some(Selection::new(
            sel_type,
            point,
            alacritty_terminal::index::Side::Left,
        ));
        self.cached_content =
            Self::build_content(&self.term, self.cached_content.bounds);
        MouseResult { report_bytes: None, needs_paint: true }
    }

    // ── Mouse: drag ──

    /// Mouse moved — updates selection or sends motion report (mode 1003).
    pub fn mouse_drag(&mut self, event: &MouseMoveEvent, hitbox: Bounds<Pixels>) -> MouseResult {
        let mode = self.mode();
        let cell_w = self.cached_content.bounds.cell_width;
        let line_h = self.cached_content.bounds.line_height;
        let origin = hitbox.origin;

        if in_mouse_drag_mode(mode) {
            if let Some(button) = event.pressed_button {
                let btn = match mouse_btn_code(button) {
                    Some(b) => b | 32,
                    None => return MouseResult { report_bytes: None, needs_paint: false },
                };
                let col = pixel_to_col_1based(event.position.x, origin.x, cell_w);
                let line = pixel_to_line_1based(event.position.y, origin.y, line_h);
                let bytes = mouse_report_bytes(btn, col, line, false, mode);
                return MouseResult { report_bytes: Some(bytes), needs_paint: false };
            }
        }

        if let Some(ref mut sel) = self.term.selection {
            let col = pixel_to_col(event.position.x, origin.x, cell_w);
            let line = pixel_to_line(event.position.y, origin.y, line_h);
            let point = alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(line),
                alacritty_terminal::index::Column(col),
            );
            sel.update(point, alacritty_terminal::index::Side::Right);
            self.cached_content =
                Self::build_content(&self.term, self.cached_content.bounds);
            return MouseResult { report_bytes: None, needs_paint: true };
        }

        MouseResult { report_bytes: None, needs_paint: false }
    }

    // ── Mouse: up ──

    /// Mouse button released — sends release report or finalises selection.
    pub fn mouse_up(&mut self, event: &MouseUpEvent) -> MouseResult {
        let mode = self.mode();

        if in_mouse_mode(mode) {
            let btn = match mouse_btn_code(event.button) {
                Some(b) => b,
                None => return MouseResult { report_bytes: None, needs_paint: false },
            };
            let cell_w = self.cached_content.bounds.cell_width;
            let line_h = self.cached_content.bounds.line_height;
            let origin = self.cached_content.bounds.bounds.origin;
            let col = pixel_to_col_1based(event.position.x, origin.x, cell_w);
            let line = pixel_to_line_1based(event.position.y, origin.y, line_h);
            let bytes = mouse_report_bytes(btn, col, line, true, mode);
            return MouseResult { report_bytes: Some(bytes), needs_paint: false };
        }

        self.cached_content =
            Self::build_content(&self.term, self.cached_content.bounds);
        MouseResult { report_bytes: None, needs_paint: true }
    }

    // ── Mouse: move (hover) ──

    pub fn mouse_move(&mut self, event: &MouseMoveEvent, hitbox: Bounds<Pixels>) -> MouseResult {
        let mode = self.mode();

        if mode.contains(TermMode::MOUSE_MOTION) && event.pressed_button.is_none() {
            let cell_w = self.cached_content.bounds.cell_width;
            let line_h = self.cached_content.bounds.line_height;
            let origin = hitbox.origin;
            let col = pixel_to_col_1based(event.position.x, origin.x, cell_w);
            let line = pixel_to_line_1based(event.position.y, origin.y, line_h);
            let bytes = mouse_report_bytes(32, col, line, false, mode);
            return MouseResult { report_bytes: Some(bytes), needs_paint: true };
        }

        MouseResult { report_bytes: None, needs_paint: true }
    }

    // ── Scroll wheel ──

    pub fn scroll_wheel(&mut self, event: &ScrollWheelEvent) -> ScrollResult {
        let mode = self.mode();
        let line_h = self.cached_content.bounds.line_height;
        let cell_w = self.cached_content.bounds.cell_width;
        let origin = self.cached_content.bounds.bounds.origin;

        if scroll_should_report(mode) {
            let dy = event.delta.pixel_delta(line_h).y;
            let v_lines = (f32::from(dy) / f32::from(line_h)).round() as i32;
            let dx = event.delta.pixel_delta(cell_w).x;
            let h_lines = (f32::from(dx) / f32::from(cell_w)).round() as i32;

            let col = pixel_to_col_1based(event.position.x, origin.x, cell_w);
            let line = pixel_to_line_1based(event.position.y, origin.y, line_h);

            let mut bytes = Vec::new();
            if v_lines != 0 {
                let btn: u8 = if v_lines > 0 { 64 } else { 65 };
                for _ in 0..v_lines.abs() {
                    bytes.extend(mouse_report_bytes(btn, col, line, false, mode));
                }
                bytes.extend(mouse_report_bytes(btn, col, line, true, mode));
            }
            if h_lines != 0 {
                let btn: u8 = if h_lines > 0 { 66 } else { 67 };
                for _ in 0..h_lines.abs() {
                    bytes.extend(mouse_report_bytes(btn, col, line, false, mode));
                }
                bytes.extend(mouse_report_bytes(btn, col, line, true, mode));
            }
            if !bytes.is_empty() {
                return ScrollResult { report_bytes: Some(bytes), needs_paint: false };
            }
            return ScrollResult { report_bytes: None, needs_paint: false };
        }

        let dy = event.delta.pixel_delta(line_h).y;
        let lines = (f32::from(dy) / f32::from(line_h)).round() as i32;
        let lines = if lines == 0 && dy != Pixels::ZERO {
            if dy > Pixels::ZERO { 1 } else { -1 }
        } else {
            lines
        };
        if lines > 0 {
            for _ in 0..lines as usize {
                self.term
                    .scroll_display(alacritty_terminal::grid::Scroll::Delta(1));
            }
        } else if lines < 0 {
            for _ in 0..(-lines) as usize {
                self.term
                    .scroll_display(alacritty_terminal::grid::Scroll::Delta(-1));
            }
        }
        self.cached_content =
            Self::build_content(&self.term, self.cached_content.bounds);
        ScrollResult { report_bytes: None, needs_paint: true }
    }

    // ── Scroll line ──

    pub fn scroll_line_up(&mut self) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Delta(1));
        self.cached_content =
            Self::build_content(&self.term, self.cached_content.bounds);
    }

    pub fn scroll_line_down(&mut self) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Delta(-1));
        self.cached_content =
            Self::build_content(&self.term, self.cached_content.bounds);
    }

    // ── Keyboard ──

    pub fn try_keystroke(&self, keystroke: &gpui::Keystroke) -> Option<Vec<u8>> {
        let bytes = keys::keystroke_to_bytes(keystroke, self.mode());
        if bytes.is_empty() { None } else { Some(bytes) }
    }

    // ── Clipboard ──

    pub fn copy(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// Build paste bytes, wrapping in bracketed paste sequences when
    /// [`TermMode::BRACKETED_PASTE`] is active.
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if self.mode().contains(TermMode::BRACKETED_PASTE) {
            let mut bytes = b"\x1b[200~".to_vec();
            bytes.extend(text.as_bytes());
            bytes.extend(b"\x1b[201~");
            bytes
        } else {
            text.as_bytes().to_vec()
        }
    }

    // ── Focus ──

    pub fn focus_in(&mut self) -> Option<Vec<u8>> {
        self.term.is_focused = true;
        if self.mode().contains(TermMode::FOCUS_IN_OUT) {
            Some(b"\x1b[I".to_vec())
        } else {
            None
        }
    }

    pub fn focus_out(&mut self) -> Option<Vec<u8>> {
        self.term.is_focused = false;
        if self.mode().contains(TermMode::FOCUS_IN_OUT) {
            Some(b"\x1b[O".to_vec())
        } else {
            None
        }
    }

    // ── Simple accessors ──

    pub fn last_content(&self) -> &Content { &self.cached_content }
    pub fn mode(&self) -> TermMode { self.cached_content.mode }
    pub fn total_lines(&self) -> usize { self.total_lines }
    pub fn selection_started(&self) -> bool { self.term.selection.is_some() }
    pub fn matches_count(&self) -> usize { 0 }
    pub fn matches_clone(&self) -> Vec<alacritty_terminal::selection::SelectionRange> {
        Vec::new()
    }
    pub fn title(&self) -> &str { &self.title_text }
}
