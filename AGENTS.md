# AGENTS.md — gpui-term

GPU-accelerated terminal emulator widget for GPUI. Built on `alacritty_terminal`.

## Project Structure

```
gpui-term/
├── Cargo.toml          # features: pty (default), ssh
├── examples/minimal.rs # demo binary
└── src/
    ├── lib.rs          # public API re-exports
    ├── content.rs      # Content, TerminalBounds types
    ├── colors.rs       # TerminalColors, NamedColor→Hsla, 256-color palette
    ├── contrast.rs     # APCA contrast algorithm
    ├── keys.rs         # Keystroke→bytes (shared PTY + SSH)
    ├── painter.rs      # Cell batching: cells→BatchedTextRun+LayoutRect
    ├── cursor.rs       # Block/Underline/Bar/Hollow cursor shapes
    ├── scrollbar.rs    # Scrollbar rendering
    ├── highlight.rs    # Selection/search highlight rendering
    ├── url.rs          # Safe URL opening (cross-platform)
    ├── mouse.rs        # Mouse event registration
    ├── element.rs      # TerminalElement (GPUI Element trait)
    ├── entity.rs       # TerminalEntity enum (Local|Ssh dispatch)
    ├── terminal.rs     # PtyTerminal (PTY + alacritty Term)
    ├── ssh.rs          # SshTerminal (SSH via russh)
    ├── font.rs         # Cross-platform monospace font fallback
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
            ├── prepaint: sync + resize + cell processing → LayoutState
            └── paint: quads + text runs + cursor + scrollbar + mouse
```

- **TerminalElement** is a pure GPUI `Element` — no keyboard handling inside. Keyboard is handled at the view level via `div().on_key_down(cx.listener(...))` + `.track_focus()`.
- **PtyTerminal** / **SshTerminal** have identical public API — dispatched through `TerminalEntity` enum.
- **Content** uses alacritty types directly (`Point`, `Cell`, `TermMode`, `SelectionRange`).

## Key Patterns

| Pattern | Where | Notes |
|---------|-------|-------|
| `div().size_full().track_focus().on_key_down(cx.listener())` | `minimal.rs` | Root view layout |
| `interactivity.paint(Some(&hitbox), ...)` | `element.rs` | Focus↔hitbox binding |
| Mouse listeners OUTSIDE `interactivity.paint()` | `element.rs` | Must match pulse-render |
| `set_size` → `build_content` | `terminal.rs` | Rebuild content on resize |
| `hash_color()` — full Color data | `element.rs` | Includes RGB values, not just discriminant |
| `keystroke_to_bytes(mode)` | `keys.rs` | App cursor, app keypad, Alt prefix |
| Mouse reporting (SGR) | `terminal.rs` | Encodes events for TUI apps |

## Code Conventions

- `use gpui::*` in examples, explicit imports in library
- `// ==== Section ====` / `// ── Subsection ──` visual separators
- `///` doc comments on all public items
- `pub(crate)` for cross-module, private by default
- `SCREAMING_SNAKE_CASE` for constants
- One concept per file, ~500 lines max
- Zero warnings: `cargo check` must be clean
- No `unsafe` except FFI (`libc::dup`, `libc::fcntl`)

## GPUI Version Notes

- GPUI rev `1d217ee` — `size_full()` exists only via `use gpui::*` (Styled trait through wildcard)
- `AppContext::new()` requires `use gpui::AppContext as _`
- `WindowOptions::window_bounds` (not `bounds`)
- `WindowBounds::Windowed(...)` (not `Fixed`)
- `Render::render()` takes `(&mut Window, &mut Context<Self>)`
