# gpui-term

GPU-accelerated terminal emulator widget for [GPUI](https://gpui.rs).

Built on [alacritty_terminal](https://github.com/alacritty/alacritty) for
ANSI/VT parsing and state management, rendered through GPUI's native graphics
pipeline (Metal on macOS, WGPU on Linux, DirectX on Windows).

```rust
use gpui::*;
use gpui_term::*;

let app = gpui_platform::application();
app.run(move |cx| {
    cx.open_window(WindowOptions::default(), |window, cx| {
        let focus = cx.focus_handle();
        let (cw, lh) = compute_cell_dimensions(
            window, "Menlo".into(), px(14.0), 1.2,
        );
        let bounds = TerminalBounds::new(lh, cw, window.bounds());
        let pty = cx.new(|cx| PtyTerminal::spawn(None, bounds, cx));
        cx.new(|_cx| TerminalElement::new(
            TerminalEntity::Local(pty),
            TerminalColors::default_dark(),
            "Menlo".into(),
            px(14.0),
            1.2,
            focus,
            true,
        ))
    });
    cx.activate(true);
});
```

## Features

- **GPU rendering** — cell batching into GPUI text runs with native font shaping
- **True colour** — 24-bit RGB, 256-colour palette, ANSI VGA colours
- **Full keyboard** — Kitty protocol (CSI u), F1–F20 with modifiers, Ctrl+digits, Alt/Shift combos, app cursor/keypad
- **Full mouse** — SGR/X10/UTF-8 encoding, double/triple click selection, block selection, modes 1000–1007
- **Clipboard** — OSC 52 read/write, bracketed paste, auto-copy on select
- **Hyperlinks** — OSC 8 parsing with Cmd+click to open
- **Font-aware** — cell dimensions computed from actual font metrics
- **APCA contrast** — automatic foreground-background contrast adjustment
- **Local PTY** — spawns system shell via `alacritty_terminal::tty`
- **Remote SSH** — connects via `russh` with agent, key, and password auth

### Keyboard protocol support

| Protocol | Status |
|----------|--------|
| xterm (CSI sequences) | ✅ |
| Kitty (CSI u) | ✅ |
| App Cursor (DECCKM) | ✅ |
| App Keypad (DECKPAM) | ✅ |
| F1–F20 with all modifiers | ✅ |
| Ctrl+digits (0x00–0x7f) | ✅ |
| Alt/Shift/Ctrl/Super combos | ✅ |

### Mouse protocol support

| Mode | Protocol | Status |
|------|----------|--------|
| 1000 | X11 — click + release | ✅ |
| 1002 | Button-event — drag | ✅ |
| 1003 | Any-event — motion | ✅ |
| 1004 | Focus in/out | ✅ |
| 1005 | UTF-8 encoding | ✅ |
| 1006 | SGR encoding | ✅ |
| 1007 | Alternate scroll | ✅ |

## Installation

Add as a git dependency:

```toml
[dependencies]
gpui-term = { git = "https://github.com/torneos/gpui-term", rev = "3c006ef" }
```

### Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `pty` | ✅ | Local pseudo-terminal via system shell |
| `ssh` | — | Remote terminal via SSH (adds `russh` + `tokio`) |

```toml
# With SSH support
gpui-term = { git = "...", features = ["ssh"] }
```

## Usage

### Minimal example

See [`examples/minimal.rs`](examples/minimal.rs) — press `Cmd+Shift+S` to toggle
between local PTY and SSH to `localhost`.

### Local terminal

```rust
struct MyTerminalView {
    terminal: TerminalEntity,
    colors: TerminalColors,
    focus: FocusHandle,
}

impl Render for MyTerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                this.terminal.try_keystroke(&e.keystroke, cx);
            }))
            .child(TerminalElement::new(
                self.terminal.clone(),
                self.colors.clone(),
                "Menlo".into(),
                px(14.0),
                1.2,
                self.focus.clone(),
                true,
            ))
    }
}
```

### SSH terminal

```rust
#[cfg(feature = "ssh")]
let ssh = cx.new(|cx| {
    SshTerminal::connect("user", "host", 22, None, None, cx)
});
let terminal = TerminalEntity::Ssh(ssh);
```

## Architecture

```
TerminalView (your entity, impl Render)
  └─ div().size_full().track_focus().on_key_down()
       └─ TerminalElement (impl Element)
            ├─ prepaint: sync + resize + cell processing → LayoutState
            └─ paint: background + text runs + cursor + scrollbar + mouse

TerminalCore (shared alacritty state machine)
  ├─ PtyTerminal wreps with PTY I/O (fd-based)
  └─ SshTerminal wreps with SSH I/O (russh channels)
```

All terminal logic (mouse, scroll, keyboard, content building) lives in
`TerminalCore`. PTY and SSH backends are thin wrappers that only handle I/O.

## License

Apache-2.0
