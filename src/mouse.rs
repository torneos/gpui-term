//! Mouse event handling for TerminalElement.
//!
//! Registers per-frame listeners for left/right/middle click, drag, scroll,
//! and scrollbar interaction.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use alacritty_terminal::term::TermMode;
use gpui::{
    App, DispatchPhase, FocusHandle, Hitbox, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, Window,
    point, px,
};

use crate::entity::TerminalEntity;

/// Shared scrollbar drag state. TerminalElement is recreated every frame,
/// so a local cell in register_mouse_listeners would lose state between frames.
static SCROLLBAR_DRAGGING: AtomicBool = AtomicBool::new(false);

/// Whether the terminal is in any mouse-reporting mode.
fn in_mouse_mode(mode: TermMode) -> bool {
    mode.contains(
        TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
    )
}

// =========================================================================
// Public registration
// =========================================================================

/// Register all mouse event listeners on the window for one frame.
pub fn register_mouse_listeners(
    terminal: TerminalEntity,
    focus: FocusHandle,
    mode: TermMode,
    hitbox: &Hitbox,
    window: &mut Window,
) {
    let hitbox = hitbox.clone();
    let bar_w = px(crate::scrollbar::SCROLLBAR_WIDTH);

    fn in_scrollbar(hitbox: &Hitbox, bar_w: Pixels, pos: impl Into<Point<Pixels>>) -> bool {
        pos.into().x >= hitbox.bounds.origin.x + hitbox.bounds.size.width - bar_w
    }

    // ── Left click ──
    window.on_mouse_event({
        let terminal = terminal.clone();
        let focus = focus.clone();
        let hitbox = hitbox.clone();
        move |e: &MouseDownEvent, phase, window, cx| {
            if !check_mouse_down(phase, e.button, MouseButton::Left, &hitbox, window) {
                return;
            }
            window.focus(&focus, cx);

            // Cmd+click hyperlink: open URL and skip selection
            if e.modifiers.platform {
                let content = terminal.last_content(cx);
                let col = ((f32::from(e.position.x) - f32::from(hitbox.bounds.origin.x))
                    / f32::from(content.bounds.cell_width))
                    .floor()
                    .max(0.0) as usize;
                let line = ((f32::from(e.position.y) - f32::from(hitbox.bounds.origin.y))
                    / f32::from(content.bounds.line_height))
                    .floor()
                    .max(0.0) as i32;
                let point = alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    alacritty_terminal::index::Column(col),
                );
                for (p, cell) in &content.cells {
                    if p == &point {
                        if let Some(hyperlink) = cell.hyperlink() {
                            crate::url::open_url(hyperlink.uri());
                        }
                        return;
                    }
                }
            }

            // Scrollbar: start drag, skip terminal.
            if in_scrollbar(&hitbox, bar_w, e.position) {
                SCROLLBAR_DRAGGING.store(true, Ordering::SeqCst);
                scrollbar_scroll_to(&terminal, &hitbox, e.position.y, cx);
                return;
            }

            terminal.mouse_down(e, hitbox.bounds, cx);
        }
    });

    // ── Left up ──
    window.on_mouse_event({
        let terminal = terminal.clone();
        let focus = focus.clone();
        let hitbox = hitbox.clone();
        move |e: &MouseUpEvent, phase, window, cx| {
            if !check_mouse_up(phase, e.button, MouseButton::Left, &focus, &hitbox, window) {
                return;
            }
            if SCROLLBAR_DRAGGING.load(Ordering::SeqCst) {
                SCROLLBAR_DRAGGING.store(false, Ordering::SeqCst);
                return;
            }
            terminal.mouse_up(e, cx);
            // Auto-copy selection to clipboard on mouse-up.
            if let Some(text) = terminal.copy(cx) {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
        }
    });

    // ── Mouse move ──
    window.on_mouse_event({
        let terminal = terminal.clone();
        let hitbox = hitbox.clone();
        let focus = focus.clone();
        move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            if SCROLLBAR_DRAGGING.load(Ordering::SeqCst) {
                scrollbar_scroll_to(&terminal, &hitbox, e.position.y, cx);
                return;
            }
            if e.pressed_button.is_some() && !cx.has_active_drag() && focus.is_focused(window) {
                let hovered = hitbox.is_hovered(window);
                if terminal.selection_started(cx) || hovered {
                    terminal.mouse_drag(e, hitbox.bounds, cx);
                }
            }
            if hitbox.is_hovered(window) {
                terminal.mouse_move(e, hitbox.bounds, cx);
            }
        }
    });

    // ── Scroll wheel ──
    window.on_mouse_event({
        let terminal = terminal.clone();
        let hitbox = hitbox.clone();
        move |e: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            terminal.scroll_wheel(e, cx);
        }
    });

    // ── Middle click: paste ──
    window.on_mouse_event({
        let terminal = terminal.clone();
        let focus = focus.clone();
        let hitbox = hitbox.clone();
        move |e: &MouseDownEvent, phase, window, cx| {
            if !check_mouse_down(phase, e.button, MouseButton::Middle, &hitbox, window) {
                return;
            }
            window.focus(&focus, cx);
            let text = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default();
            if !text.is_empty() {
                terminal.paste(&text, cx);
            }
        }
    });

    // ── Mouse mode: right-click and middle-up → terminal ──
    if in_mouse_mode(mode) {
        window.on_mouse_event({
            let terminal = terminal.clone();
            let focus = focus.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseDownEvent, phase, window, cx| {
                if !check_mouse_down(phase, e.button, MouseButton::Right, &hitbox, window) {
                    return;
                }
                window.focus(&focus, cx);
                terminal.mouse_down(e, hitbox.bounds, cx);
            }
        });
        window.on_mouse_event({
            let terminal = terminal.clone();
            let focus = focus.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseUpEvent, phase, window, cx| {
                if !check_mouse_up(phase, e.button, MouseButton::Right, &focus, &hitbox, window) {
                    return;
                }
                terminal.mouse_up(e, cx);
            }
        });
        window.on_mouse_event({
            let terminal = terminal.clone();
            let focus = focus.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseUpEvent, phase, window, cx| {
                if !check_mouse_up(phase, e.button, MouseButton::Middle, &focus, &hitbox, window) {
                    return;
                }
                terminal.mouse_up(e, cx);
            }
        });
    } else {
        // ── Right-click outside mouse mode: copy selection or paste ──
        window.on_mouse_event({
            let terminal = terminal.clone();
            let focus = focus.clone();
            let hitbox = hitbox.clone();
            move |e: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || e.button != MouseButton::Right
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                window.focus(&focus, cx);
                let has_selection = terminal.last_content(cx).selection.is_some();
                if has_selection {
                    if let Some(text) = terminal.copy(cx) {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }
                } else {
                    let text = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .unwrap_or_default();
                    if !text.is_empty() {
                        terminal.paste(&text, cx);
                    }
                }
            }
        });
    }
}

// =========================================================================
// Scrollbar helpers
// =========================================================================

fn scrollbar_scroll_to(
    terminal: &TerminalEntity,
    hitbox: &Hitbox,
    mouse_y: Pixels,
    cx: &mut App,
) {
    let content = terminal.last_content(cx);
    let total_lines = terminal
        .total_lines(cx)
        .max(content.bounds.num_lines());
    let total = total_lines as f32;
    let line_h = content.bounds.line_height.as_f32();
    let visible = (hitbox.bounds.size.height.as_f32() / line_h).max(1.0);
    if total <= visible {
        return;
    }
    let track_h = hitbox.bounds.size.height.as_f32();
    let thumb_h = (visible / total * track_h).max(20.0);
    let click_ratio = ((mouse_y - hitbox.bounds.origin.y).as_f32()
        / (track_h - thumb_h))
        .clamp(0.0, 1.0);
    let target = ((1.0 - click_ratio) * (total - visible)) as usize;
    let current = content.display_offset;
    let delta = target as i32 - current as i32;
    if delta != 0 {
        let event = ScrollWheelEvent {
            position: point(hitbox.bounds.origin.x, mouse_y),
            delta: ScrollDelta::Pixels(point(px(0.), px(delta as f32 * line_h))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::default(),
        };
        terminal.scroll_wheel(&event, cx);
    }
}

// =========================================================================
// Prerequisite checks
// =========================================================================

fn check_mouse_down(
    phase: DispatchPhase,
    button: MouseButton,
    expected: MouseButton,
    hitbox: &Hitbox,
    window: &mut Window,
) -> bool {
    phase == DispatchPhase::Bubble && button == expected && hitbox.is_hovered(window)
}

fn check_mouse_up(
    phase: DispatchPhase,
    button: MouseButton,
    expected: MouseButton,
    focus: &FocusHandle,
    hitbox: &Hitbox,
    window: &mut Window,
) -> bool {
    phase == DispatchPhase::Bubble
        && button == expected
        && focus.is_focused(window)
        && hitbox.is_hovered(window)
}
