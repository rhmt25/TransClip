//! User-visible text. Kept in one place so an English set can be added later.

pub const APP_NAME: &str = "TransClip";
pub const MENU_SEND_FILE: &str = "Kirim file\u{2026}";
pub const MENU_PAUSE: &str = "Jeda sinkronisasi";
pub const MENU_SETTINGS: &str = "Pengaturan";
pub const MENU_PAIR: &str = "Pairing\u{2026}";
pub const MENU_OPEN_DOWNLOADS: &str = "Buka folder unduhan";
pub const MENU_QUIT: &str = "Keluar";

/// Status line for the menu and tooltip.
pub fn status_line(online: usize, paired: bool, paused: bool) -> String {
    if !paired {
        "Belum dipasangkan dengan perangkat lain".to_string()
    } else if paused {
        "Sinkronisasi dijeda".to_string()
    } else if online == 0 {
        "Tidak ada perangkat online".to_string()
    } else {
        format!("{online} perangkat online")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_covers_every_state() {
        assert!(status_line(0, false, false).contains("Belum"));
        assert!(status_line(2, true, true).contains("dijeda"));
        assert!(status_line(0, true, false).contains("Tidak ada"));
        assert_eq!(status_line(3, true, false), "3 perangkat online");
    }
}
