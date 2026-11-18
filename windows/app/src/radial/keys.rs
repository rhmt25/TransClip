//! Trigger key names for the config file and key recording for the settings window.
//!
//! The config stores a stable name (`RControl`, `F8`, `A`, `VK:7B`, ...), not a raw number, so the
//! file stays readable. Left and right modifier keys are told apart, which egui cannot do.

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Default trigger.
pub const DEFAULT: &str = "Control";

/// Names with the virtual-key codes that count as "pressed". Left and right modifier keys are one
/// trigger: pressing either side works.
const NAMED: &[(&str, &[i32])] = &[
    ("Shift", &[0x10]),
    ("Control", &[0x11]),
    ("Alt", &[0x12]),
    ("Win", &[0x5B, 0x5C]),
    ("Tab", &[0x09]),
    ("CapsLock", &[0x14]),
    ("Space", &[0x20]),
    ("PageUp", &[0x21]),
    ("PageDown", &[0x22]),
    ("End", &[0x23]),
    ("Home", &[0x24]),
    ("Insert", &[0x2D]),
    ("Delete", &[0x2E]),
    ("Menu", &[0x5D]),
];

/// Names written by older versions, which told left and right apart.
fn legacy(name: &str) -> &str {
    match name {
        "LShift" | "RShift" => "Shift",
        "LControl" | "RControl" => "Control",
        "LAlt" | "RAlt" => "Alt",
        "LWin" | "RWin" => "Win",
        other => other,
    }
}

/// Virtual-key codes for a config name, or `None` when the name is not valid.
pub fn name_to_vks(name: &str) -> Option<Vec<i32>> {
    let name = legacy(name);
    if let Some((_, vks)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(vks.to_vec());
    }
    if let Some(rest) = name.strip_prefix("VK:") {
        let v = i32::from_str_radix(rest, 16).ok()?;
        return (0x08..=0xFE).contains(&v).then(|| vec![v]);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|d| d.parse::<i32>().ok()) {
        return (1..=24).contains(&n).then(|| vec![0x70 + n - 1]);
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(vec![c as i32]);
        }
    }
    None
}

/// Codes for a name; an invalid name gives the default trigger.
pub fn vks_or_default(name: &str) -> Vec<i32> {
    name_to_vks(name)
        .or_else(|| name_to_vks(DEFAULT))
        .unwrap_or_else(|| vec![0x11])
}

/// Config name for a virtual-key code (left and right variants share a name).
pub fn vk_to_name(vk: i32) -> String {
    let vk = match vk {
        0xA0 | 0xA1 => 0x10,
        0xA2 | 0xA3 => 0x11,
        0xA4 | 0xA5 => 0x12,
        other => other,
    };
    if let Some((n, _)) = NAMED.iter().find(|(_, v)| v.contains(&vk)) {
        return (*n).to_owned();
    }
    match vk {
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x30..=0x39 | 0x41..=0x5A => char::from(u8::try_from(vk).unwrap_or(b'?')).to_string(),
        _ => format!("VK:{vk:02X}"),
    }
}

/// The current spelling of a valid name (older names become the shared ones), else `None`.
pub fn canonical(name: &str) -> Option<String> {
    name_to_vks(name).map(|v| vk_to_name(v[0]))
}

/// Text shown in the settings window.
pub fn display(name: &str) -> String {
    match legacy(name) {
        "Control" => "Ctrl (kiri atau kanan)".into(),
        "Shift" => "Shift (kiri atau kanan)".into(),
        "Alt" => "Alt (kiri atau kanan)".into(),
        "Win" => "Win (kiri atau kanan)".into(),
        other => other.replace("VK:", "Tombol 0x"),
    }
}

fn down(vk: i32) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

/// Keys that must not be recorded: mouse buttons, Esc (cancels) and the side-agnostic modifier
/// codes that are reported together with their left or right variants.
fn recordable(vk: i32) -> bool {
    !matches!(vk, 0x01..=0x06 | 0x1B | 0x10..=0x12 | 0x00 | 0xFF)
}

/// The first recordable key that is currently held, if any.
pub fn pressed_key() -> Option<i32> {
    (0x08..=0xFE).find(|&vk| recordable(vk) && down(vk))
}

/// True while Esc is held.
pub fn escape_down() -> bool {
    down(0x1B)
}

/// True while any recordable key is held (used to wait for the previous click to be released).
pub fn any_held() -> bool {
    pressed_key().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for name in [
            "Control", "Shift", "Alt", "Win", "F1", "F12", "F24", "A", "Z", "0", "9", "Space",
            "VK:C0",
        ] {
            assert_eq!(canonical(name).as_deref(), Some(name), "{name}");
        }
    }

    #[test]
    fn left_and_right_are_the_same_trigger() {
        assert_eq!(vk_to_name(0xA2), vk_to_name(0xA3));
        assert_eq!(vk_to_name(0xA2), "Control");
        assert_eq!(vk_to_name(0xA0), "Shift");
        assert_eq!(vk_to_name(0xA5), "Alt");
        assert_eq!(vks_or_default("Win"), vec![0x5B, 0x5C]);
    }

    #[test]
    fn older_config_names_are_upgraded() {
        assert_eq!(canonical("RControl").as_deref(), Some("Control"));
        assert_eq!(canonical("LShift").as_deref(), Some("Shift"));
        assert_eq!(canonical("RAlt").as_deref(), Some("Alt"));
    }

    #[test]
    fn invalid_names_fall_back_to_the_default() {
        assert_eq!(canonical("???"), None);
        assert_eq!(canonical("VK:ZZ"), None);
        assert_eq!(canonical("F99"), None);
        assert_eq!(vks_or_default("???"), vec![0x11]);
    }

    #[test]
    fn recording_ignores_mouse_buttons_and_escape() {
        assert!(!recordable(0x01));
        assert!(!recordable(0x02));
        assert!(!recordable(0x1B));
        assert!(recordable(0xA3));
        assert!(recordable(0x77));
    }

    #[test]
    fn display_is_readable() {
        assert_eq!(display("Control"), "Ctrl (kiri atau kanan)");
        assert_eq!(display("RControl"), "Ctrl (kiri atau kanan)");
        assert_eq!(display("F8"), "F8");
        assert_eq!(display("VK:C0"), "Tombol 0xC0");
    }
}
