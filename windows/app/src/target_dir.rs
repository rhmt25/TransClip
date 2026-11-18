//! Effective receive folder (PLAN section 4.5): the configured one when it is usable,
//! otherwise the default `Downloads\TransClip`.

use std::path::{Path, PathBuf};

/// The folder to receive into and whether the configured one had to be replaced.
#[derive(Debug, PartialEq, Eq)]
pub struct Resolved {
    /// Folder to write into.
    pub dir: PathBuf,
    /// True when a custom folder was configured but is not usable.
    pub fell_back: bool,
}

fn default_dir() -> PathBuf {
    crate::paths::default_receive_dir().unwrap_or_else(|_| std::env::temp_dir().join("TransClip"))
}

/// Check that `dir` is absolute, can be created and can be written to.
pub fn validate(dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() {
        return Err("the folder path must be absolute".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the folder: {e}"))?;
    let probe = dir.join(format!(".transclip-probe-{}", std::process::id()));
    std::fs::write(&probe, b"x").map_err(|e| format!("the folder is not writable: {e}"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Resolve the configured value (`""` means the default folder).
pub fn resolve(configured: &str) -> Resolved {
    if configured.is_empty() {
        let dir = default_dir();
        let _ = std::fs::create_dir_all(&dir);
        return Resolved {
            dir,
            fell_back: false,
        };
    }
    let custom = PathBuf::from(configured);
    if validate(&custom).is_ok() {
        return Resolved {
            dir: custom,
            fell_back: false,
        };
    }
    let dir = default_dir();
    let _ = std::fs::create_dir_all(&dir);
    Resolved {
        dir,
        fell_back: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tc-td-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn writable_folder_is_used_and_created() {
        let d = tmp("ok").join("inner");
        let r = resolve(d.to_str().unwrap_or_default());
        assert_eq!(r.dir, d);
        assert!(!r.fell_back && d.is_dir());
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn relative_path_is_rejected_and_falls_back() {
        assert!(validate(Path::new("relative\\dir")).is_err());
        assert!(resolve("relative\\dir").fell_back);
    }

    #[test]
    fn unusable_folder_falls_back_to_default() {
        // A file in the way makes the folder impossible to create.
        let base = tmp("bad");
        std::fs::create_dir_all(&base).unwrap();
        let file = base.join("f");
        std::fs::write(&file, b"x").unwrap();
        let r = resolve(file.join("sub").to_str().unwrap_or_default());
        assert!(r.fell_back);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn empty_means_default_without_fallback() {
        assert!(!resolve("").fell_back);
    }

    #[test]
    fn free_space_reports_something_for_temp() {
        assert!(crate::paths::free_space(&std::env::temp_dir()).is_some_and(|b| b > 0));
    }
}
