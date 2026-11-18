//! Shell shortcuts created through the WScript.Shell automation object, so no raw COM code is
//! needed: the Explorer "Send to" entry (PLAN section 5.5) and the Start menu entry that makes
//! TransClip appear in Windows Search.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

fn appdata_path(parts: &[&str]) -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
    let mut p = PathBuf::from(appdata);
    p.extend(parts);
    Ok(p)
}

fn send_to_path() -> Result<PathBuf> {
    appdata_path(&["Microsoft", "Windows", "SendTo", "TransClip.lnk"])
}

fn start_menu_path() -> Result<PathBuf> {
    appdata_path(&[
        "Microsoft",
        "Windows",
        "Start Menu",
        "Programs",
        "TransClip.lnk",
    ])
}

/// PowerShell single-quoted literal.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn remove(lnk: &Path) -> Result<()> {
    match std::fs::remove_file(lnk) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("cannot remove the shortcut"),
    }
}

/// Write a `.lnk` that starts this executable with `args`.
fn create(lnk: &Path, args: &str, description: &str) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find our own executable")?;
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({lnk}); \
         $s.TargetPath={exe}; $s.Arguments={args}; $s.IconLocation={exe}; \
         $s.Description={desc}; $s.WorkingDirectory={dir}; $s.Save()",
        lnk = ps_quote(&lnk.to_string_lossy()),
        exe = ps_quote(&exe.to_string_lossy()),
        args = ps_quote(args),
        desc = ps_quote(description),
        dir = ps_quote(
            &exe.parent()
                .map_or_else(String::new, |p| p.to_string_lossy().into_owned())
        ),
    );
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .status()
        .context("cannot run PowerShell")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("PowerShell could not create the shortcut"))
    }
}

/// Create or remove the "Send to" shortcut.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let lnk = send_to_path()?;
    if enabled {
        create(
            &lnk,
            "--send-ui",
            "Kirim ke perangkat lain dengan TransClip",
        )
    } else {
        remove(&lnk)
    }
}

/// Whether the "Send to" shortcut currently exists.
pub fn is_enabled() -> bool {
    send_to_path().is_ok_and(|p| p.exists())
}

/// Create or remove the Start menu shortcut. Windows Search indexes the Start menu, so this is
/// what makes "TransClip" findable there. Launching it opens the settings window (a second
/// launch asks the running daemon to show it).
pub fn set_start_menu(enabled: bool) -> Result<()> {
    let lnk = start_menu_path()?;
    if enabled {
        create(&lnk, "", "Sinkronisasi clipboard dan file antar perangkat")
    } else {
        remove(&lnk)
    }
}

/// Whether the Start menu shortcut currently exists.
pub fn start_menu_enabled() -> bool {
    start_menu_path().is_ok_and(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_doubles_single_quotes() {
        assert_eq!(ps_quote("a'b"), "'a''b'");
    }

    #[test]
    fn shortcut_paths_end_with_the_expected_names() {
        if let Ok(p) = send_to_path() {
            assert!(p.ends_with("SendTo\\TransClip.lnk"));
        }
        if let Ok(p) = start_menu_path() {
            assert!(p.ends_with("Programs\\TransClip.lnk"));
        }
    }
}
