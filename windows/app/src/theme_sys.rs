//! Whether Windows is currently set to the dark app theme.

use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

/// `true` when "Choose your default app mode" is Dark. `None` when the setting cannot be read.
pub fn system_uses_dark_theme() -> Option<bool> {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are valid NUL-terminated wide literals; `value` and `size` are valid
    // out pointers for a 4-byte DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    status.is_ok().then_some(value == 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reading_the_theme_does_not_panic() {
        let _ = super::system_uses_dark_theme();
    }
}
