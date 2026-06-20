# AGENTS.md — gpui-term

GPU-accelerated terminal emulator widget for GPUI. Built on `alacritty_terminal`.

## Project Structure

```
gpui-term/
├── Cargo.toml          # features: pty (default), ssh
├── README.md
├── AGENTS.md
├── PLAN.md
├── examples/minimal.rs # demo binary (PTY + optional SSH toggle)
└── src/
    ├── lib.rs          # public API re-exports
    ├── core.rs         # TerminalCore — all shared logic (Term, Processor, mouse, scroll, keyboard)
    ├── content.rs      # Content, TerminalBounds types
    ├── colors.rs       # TerminalColors, NamedColor→Hsla, 256-color palette
    ├── contrast.rs     # APCA contrast algorithm (auto fg/bg contrast)
    ├── keys.rs         # Keystroke→bytes (xterm + Kitty CSI u + app modes)
    ├── painter.rs      # Cell batching: cells→BatchedTextRun+LayoutRect
    ├── cursor.rs       # Block/Underline/Bar/Hollow cursor shapes
    ├── scrollbar.rs    # Scrollbar rendering
    ├── highlight.rs    # Selection/search highlight rendering
    ├── url.rs          # Safe URL opening (cross-platform)
    ├── mouse.rs        # Mouse event registration (click, drag, scroll, scrollbar)
    ├── element.rs      # TerminalElement (GPUI Element trait)
    ├── entity.rs       # TerminalEntity enum (Local|Ssh dispatch)
    ├── terminal.rs     # PtyTerminal — thin PTY wrapper (~265 lines)
    ├── ssh.rs          # SshTerminal — thin SSH wrapper with russh auth (~443 lines)
    ├── font.rs         # Font fallback + compute_cell_dimensions()
```

## Stack

| Layer | Technology |
|-------|-----------|
| UI | GPUI (git dep, rev `1d217ee`) |
| Terminal | `alacritty_terminal` (git dep, rev `fcf32fe`) |
| PTY | `alacritty_terminal::tty` |
| SSH | `russh` 0.61 (optional, feature `ssh`) |

## Build

```bash
# Library + example (PTY only)
cargo check --example minimal

# With SSH support
cargo check --features ssh --example minimal

# Run
cargo run --example minimal
```

## Architecture

```
TerminalView (user's entity, impl Render)
  └─ div().size_full().track_focus().on_key_down()
       └─ TerminalElement (impl Element)
            ├── prepaint: resize → sync → content hash → cell processing → LayoutState
            └── paint: background + text runs + cursor + scrollbar + mouse listeners

TerminalCore (shared alacritty state machine)
  ├─ term: Term<CoreEventListener>     — ANSI/VT state
  ├─ processor: Processor              — escape-sequence parser
  ├─ rx + incoming + cached_content    — I/O buffer + render cache
  └─ event channels                    — title, bell, pty_write, clipboard

PtyTerminal → wraps TerminalCore + pty file descriptors + SIGWINCH
SshTerminal → wraps TerminalCore + russh channels + tokio runtime
```

- **TerminalElement** is a pure GPUI `Element` — no keyboard handling inside. Keyboard is at the view level via `div().on_key_down(cx.listener(...))` + `.track_focus()`.
- **TerminalCore** holds ALL terminal logic (mouse, scroll, keyboard, content). PTY and SSH backends are thin I/O wrappers. Fix a bug once in core.rs — it's fixed everywhere.
- **PtyTerminal** / **SshTerminal** share the same public API — dispatched through `TerminalEntity` enum.
- **Content** uses alacritty types directly (`Point`, `Cell`, `TermMode`, `SelectionRange`).

## Key Patterns

| Pattern | Where | Notes |
|---------|-------|-------|
| `div().size_full().track_focus().on_key_down(cx.listener())` | `minimal.rs` | Root view layout |
| Mouse listeners OUTSIDE `interactivity.paint()` | `element.rs:560-575` | Must match pulse-render |
| `TerminalCore::set_size()` → `build_content()` | `core.rs:313` | Rebuild content on resize |
| `mouse_down/drag/up/move` → `MouseResult` | `core.rs:320-415` | Report bytes returned to caller for I/O |
| `scroll_wheel` → `ScrollResult` | `core.rs:435` | Same pattern as MouseResult |
| `hash_color()` — full Color data (RGB values) | `element.rs:615` | Includes Spec r,g,b, Indexed number |
| `compute_hash()` — cell flags + cursor + hyperlink | `element.rs:148` | Full content fingerprint for cache |
| `keystroke_to_bytes(mode)` | `keys.rs` | Kitty CSI u, xterm, App Cursor, App Keypad |
| Kitty keyboard check at TOP of `keystroke_to_bytes` | `keys.rs:12` | `DISAMBIGUATE_ESC_CODES \| REPORT_ALL_KEYS_AS_ESC \| REPORT_EVENT_TYPES` |
| `paste()` wraps in `\x1b[200~...\x1b[201~` | `core.rs:530` | When `BRACKETED_PASTE` mode active |
| Cmd+click hyperlink → `url::open_url()` | `mouse.rs:68-85` | OSC 8 hyperlinks |
| `resize_pending` — skip cell render until shell redraws | `element.rs:340,372` | Viewport clearing on resize |
| `compute_cell_dimensions()` — font-based sizing | `font.rs:34` | `advance('m')` for cell_width |
| Font cache invalidated on family/size/ratio change | `element.rs:117-123` | Tuple key `(font_family, font_size, ratio)` |
| SIGWINCH debounce 500ms in PtyTerminal/SshTerminal | `terminal.rs:120, ssh.rs:171` | Grid always resizes immediately; SIGWINCH debounced |
| `CoreEventListener` with mpsc channels | `core.rs:30-38` | Forwards title, bell, pty_write, clipboard events |
| `sync()` drains `event_rx` → queues → callbacks | `core.rs:235-268` | pty_write to PTY, clipboard to GPUI |

## Code Conventions

- `use gpui::*` in examples, explicit imports in library
- `// ==== Section ====` / `// ── Subsection ──` visual separators
- `///` doc comments on all public API items (structs, enums, key methods)
- `pub(crate)` for cross-module, private by default
- `SCREAMING_SNAKE_CASE` for constants
- One concept per file, ~500 lines max (core.rs and element.rs are slightly over)
- Zero warnings: `cargo check` and `cargo clippy` must be clean
- No `unsafe` except FFI (`libc::dup`, `libc::fcntl`) in `terminal.rs` under `#[cfg(unix)]`
- `#[allow(clippy::too_many_arguments)]` on complex functions with 8+ params

## GPUI Version Notes

- GPUI rev `1d217ee` — `size_full()` exists only via `use gpui::*` (Styled trait through wildcard)
- `AppContext::new()` requires `use gpui::AppContext as _`
- `WindowOptions::window_bounds` (not `bounds`)
- `WindowBounds::Windowed(...)` (not `Fixed`)
- `Render::render()` takes `(&mut Window, &mut Context<Self>)`
- `Pixels.0` is private — use `f32::from(pixels)` or `px(value)`
