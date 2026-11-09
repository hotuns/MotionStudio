//! Original pure HID keyboard lookup from keymouse_hm.rs, c69f53ad (MIT).
//! Exposed for headless sinks; device creation and GUI remain excluded.
pub fn key_name_to_hid_usage(name: &str) -> Option<u8> {
    let rest = name.strip_prefix("key_").unwrap_or(name);
    let n = rest.to_ascii_lowercase();
    // Single character: letter or top-row digit.
    if n.len() == 1 {
        let c = n.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(0x04 + (c as u8 - b'a'));
        }
        if c.is_ascii_digit() {
            // '1'..'9' → 0x1E..0x26, '0' → 0x27.
            return Some(if c == '0' { 0x27 } else { 0x1E + (c as u8 - b'1') });
        }
        return None;
    }
    // egui names its top-row digits Num0..Num9 (it has no numpad variants), so
    // "numN" is the digit row too — matching what the enigo backend sends.
    if let Some(d) = n.strip_prefix("num") {
        if d.len() == 1 {
            let c = d.chars().next()?;
            if c.is_ascii_digit() {
                return Some(if c == '0' { 0x27 } else { 0x1E + (c as u8 - b'1') });
            }
        }
    }
    if let Some(f) = n.strip_prefix('f') {
        if let Ok(num) = f.parse::<u8>() {
            return match num {
                1..=12 => Some(0x3A + (num - 1)),  // F1..F12
                13..=20 => Some(0x68 + (num - 13)), // F13..F20
                _ => None,
            };
        }
    }
    Some(match n.as_str() {
        "enter" | "return" => 0x28,
        "escape" => 0x29,
        "backspace" => 0x2A,
        "tab" => 0x2B,
        "space" => 0x2C,
        "minus" => 0x2D,
        "plus" | "equals" => 0x2E,
        "openbracket" => 0x2F,
        "closebracket" => 0x30,
        "backslash" => 0x31,
        "semicolon" => 0x33,
        "quote" => 0x34,
        "backtick" => 0x35,
        "comma" => 0x36,
        "period" => 0x37,
        "slash" => 0x38,
        "capslock" => 0x39,
        "printscreen" => 0x46,
        "pause" => 0x48,
        "insert" => 0x49,
        "home" => 0x4A,
        "pageup" => 0x4B,
        "delete" => 0x4C,
        "end" => 0x4D,
        "pagedown" => 0x4E,
        "arrowright" => 0x4F,
        "arrowleft" => 0x50,
        "arrowdown" => 0x51,
        "arrowup" => 0x52,
        _ => return None,
    })
}
