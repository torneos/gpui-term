//! GPU-accelerated terminal renderer for GPUI.
//!
//! `gpui-term` provides a complete terminal emulator widget built on
//! [GPUI](https://gpui.rs) and [alacritty_terminal].
//!
//! # Features
//!
//! - `pty` (default) — local pseudo-terminal via system shell
//! - `ssh` — remote terminal via SSH (adds `russh` + `tokio`)
//!
//! # Example
//!
//! ```no_run
//! use gpui_term::*;
//! use gpui::*;
//!
//! let app = gpui_platform::application();
//! app.run(move |cx| {
//!     let bounds = TerminalBounds::new(px(16.0), px(8.0),
//!         Bounds::new(point(px(0.), px(0.)), size(px(640.), px(384.))));
//!     cx.open_window(WindowOptions::default(), move |window, cx| {
//!         let focus = cx.focus_handle();
//!         let pty = cx.new(|cx| PtyTerminal::spawn(None, bounds, cx));
//!         cx.new(|_cx| TerminalElement::new(
//!             TerminalEntity::Local(pty),
//!             TerminalColors::default_dark(),
//!             "Fira Code".into(),
//!             px(14.0), 1.2, focus, true,
//!         ))
//!     });
//!     cx.activate(true);
//! });
//! ```

// Re-export alacritty types used in the public API.
pub use alacritty_terminal::index::Point;
pub use alacritty_terminal::term::cell::Cell as TermCell;
pub use alacritty_terminal::term::cell::Flags as CellFlags;
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};

pub mod colors;
pub mod content;
pub mod contrast;
pub mod core;
pub mod cursor;
pub mod element;
pub mod entity;
pub mod font;
pub mod highlight;
pub mod keys;
pub mod mouse;
pub mod painter;
pub mod scrollbar;
pub mod url;

#[cfg(feature = "pty")]
pub mod terminal;
#[cfg(feature = "ssh")]
pub mod ssh;

pub use colors::TerminalColors;
pub use content::{Content, TerminalBounds};
pub use element::TerminalElement;
pub use entity::TerminalEntity;
pub use font::{compute_cell_dimensions, default_monospace_family};
pub use painter::{BatchedTextRun, LayoutRect};

#[cfg(feature = "pty")]
pub use terminal::PtyTerminal;
#[cfg(feature = "ssh")]
pub use ssh::{ConnectionState, SshTerminal};
