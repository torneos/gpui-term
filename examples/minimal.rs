//! Minimal terminal emulator using gpui-term.
//!
//! - **Cmd+C/V**: copy/paste
//! - **Cmd+Shift+S**: toggle SSH connection to localhost (requires `ssh` feature)

use gpui::*;
use gpui_term::{
    TerminalBounds, TerminalColors, TerminalElement, TerminalEntity,
    compute_cell_dimensions, default_monospace_family,
};
#[cfg(feature = "pty")]
use gpui_term::PtyTerminal;
#[cfg(feature = "ssh")]
use gpui_term::SshTerminal;

struct TerminalView {
    #[cfg(feature = "ssh")]
    local_terminal: TerminalEntity,
    active_terminal: TerminalEntity,
    #[cfg(feature = "ssh")]
    ssh_terminal: Option<TerminalEntity>,
    colors: TerminalColors,
    font_family: SharedString,
    font_size_px: gpui::Pixels,
    line_height_ratio: f32,
    focus: FocusHandle,
    focused: bool,
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let eid = cx.entity_id();
        cx.defer(move |cx| cx.notify(eid));

        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(
                |this: &mut TerminalView, e: &KeyDownEvent, _window, cx| {
                    let keystroke = &e.keystroke;
                    let mods = &keystroke.modifiers;

                    // Cmd+Shift+S: toggle SSH ↔ local
                    #[cfg(feature = "ssh")]
                    if mods.platform && mods.shift && keystroke.key == "s" {
                        if this.ssh_terminal.is_none() {
                            let user = std::env::var("USER").unwrap_or_default();
                            let ssh = cx.new(|cx| {
                                SshTerminal::connect(&user, "localhost", 22, None, None, cx)
                            });
                            this.ssh_terminal = Some(TerminalEntity::Ssh(ssh));
                        }
                        if matches!(this.active_terminal, TerminalEntity::Ssh(_)) {
                            this.active_terminal = this.local_terminal.clone();
                        } else {
                            this.active_terminal = this.ssh_terminal.clone().unwrap();
                        }
                        cx.notify();
                        return;
                    }

                    // Cmd+C: copy
                    if mods.platform && keystroke.key == "c" {
                        if let Some(text) = this.active_terminal.copy(cx) {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        }
                        return;
                    }
                    // Cmd+V: paste
                    if mods.platform && keystroke.key == "v" {
                        if let Some(text) = cx.read_from_clipboard()
                            .and_then(|item| item.text())
                        {
                            this.active_terminal.paste(&text, cx);
                        }
                        return;
                    }

                    this.active_terminal.try_keystroke(&e.keystroke, cx);
                },
            ))
            .child(TerminalElement::new(
                self.active_terminal.clone(),
                self.colors.clone(),
                self.font_family.clone(),
                self.font_size_px,
                self.line_height_ratio,
                self.focus.clone(),
                self.focused,
            ))
    }
}

fn main() {
    let app = gpui_platform::application();
    app.run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(100.), px(100.)),
                    size(px(800.), px(600.)),
                ))),
                ..Default::default()
            },
            |window, cx| {
                let focus = cx.focus_handle();
                let font_family = SharedString::from(default_monospace_family());
                let font_size_px = px(14.0);
                let line_height_ratio = 1.2;
                let (cell_width, line_height) =
                    compute_cell_dimensions(window, font_family.clone(), font_size_px, line_height_ratio);
                let bounds = TerminalBounds::new(
                    line_height,
                    cell_width,
                    window.bounds(),
                );
                let pty =
                    cx.new(|cx| PtyTerminal::spawn(None, bounds, cx));
                let entity = TerminalEntity::Local(pty);

                cx.new(|_cx| TerminalView {
                    #[cfg(feature = "ssh")]
                    local_terminal: entity.clone(),
                    active_terminal: entity,
                    #[cfg(feature = "ssh")]
                    ssh_terminal: None,
                    colors: TerminalColors::default_dark(),
                    font_family,
                    font_size_px,
                    line_height_ratio,
                    focus: focus.clone(),
                    focused: true,
                })
            },
        )
        .unwrap();

        cx.activate(true);
    });
}
