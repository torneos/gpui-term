//! Unified terminal entity — dispatches to [`PtyTerminal`] or
//! [`SshTerminal`](crate::SshTerminal) with identical method names.

use gpui::{
    App, Bounds, Entity, EntityId, Keystroke, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent, SharedString, Window,
};

use crate::TerminalBounds;
#[cfg(feature = "pty")]
use crate::PtyTerminal;
#[cfg(feature = "ssh")]
use crate::SshTerminal;

/// A type-erased terminal backend.
///
/// Both variants have the **same public method names** — callers never
/// need to know which backend is active.
#[derive(Clone)]
pub enum TerminalEntity {
    /// Local PTY terminal.
    #[cfg(feature = "pty")]
    Local(Entity<PtyTerminal>),
    /// Remote SSH terminal.
    #[cfg(feature = "ssh")]
    Ssh(Entity<SshTerminal>),
}

impl TerminalEntity {
    /// Return the GPUI entity ID of the inner terminal.
    pub fn entity_id(&self) -> EntityId {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.entity_id(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.entity_id(),
        }
    }

    /// Borrow the most recent renderable content.
    pub fn last_content(&self, cx: &App) -> crate::Content {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).last_content().clone(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).last_content().clone(),
        }
    }

    /// Total lines including scrollback.
    pub fn total_lines(&self, cx: &App) -> usize {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).total_lines(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).total_lines(),
        }
    }

    /// Current terminal mode flags.
    pub fn mode(&self, cx: &App) -> alacritty_terminal::term::TermMode {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).mode(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).mode(),
        }
    }

    /// Process pending I/O and rebuild content cache.
    pub fn sync(&self, window: &mut Window, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.sync(window, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.sync(window, cx)),
        }
    }

    /// Resize the terminal grid.
    pub fn set_size(&self, bounds: TerminalBounds, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _cx| t.set_size(bounds)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _cx| t.set_size(bounds)),
        }
    }

    /// Handle a keystroke. Returns `true` if consumed.
    pub fn try_keystroke(&self, keystroke: &Keystroke, cx: &mut App) -> bool {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.try_keystroke(keystroke)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.try_keystroke(keystroke)),
        }
    }

    /// Mouse-down on the terminal.
    pub fn mouse_down(&self, event: &MouseDownEvent, bounds: Bounds<Pixels>, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.mouse_down(event, bounds, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.mouse_down(event, bounds, cx)),
        }
    }

    /// Mouse-up on the terminal.
    pub fn mouse_up(&self, event: &MouseUpEvent, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.mouse_up(event, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.mouse_up(event, cx)),
        }
    }

    /// Mouse-drag (selection, scrollbar).
    pub fn mouse_drag(&self, event: &MouseMoveEvent, bounds: Bounds<Pixels>, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.mouse_drag(event, bounds, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.mouse_drag(event, bounds, cx)),
        }
    }

    /// Mouse-move (hover).
    pub fn mouse_move(&self, event: &MouseMoveEvent, bounds: Bounds<Pixels>, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.mouse_move(event, bounds, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.mouse_move(event, bounds, cx)),
        }
    }

    /// Scroll wheel.
    pub fn scroll_wheel(&self, event: &ScrollWheelEvent, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, cx| t.scroll_wheel(event, cx)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, cx| t.scroll_wheel(event, cx)),
        }
    }

    /// Copy selection to clipboard. Returns the selected text, if any.
    pub fn copy(&self, cx: &mut App) -> Option<String> {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.copy()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.copy()),
        }
    }

    /// Paste text to terminal.
    pub fn paste(&self, text: &str, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.paste(text)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.paste(text)),
        }
    }

    /// Send raw bytes to terminal (keyboard input).
    pub fn input_bytes(&self, bytes: Vec<u8>, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.input_bytes(bytes)),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.input_bytes(bytes)),
        }
    }

    /// Notify terminal of focus gained.
    pub fn focus_in(&self, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.focus_in()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.focus_in()),
        }
    }

    /// Notify terminal of focus lost.
    pub fn focus_out(&self, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _| t.focus_out()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _| t.focus_out()),
        }
    }

    /// Whether a text selection is in progress.
    pub fn selection_started(&self, cx: &App) -> bool {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).selection_started(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).selection_started(),
        }
    }

    /// Number of active search matches.
    pub fn matches_count(&self, cx: &App) -> usize {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).matches_count(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).matches_count(),
        }
    }

    /// Clone current search match ranges.
    pub fn matches_clone(&self, cx: &App) -> Vec<alacritty_terminal::selection::SelectionRange> {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).matches_clone(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).matches_clone(),
        }
    }

    /// Terminal title.
    pub fn title(&self, cx: &App) -> SharedString {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => SharedString::from(e.read(cx).title()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => SharedString::from(e.read(cx).title()),
        }
    }

    /// Scroll one line up (toward history).
    pub fn scroll_line_up(&self, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _cx| t.scroll_line_up()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _cx| t.scroll_line_up()),
        }
    }

    /// Scroll one line down (toward prompt).
    pub fn scroll_line_down(&self, cx: &mut App) {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.update(cx, |t, _cx| t.scroll_line_down()),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.update(cx, |t, _cx| t.scroll_line_down()),
        }
    }

    /// Whether the last sync processed new output bytes.
    pub fn had_sync_output(&self, cx: &App) -> bool {
        match self {
            #[cfg(feature = "pty")]
            Self::Local(e) => e.read(cx).had_sync_output(),
            #[cfg(feature = "ssh")]
            Self::Ssh(e) => e.read(cx).had_sync_output(),
        }
    }
}
