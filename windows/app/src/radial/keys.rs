//! Trigger key names for the config file and key recording for the settings window.
//!
//! The config stores a stable name (`RControl`, `F8`, `A`, `VK:7B`, ...), not a raw number, so the
//! file stays readable. Left and right modifier keys are told apart, which egui cannot do.

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Default trigger.
pub const DEFAULT: &str = "RControl";
const DEFAULT_VK: i32 = 0xA3;

const NAMED: &[(&str, i32)] = &[
    ("LShift", 0xA0),
    ("RShift", 0xA1),
    ("LControl", 0xA2),
    ("RControl", 0xA3),
    ("LAlt", 0xA4),
    ("RAlt", 0xA5),
    ("Tab", 0x09),
    ("CapsLock", 0x14),
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("LWin", 0x5B),
    ("RWin", 0x5C),
    ("Menu", 0x5D),
];

/// Virtual-key code for a config name; unknown names give the default trigger.
pub fn name_to_vk(name: &str) -> i32 {
    if let Some((_, vk)) = NAMED.iter().find(|(n, _)| *n == name) {
        return *vk;
    }
    if let Some(rest) = name.strip_prefix("VK:") {
        if let Ok(v) = i32::from_str_radix(rest, 16) {
            if (0x08..=0xFE).contains(&v) {
                return v;
            }
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|d| d.parse::<i32>().ok()) {
        if (1..=24).contains(&n) {
            return 0x70 + n - 1;
        }
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return c as i32;
        }
    }
    DEFAULT_VK
}

/// Config name for a virtual-key code.
pub fn vk_to_name(vk: i32) -> String {
    if let Some((n, _)) = NAMED.iter().find(|(_, v)| *v == vk) {
        return (*n).to_owned();
    }
    match vk {
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x30..=0x39 | 0x41..=0x5A => char::from(u8::try_from(vk).unwrap_or(b'?')).to_string(),
        _ => format!("VK:{vk:02X}"),
    }
}

/// Text shown in the settings window.
pub fn display(name: &str) -> String {
    match name {
        "LShift" => "Shift kiri".into(),
        "RShift" => "Shift kanan".into(),
        "LControl" => "Ctrl kiri".into(),
        "RControl" => "Ctrl kanan".into(),
        "LAlt" => "Alt kiri".into(),
        "RAlt" => "Alt kanan".into(),
        "LWin" => "Win kiri".into(),
        "RWin" => "Win kanan".into(),
        other => other.replace("VK:", "Tombol 0x"),
    }
}

/// True for names the radial menu accepts (anything [`name_to_vk`] round-trips).
pub fn is_valid(name: &str) -> bool {
    vk_to_name(name_to_vk(name)) == name
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
            "RControl", "LShift", "F1", "F12", "F24", "A", "Z", "0", "9", "Space", "VK:C0",
        ] {
            assert_eq!(vk_to_name(name_to_vk(name)), name, "{name}");
        }
    }

    #[test]
    fn unknown_names_fall_back_to_the_default() {
        assert_eq!(name_to_vk("???"), DEFAULT_VK);
        assert_eq!(name_to_vk("VK:ZZ"), DEFAULT_VK);
        assert_eq!(name_to_vk("F99"), DEFAULT_VK);
        assert!(!is_valid("???"));
        assert!(is_valid("RControl"));
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
        assert_eq!(display("RControl"), "Ctrl kanan");
        assert_eq!(display("F8"), "F8");
        assert_eq!(display("VK:C0"), "Tombol 0xC0");
    }
}
