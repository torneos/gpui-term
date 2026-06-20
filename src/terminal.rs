//! PTY-backed terminal entity for GPUI.
//!
//! [`PtyTerminal`] owns a [`TerminalCore`](crate::core::TerminalCore) that
//! handles all alacritty logic (mouse, scroll, keyboard) and adds PTY-specific
//! I/O (shell spawn, SIGWINCH, fd-based reads/writes).

use std::io::Read as _;
use std::io::Write as _;
use std::time::{Duration, Instant};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::sync::mpsc;

use alacritty_terminal::tty::{self, EventedReadWrite};
use alacritty_terminal::term::TermMode;
use gpui::{Context, Keystroke, Window};

use crate::content::TerminalBounds;
use crate::core::TerminalCore;

// =========================================================================
// PtyTerminal
// =========================================================================

/// A GPUI entity managing a local pseudo-terminal.
///
/// Wraps [`TerminalCore`](crate::core::TerminalCore) with PTY I/O:
/// shell spawn, fd-based reads/writes, and SIGWINCH notifications.
pub struct PtyTerminal {
    core: TerminalCore,
    pty: tty::Pty,
    last_sigwinch: Instant,
}

impl PtyTerminal {
    /// Spawn the default shell in a new PTY.
    ///
    /// # Panics
    ///
    /// Panics if the PTY cannot be opened.
    pub fn spawn(
        shell: Option<&str>,
        bounds: TerminalBounds,
        _cx: &mut Context<Self>,
    ) -> Self {
        use alacritty_terminal::event::WindowSize;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let core = TerminalCore::new(bounds, rx);

        let window_size = WindowSize {
            num_lines: bounds.num_lines() as u16,
            num_cols: bounds.num_columns() as u16,
            cell_width: f32::from(bounds.cell_width) as u16,
            cell_height: f32::from(bounds.line_height) as u16,
        };
        let options = tty::Options {
            shell: shell.map(|s| tty::Shell::new(s.to_string(), vec![])),
            ..Default::default()
        };
        let pty = tty::new(&options, window_size, 0).expect("failed to open PTY");

        // Background reader thread
        let reader = dup_fd_blocking(pty.file());
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 { break; }
                if tx.send(buf[..n].to_vec()).is_err() { break; }
            }
        });

        Self { core, pty, last_sigwinch: Instant::now() }
    }

    // ── I/O ──

    /// Send bytes to the PTY.
    pub fn input_bytes(&mut self, bytes: Vec<u8>) {
        let _ = self.pty.writer().write(&bytes);
    }

    // ── Sync ──

    /// Process pending PTY output and rebuild the content cache.
    /// Also drains side-band events (pty_write, title, bell, clipboard, etc.).
    pub fn sync(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.core.sync() {
            cx.notify();
        }
        // Send pty_write output (DA responses, DSR, color query replies)
        for text in self.core.drain_pty_writes() {
            let _ = self.pty.writer().write(text.as_bytes());
        }
        // Handle clipboard events
        for event in self.core.take_pending_events() {
            match event {
                alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                alacritty_terminal::event::Event::ClipboardLoad(_, formatter) => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            let s = formatter(&text);
                            let _ = self.pty.writer().write(s.as_bytes());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ── Resize ──

    pub fn set_size(&mut self, bounds: TerminalBounds) {
        use alacritty_terminal::event::{OnResize, WindowSize};
        // Always resize the grid immediately — no debounce on rendering.
        self.core.set_size(bounds);
        // Debounce SIGWINCH to avoid flooding the shell.
        let now = Instant::now();
        if now.duration_since(self.last_sigwinch) < Duration::from_millis(500) {
            return;
        }
        self.last_sigwinch = now;
        let window_size = WindowSize {
            num_lines: bounds.num_lines() as u16,
            num_cols: bounds.num_columns() as u16,
            cell_width: f32::from(bounds.cell_width) as u16,
            cell_height: f32::from(bounds.line_height) as u16,
        };
        self.pty.on_resize(window_size);
    }

    // ── Keyboard ──

    pub fn try_keystroke(&mut self, keystroke: &Keystroke) -> bool {
        if let Some(bytes) = self.core.try_keystroke(keystroke) {
            self.input_bytes(bytes);
            true
        } else {
            false
        }
    }

    // ── Mouse ──

    pub fn mouse_down(
        &mut self,
        event: &gpui::MouseDownEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_down(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_up(&mut self, event: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let result = self.core.mouse_up(event);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_drag(
        &mut self,
        event: &gpui::MouseMoveEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_drag(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_move(
        &mut self,
        event: &gpui::MouseMoveEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_move(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn scroll_wheel(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.scroll_wheel(event);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    // ── Clipboard ──

    pub fn copy(&self) -> Option<String> {
        self.core.copy()
    }

    pub fn paste(&mut self, text: &str) {
        self.input_bytes(self.core.paste(text));
    }

    // ── Focus ──

    pub fn focus_in(&mut self) {
        if let Some(bytes) = self.core.focus_in() {
            self.input_bytes(bytes);
        }
    }
    pub fn focus_out(&mut self) {
        if let Some(bytes) = self.core.focus_out() {
            self.input_bytes(bytes);
        }
    }

    // ── Simple accessors ──

    pub fn last_content(&self) -> &crate::content::Content { self.core.last_content() }
    pub fn total_lines(&self) -> usize { self.core.total_lines() }
    pub fn mode(&self) -> TermMode { self.core.mode() }
    pub fn selection_started(&self) -> bool { self.core.selection_started() }
    pub fn matches_count(&self) -> usize { self.core.matches_count() }
    pub fn matches_clone(&self) -> Vec<alacritty_terminal::selection::SelectionRange> {
        self.core.matches_clone()
    }
    pub fn title(&self) -> &str { self.core.title() }

    pub fn had_sync_output(&self) -> bool { self.core.had_output }

    pub fn scroll_line_up(&mut self) { self.core.scroll_line_up(); }
    pub fn scroll_line_down(&mut self) { self.core.scroll_line_down(); }
}

// =========================================================================
// PTY reader setup
// =========================================================================

#[cfg(unix)]
fn dup_fd_blocking(file: &std::fs::File) -> std::fs::File {
    use std::os::fd::{FromRawFd, RawFd};
    let raw: RawFd = file.as_raw_fd();
    let dup = unsafe { libc::dup(raw) };
    if dup < 0 { panic!("failed to dup PTY fd"); }
    let flags = unsafe { libc::fcntl(dup, libc::F_GETFL, 0) };
    let _ = unsafe { libc::fcntl(dup, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    unsafe { std::fs::File::from_raw_fd(dup) }
}

#[cfg(not(unix))]
fn dup_fd_blocking(_file: &std::fs::File) -> std::fs::File {
    unimplemented!("PTY reader not implemented for this platform")
}
