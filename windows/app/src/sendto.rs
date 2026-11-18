//! The Explorer "Send to" shortcut (PLAN section 5.5), created through the WScript.Shell
//! automation object so no raw COM code is needed.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

fn shortcut_path() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
    Ok(PathBuf::from(appdata)
        .join("Microsoft")
        .join("Windows")
        .join("SendTo")
        .join("TransClip.lnk"))
}

/// PowerShell single-quoted literal.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Create or remove the shortcut.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let lnk = shortcut_path()?;
    if !enabled {
        match std::fs::remove_file(&lnk) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("cannot remove the SendTo shortcut"),
        }
        return Ok(());
    }
    let exe = std::env::current_exe().context("cannot find our own executable")?;
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({lnk}); \
         $s.TargetPath={exe}; $s.Arguments='--send-ui'; $s.IconLocation={exe}; $s.Save()",
        lnk = ps_quote(&lnk.to_string_lossy()),
        exe = ps_quote(&exe.to_string_lossy()),
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

/// Whether the shortcut currently exists.
pub fn is_enabled() -> bool {
    shortcut_path().is_ok_and(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_doubles_single_quotes() {
        assert_eq!(ps_quote("a'b"), "'a''b'");
    }
}
