//! Start with Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\TransClip`.

use std::path::Path;

use anyhow::{anyhow, Result};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

/// Registry key that Windows reads at sign-in.
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// Value name used by TransClip.
pub const VALUE_NAME: &str = "TransClip";

fn check(code: WIN32_ERROR) -> Result<()> {
    code.ok().map_err(|e| anyhow!("registry: {e}"))
}

/// Closes the key when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by RegCreateKeyExW and is closed only here.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(subkey: &str) -> Result<Key> {
    let mut hkey = HKEY::default();
    // SAFETY: `subkey` is a valid wide string and `hkey` a valid out-pointer for the call.
    unsafe {
        check(RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(subkey),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            None,
            &mut hkey,
            None,
        ))?;
    }
    Ok(Key(hkey))
}

/// The command stored in the Run value: the path, always quoted (paths with spaces).
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Write or remove the Run value under `subkey`.
pub fn set_enabled_in(subkey: &str, enabled: bool, exe: &Path) -> Result<()> {
    let key = open(subkey)?;
    let name = HSTRING::from(VALUE_NAME);
    if enabled {
        let wide: Vec<u8> = command_for(exe)
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: `key.0` is open with KEY_SET_VALUE and `wide` is a NUL-terminated UTF-16 string.
        unsafe { check(RegSetValueExW(key.0, &name, None, REG_SZ, Some(&wide))) }
    } else {
        // SAFETY: `key.0` is open with KEY_SET_VALUE.
        let code = unsafe { RegDeleteValueW(key.0, &name) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(()); // already absent
        }
        check(code)
    }
}

/// The stored command, if the value exists (used by tests to verify the registry).
#[cfg(test)]
pub fn current_in(subkey: &str) -> Result<Option<String>> {
    use windows::Win32::System::Registry::RegQueryValueExW;
    let key = open(subkey)?;
    let name = HSTRING::from(VALUE_NAME);
    let mut size = 0u32;
    // SAFETY: first call only asks for the required size.
    let code = unsafe { RegQueryValueExW(key.0, &name, None, None, None, Some(&mut size)) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(code)?;
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` has `size` bytes, as reported by the first call.
    unsafe {
        check(RegQueryValueExW(
            key.0,
            &name,
            None,
            None,
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        ))?;
    }
    buf.truncate(size as usize);
    let units: Vec<u16> = buf
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    Ok(Some(String::from_utf16_lossy(&units)))
}

/// Apply the `autostart` setting for the real Run key.
pub fn apply(enabled: bool) -> Result<()> {
    let exe = std::env::current_exe()?;
    set_enabled_in(RUN_KEY, enabled, &exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_key(tag: &str) -> String {
        format!("Software\\TransClipTest\\{tag}-{}", std::process::id())
    }

    fn cleanup(subkey: &str) {
        use windows::Win32::System::Registry::RegDeleteKeyW;
        // SAFETY: deletes the empty test key created by the test.
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(subkey));
        }
    }

    #[test]
    fn path_with_spaces_is_quoted() {
        let p = PathBuf::from("C:\\Program Files\\Trans Clip\\transclip.exe");
        assert_eq!(
            command_for(&p),
            "\"C:\\Program Files\\Trans Clip\\transclip.exe\""
        );
    }

    #[test]
    fn enable_writes_and_disable_removes_the_value() {
        let key = test_key("toggle");
        let exe = PathBuf::from("C:\\Program Files\\Trans Clip\\transclip.exe");
        assert_eq!(current_in(&key).unwrap(), None);
        set_enabled_in(&key, true, &exe).unwrap();
        assert_eq!(current_in(&key).unwrap(), Some(command_for(&exe)));
        // Enabling twice keeps one value; disabling twice is fine.
        set_enabled_in(&key, true, &exe).unwrap();
        set_enabled_in(&key, false, &exe).unwrap();
        assert_eq!(current_in(&key).unwrap(), None);
        set_enabled_in(&key, false, &exe).unwrap();
        cleanup(&key);
    }
}
