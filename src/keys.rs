//! Keystroke → bytes conversion shared between PTY and SSH terminals.

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

/// Convert a GPUI keystroke to raw bytes for the terminal.
///
/// `mode` is the current terminal mode flags — used for application
/// cursor keys, application keypad, and Kitty keyboard protocol.
pub(crate) fn keystroke_to_bytes(keystroke: &Keystroke, mode: TermMode) -> Vec<u8> {
    // ── Kitty keyboard protocol ──
    if mode.intersects(
        TermMode::DISAMBIGUATE_ESC_CODES
            | TermMode::REPORT_ALL_KEYS_AS_ESC
            | TermMode::REPORT_EVENT_TYPES,
    ) {
        return kitty_encode(keystroke, mode);
    }

    let modifiers = &keystroke.modifiers;
    let ctrl_only = modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform;

    // ── Enter/Return must always be \r regardless of key_char ──
    if keystroke.key.as_str() == "enter" || keystroke.key.as_str() == "return" {
        if modifiers.alt {
            return b"\x1b\r".to_vec();
        }
        if ctrl_only {
            return b"\n".to_vec();
        }
        return b"\r".to_vec();
    }

    // ── Ctrl+letter → control character ──
    if ctrl_only {
        if let Some(ref ch) = keystroke.key_char {
            if ch.len() == 1 {
                let c = ch.chars().next().unwrap();
                let ctrl = char_to_ctrl(c);
                if !ctrl.is_empty() {
                    return ctrl;
                }
            }
        }
        if keystroke.key.len() == 1 {
            let c = keystroke.key.chars().next().unwrap();
            let ctrl = char_to_ctrl(c);
            if !ctrl.is_empty() {
                return ctrl;
            }
        }
        return match keystroke.key.as_str() {
            "backspace" => b"\x7f".to_vec(),
            "delete" => b"\x1b[3;5~".to_vec(),
            _ => Vec::new(),
        };
    }

    // ── Alt combination → ESC prefix ──
    if modifiers.alt && !modifiers.platform {
        let inner = keystroke_plain(keystroke, mode, modifiers);
        if !inner.is_empty() {
            // For Alt+Shift — do ESC + shifted_char; for Alt+Ctrl — ESC + ctrl_char
            let mut out = vec![0x1b];
            out.extend(inner);
            return out;
        }
    }

    // ── Regular characters ──
    if !modifiers.control && !modifiers.alt && !modifiers.platform {
        if let Some(ref ch) = keystroke.key_char {
            if !ch.is_empty() {
                return ch.as_bytes().to_vec();
            }
        }
        if keystroke.key.len() == 1 && modifiers.shift {
            return keystroke.key.as_bytes().to_vec();
        }
        if keystroke.key.len() == 1
            && !modifiers.shift
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
        {
            return keystroke.key.as_bytes().to_vec();
        }
    }

    // ── Special keys ──
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let app_keypad = mode.contains(TermMode::APP_KEYPAD);
    let mod1 = modifier_csi_param(modifiers);

    match keystroke.key.as_str() {
        "backspace" => b"\x7f".to_vec(),
        "tab" => {
            if modifiers.shift { b"\x1b[Z".to_vec() } else { b"\t".to_vec() }
        }
        "escape" => b"\x1b".to_vec(),
        "space" => b" ".to_vec(),

        // Arrow keys — with modifiers use CSI; bare use SS3 in app cursor
        "up" => arrow_seq("A", &mod1, modifiers, app_cursor),
        "down" => arrow_seq("B", &mod1, modifiers, app_cursor),
        "right" => arrow_seq("C", &mod1, modifiers, app_cursor),
        "left" => arrow_seq("D", &mod1, modifiers, app_cursor),
        "home" => if modifier_present(modifiers) || !app_cursor {
            format!("\x1b[{mod1}H").into_bytes()
        } else {
            b"\x1bOH".to_vec()
        },
        "end" => if modifier_present(modifiers) || !app_cursor {
            format!("\x1b[{mod1}F").into_bytes()
        } else {
            b"\x1bOF".to_vec()
        },

        "pageup" => format!("\x1b[{mod1}5~").into_bytes(),
        "pagedown" => format!("\x1b[{mod1}6~").into_bytes(),
        "delete" => format!("\x1b[{mod1}3~").into_bytes(),
        "insert" => format!("\x1b[{mod1}2~").into_bytes(),

        // Function keys F1-F20 with full modifier support
        "f1" => format!("\x1b[{mod1}P").into_bytes(),
        "f2" => format!("\x1b[{mod1}Q").into_bytes(),
        "f3" => format!("\x1b[{mod1}R").into_bytes(),
        "f4" => format!("\x1b[{mod1}S").into_bytes(),
        "f5" => format!("\x1b[{mod1}15~").into_bytes(),
        "f6" => format!("\x1b[{mod1}17~").into_bytes(),
        "f7" => format!("\x1b[{mod1}18~").into_bytes(),
        "f8" => format!("\x1b[{mod1}19~").into_bytes(),
        "f9" => format!("\x1b[{mod1}20~").into_bytes(),
        "f10" => format!("\x1b[{mod1}21~").into_bytes(),
        "f11" => format!("\x1b[{mod1}23~").into_bytes(),
        "f12" => format!("\x1b[{mod1}24~").into_bytes(),
        "f13" => format!("\x1b[{mod1}25~").into_bytes(),
        "f14" => format!("\x1b[{mod1}26~").into_bytes(),
        "f15" => format!("\x1b[{mod1}28~").into_bytes(),
        "f16" => format!("\x1b[{mod1}29~").into_bytes(),
        "f17" => format!("\x1b[{mod1}31~").into_bytes(),
        "f18" => format!("\x1b[{mod1}32~").into_bytes(),
        "f19" => format!("\x1b[{mod1}33~").into_bytes(),
        "f20" => format!("\x1b[{mod1}34~").into_bytes(),

        // Numpad keys — application vs normal mode
        "kp0" => numpad("0", "p", app_keypad),
        "kp1" => numpad("1", "q", app_keypad),
        "kp2" => numpad("2", "r", app_keypad),
        "kp3" => numpad("3", "s", app_keypad),
        "kp4" => numpad("4", "t", app_keypad),
        "kp5" => numpad("5", "u", app_keypad),
        "kp6" => numpad("6", "v", app_keypad),
        "kp7" => numpad("7", "w", app_keypad),
        "kp8" => numpad("8", "x", app_keypad),
        "kp9" => numpad("9", "y", app_keypad),
        "kpadd" => numpad("+", "k", app_keypad),
        "kpsubtract" => numpad("-", "m", app_keypad),
        "kpmultiply" => numpad("*", "j", app_keypad),
        "kpdivide" => numpad("/", "o", app_keypad),
        "kpdecimal" => numpad(".", "n", app_keypad),
        "kpenter" => numpad("\r", "M", app_keypad),

        _ => Vec::new(),
    }
}

// =========================================================================
// Kitty keyboard protocol
// =========================================================================

/// Encode a keystroke using the Kitty keyboard protocol (CSI u format).
///
/// Format: `ESC [ {key} ; {mods} {event} u`
/// where `key` is a Unicode codepoint or Kitty functional key code,
/// `mods` is an encoded bitmask, and `event` is optional (press/repeat/release).
fn kitty_encode(keystroke: &Keystroke, mode: TermMode) -> Vec<u8> {
    let mods = kitty_modifier_mask(&keystroke.modifiers);
    let event = if mode.contains(TermMode::REPORT_EVENT_TYPES) { "1" } else { "" };

    // Printable characters: use Unicode codepoint as key
    if let Some(ref ch) = keystroke.key_char {
        if ch.len() == 1 {
            let cp = ch.chars().next().unwrap() as u32;
            return format!("\x1b[{cp};{mods}{event}u").into_bytes();
        }
    }
    if keystroke.key.len() == 1 {
        let cp = keystroke.key.chars().next().unwrap() as u32;
        return format!("\x1b[{cp};{mods}{event}u").into_bytes();
    }

    // Special keys — map to Kitty functional key codes
    let code: u32 = match keystroke.key.as_str() {
        "escape" => 57344,
        "enter" | "return" => 13,
        "tab" => 9,
        "backspace" => 127,
        "space" => 32,
        "delete" => 57356,
        "insert" => 57355,

        "up" => 57345,
        "down" => 57346,
        "right" => 57347,
        "left" => 57348,
        "home" => 57349,
        "end" => 57350,
        "pageup" => 57353,
        "pagedown" => 57354,

        // Function keys F1-F20
        "f1" => 57376,  "f2" => 57377,  "f3" => 57378,
        "f4" => 57379,  "f5" => 57380,  "f6" => 57381,
        "f7" => 57382,  "f8" => 57383,  "f9" => 57384,
        "f10" => 57385, "f11" => 57386, "f12" => 57387,
        "f13" => 57388, "f14" => 57389, "f15" => 57390,
        "f16" => 57391, "f17" => 57392, "f18" => 57393,
        "f19" => 57394, "f20" => 57395,

        // Numpad
        "kp0" => 57400, "kp1" => 57401, "kp2" => 57402,
        "kp3" => 57403, "kp4" => 57404, "kp5" => 57405,
        "kp6" => 57406, "kp7" => 57407, "kp8" => 57408,
        "kp9" => 57409, "kpdecimal" => 57410, "kpenter" => 57411,
        "kpadd" => 57412, "kpsubtract" => 57413,
        "kpmultiply" => 57414, "kpdivide" => 57415,

        _ => return Vec::new(),
    };

    format!("\x1b[{code};{mods}{event}u").into_bytes()
}

/// Encode GPUI modifiers as Kitty bitmask: Shift=1, Alt=2, Ctrl=4, Super=8.
fn kitty_modifier_mask(mods: &gpui::Modifiers) -> u8 {
    let mut mask = 0u8;
    if mods.shift { mask += 1; }
    if mods.alt { mask += 2; }
    if mods.control { mask += 4; }
    if mods.platform { mask += 8; }
    mask
}

// =========================================================================
// Arrow key helper
// =========================================================================

/// Arrow key sequence: CSI in normal mode, SS3 in app cursor without modifiers.
fn arrow_seq(
    letter: &str,
    mod1: &str,
    modifiers: &gpui::Modifiers,
    app_cursor: bool,
) -> Vec<u8> {
    if app_cursor && !modifier_present(modifiers) {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{mod1}{letter}").into_bytes()
    }
}

/// Whether any modifiers (shift/alt/ctrl/platform) are pressed.
fn modifier_present(mods: &gpui::Modifiers) -> bool {
    mods.shift || mods.alt || mods.control || mods.platform
}

// =========================================================================
// Alt-key helpers
// =========================================================================

/// Inner keystroke processing for Alt+key prefix — regular character
/// or special key without the Alt modifier, but keeping Shift/Ctrl.
fn keystroke_plain(keystroke: &Keystroke, mode: TermMode, mods: &gpui::Modifiers) -> Vec<u8> {
    // Alt+Shift+char → return the shifted char directly (ESC + uppercase)
    if let Some(ref ch) = keystroke.key_char {
        if !ch.is_empty() {
            return ch.as_bytes().to_vec();
        }
    }
    if keystroke.key.len() == 1 {
        return keystroke.key.as_bytes().to_vec();
    }
    // Alt+Ctrl+char → try ctrl char
    if mods.control {
        if let Some(ref ch) = keystroke.key_char {
            if ch.len() == 1 {
                let c = ch.chars().next().unwrap();
                let ctrl = char_to_ctrl(c);
                if !ctrl.is_empty() {
                    return ctrl;
                }
            }
        }
    }
    // Alt+special key → pass without Alt modifier
    let mut m = *mods;
    m.alt = false;
    keystroke_to_bytes(
        &Keystroke {
            modifiers: m,
            key: keystroke.key.clone(),
            key_char: keystroke.key_char.clone(),
        },
        mode,
    )
}

// =========================================================================
// Numpad
// =========================================================================

/// Numpad key — numeric in normal mode, escape sequence in app mode.
fn numpad(normal: &str, app: &str, app_cursor: bool) -> Vec<u8> {
    if app_cursor {
        format!("\x1bO{app}").into_bytes()
    } else {
        normal.as_bytes().to_vec()
    }
}

// =========================================================================
// Control character mapping
// =========================================================================

/// Convert a character to its control equivalent.
fn char_to_ctrl(c: char) -> Vec<u8> {
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
        let ctrl = ((c.to_ascii_lowercase() as u8) - b'a' + 1) as char;
        return ctrl.to_string().into_bytes();
    }
    match c {
        '2' | '@' => b"\x00".to_vec(),    // Ctrl+2 / Ctrl+@ → NUL
        '3' => b"\x1b".to_vec(),           // Ctrl+3 → ESC
        '4' => b"\x1c".to_vec(),           // Ctrl+4 → FS
        '5' => b"\x1d".to_vec(),           // Ctrl+5 → GS
        '6' => b"\x1e".to_vec(),           // Ctrl+6 → RS
        '7' => b"\x1f".to_vec(),           // Ctrl+7 → US
        '8' => b"\x7f".to_vec(),           // Ctrl+8 → DEL
        '[' | '\x1b' => b"\x1b".to_vec(),
        '\\' => b"\x1c".to_vec(),
        ']' => b"\x1d".to_vec(),
        '^' => b"\x1e".to_vec(),
        '_' => b"\x1f".to_vec(),
        '/' => b"\x1f".to_vec(),
        ' ' => b"\x00".to_vec(),
        _ => Vec::new(),
    }
}

// =========================================================================
// CSI modifier parameter
// =========================================================================

/// Format the modifier part of a CSI sequence.
/// Returns e.g. "1;" for Alt, "1;2" for Alt+Shift, "" for none.
fn modifier_csi_param(modifiers: &gpui::Modifiers) -> String {
    let mut n = 1u8;
    if modifiers.shift { n += 1; }
    if modifiers.alt { n += 2; }
    if modifiers.control { n += 4; }
    if modifiers.platform { n += 8; }
    if n == 1 { String::new() } else { format!("1;{n}") }
}
