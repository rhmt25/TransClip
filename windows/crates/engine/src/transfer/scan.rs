//! Turn files and folders on disk into batches.

use std::path::{Path, PathBuf};

use proto::msg::EntryKind;
use proto::path::validate_relative_path;

use super::{Batch, EntrySource, TransferError};

fn name_of(path: &Path) -> Result<String, TransferError> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| TransferError::Invalid("path has no usable name".into()))
}

fn walk(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<EntrySource>,
    skipped: &mut u32,
) -> Result<(), TransferError> {
    let mut children: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let ft = child.file_type()?;
        let Some(name) = child.file_name().to_str().map(str::to_string) else {
            *skipped += 1;
            continue;
        };
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if ft.is_symlink() {
            *skipped += 1; // never follow links out of the chosen folder
        } else if ft.is_dir() {
            out.push(EntrySource {
                rel: rel.clone(),
                kind: EntryKind::Dir,
                path: child.path(),
            });
            walk(&child.path(), &rel, out, skipped)?;
        } else if ft.is_file() {
            out.push(EntrySource {
                rel,
                kind: EntryKind::File,
                path: child.path(),
            });
        } else {
            *skipped += 1;
        }
    }
    Ok(())
}

/// Build one batch from the paths the user chose. A single folder becomes a folder batch named
/// after it; anything else becomes loose files at the root of the destination (folders chosen
/// together with other items are sent as their own batches by the caller).
pub fn batch_from_paths(paths: &[PathBuf]) -> Result<Vec<Batch>, TransferError> {
    let mut loose = Vec::new();
    let mut batches = Vec::new();
    for p in paths {
        let meta = std::fs::symlink_metadata(p)?;
        let name = name_of(p)?;
        validate_relative_path(&name)
            .map_err(|e| TransferError::Invalid(format!("{name}: {e}")))?;
        if meta.is_dir() {
            let mut entries = Vec::new();
            let mut skipped = 0;
            walk(p, "", &mut entries, &mut skipped)?;
            batches.push(Batch { name, entries });
        } else if meta.is_file() {
            loose.push(EntrySource {
                rel: name,
                kind: EntryKind::File,
                path: p.clone(),
            });
        }
    }
    if !loose.is_empty() {
        batches.insert(
            0,
            Batch {
                name: String::new(),
                entries: loose,
            },
        );
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_becomes_a_named_batch_with_empty_dirs_and_unicode() {
        let d = std::env::temp_dir().join(format!("tc-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let root = d.join("Foto liburan");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("kosong")).unwrap();
        std::fs::write(root.join("a/b/\u{65e5}\u{672c}.txt"), b"x").unwrap();
        std::fs::write(root.join("top.bin"), b"y").unwrap();
        let loose = d.join("lepas.txt");
        std::fs::write(&loose, b"z").unwrap();

        let batches = batch_from_paths(&[root, loose]).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].name, "");
        assert_eq!(batches[0].entries[0].rel, "lepas.txt");
        let folder = &batches[1];
        assert_eq!(folder.name, "Foto liburan");
        let rels: Vec<_> = folder
            .entries
            .iter()
            .map(|e| (e.rel.as_str(), e.kind))
            .collect();
        assert!(rels.contains(&("kosong", EntryKind::Dir)));
        assert!(rels.contains(&("a/b/\u{65e5}\u{672c}.txt", EntryKind::File)));
        // A directory always comes before what it contains.
        let pos = |r: &str| folder.entries.iter().position(|e| e.rel == r).unwrap();
        assert!(pos("a") < pos("a/b") && pos("a/b") < pos("a/b/\u{65e5}\u{672c}.txt"));
    }

    #[test]
    fn unusable_names_are_refused_before_sending() {
        let d = std::env::temp_dir().join(format!("tc-scan2-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let bad = d.join("CON");
        std::fs::write(&bad, b"x").unwrap();
        assert!(batch_from_paths(&[bad]).is_err());
    }
}
