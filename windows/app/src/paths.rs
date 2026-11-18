//! Data locations (PLAN section 5.2).

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use windows::core::GUID;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Downloads, FOLDERID_LocalAppData, FOLDERID_RoamingAppData, SHGetKnownFolderPath,
    KF_FLAG_DEFAULT,
};

/// Environment variable that moves all data under one folder (tests, portable use).
pub const DATA_DIR_ENV: &str = "TRANSCLIP_DATA_DIR";

/// All folders the daemon reads and writes.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `%APPDATA%\TransClip`
    pub config_dir: PathBuf,
    /// `%LOCALAPPDATA%\TransClip`
    pub local_dir: PathBuf,
}

impl Paths {
    /// Resolve from the shell known folders, or from `TRANSCLIP_DATA_DIR` when set.
    pub fn discover() -> Result<Self> {
        if let Some(root) = std::env::var_os(DATA_DIR_ENV) {
            return Ok(Self::under(Path::new(&root)));
        }
        Ok(Self {
            config_dir: known_folder(&FOLDERID_RoamingAppData)?.join("TransClip"),
            local_dir: known_folder(&FOLDERID_LocalAppData)?.join("TransClip"),
        })
    }

    /// Both folders below `root`.
    pub fn under(root: &Path) -> Self {
        Self {
            config_dir: root.join("roaming"),
            local_dir: root.join("local"),
        }
    }

    /// `config.toml`
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// `secrets.bin` (DPAPI protected)
    pub fn secrets_file(&self) -> PathBuf {
        self.config_dir.join("secrets.bin")
    }

    /// `peers.json`
    pub fn peers_file(&self) -> PathBuf {
        self.config_dir.join("peers.json")
    }

    /// Log folder.
    pub fn log_dir(&self) -> PathBuf {
        self.local_dir.join("logs")
    }

    /// Image cache folder.
    pub fn cache_dir(&self) -> PathBuf {
        self.local_dir.join("cache")
    }

    /// Create the folders that must exist.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.local_dir,
            &self.log_dir(),
            &self.cache_dir(),
        ] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        Ok(())
    }
}

/// `<Downloads>\TransClip`, the default receive folder (PLAN section 4.5).
pub fn default_receive_dir() -> Result<PathBuf> {
    Ok(known_folder(&FOLDERID_Downloads)?.join("TransClip"))
}

/// Free bytes available to this user on the volume holding `dir` (or its nearest existing parent).
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut probe = Some(dir);
    while let Some(p) = probe {
        if p.exists() {
            let wide: Vec<u16> = p
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut free = 0u64;
            // SAFETY: `wide` is NUL-terminated and outlives the call; `free` is a valid out pointer
            // and the two optional out pointers are null.
            let ok = unsafe {
                windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                    windows::core::PCWSTR(wide.as_ptr()),
                    Some(&raw mut free),
                    None,
                    None,
                )
            };
            return ok.ok().map(|()| free);
        }
        probe = p.parent();
    }
    None
}

fn known_folder(id: &GUID) -> Result<PathBuf> {
    // SAFETY: `id` points to a valid GUID. On success the returned PWSTR is a NUL-terminated
    // string allocated by the shell; it is copied before being freed with CoTaskMemFree.
    unsafe {
        let pwstr = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
            .context("SHGetKnownFolderPath failed")?;
        let path = PathBuf::from(OsString::from_wide(pwstr.as_wide()));
        CoTaskMemFree(Some(pwstr.0.cast_const().cast()));
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_folders_resolve_to_absolute_paths() {
        let dl = default_receive_dir().unwrap();
        assert!(dl.is_absolute());
        assert!(dl.ends_with("TransClip"));
        assert!(known_folder(&FOLDERID_RoamingAppData)
            .unwrap()
            .is_absolute());
    }

    #[test]
    fn layout_matches_the_plan() {
        let p = Paths::under(Path::new("C:\\x"));
        assert!(p.config_file().ends_with("roaming\\config.toml"));
        assert!(p.log_dir().ends_with("local\\logs"));
        assert!(p.secrets_file().ends_with("secrets.bin"));
        assert!(p.peers_file().ends_with("peers.json"));
    }

    #[test]
    fn ensure_creates_the_folders() {
        let dir = tempfile::tempdir().unwrap();
        let p = Paths::under(dir.path());
        p.ensure().unwrap();
        assert!(p.log_dir().is_dir() && p.cache_dir().is_dir() && p.config_dir.is_dir());
    }
}
