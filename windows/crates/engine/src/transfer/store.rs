//! Where received files are written. The platform decides the location (PLAN section 4.5).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use proto::path::unique_name;
use sha2::{Digest, Sha256};

/// Suffix of a file that is still being received.
pub const PART_SUFFIX: &str = ".transclip-part";

/// A file being received. Dropping it without `finish` or `abort` keeps the part for resuming.
pub trait PartWriter: Send {
    /// Bytes already present (non-zero when resuming).
    fn len(&self) -> u64;

    /// True when nothing has been written yet.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Feed the bytes already present into `hasher` (needed to hash a resumed file).
    fn hash_existing(&mut self, hasher: &mut Sha256) -> io::Result<()>;

    /// Discard what is there and start again from offset 0.
    fn reset(&mut self) -> io::Result<()>;

    /// Append `data`.
    fn write(&mut self, data: &[u8]) -> io::Result<()>;

    /// Move the part to its final name (adding ` (n)` on conflict). Returns the final file name.
    fn finish(self: Box<Self>, mtime_secs: u64) -> io::Result<String>;

    /// Delete the part.
    fn abort(self: Box<Self>);
}

/// Receiver-side storage.
pub trait IncomingStore: Send + Sync + 'static {
    /// Prepare the folder of a batch and return its (possibly renamed) name. An empty
    /// `batch_name` means loose files and returns an empty string.
    fn begin_batch(&self, batch_name: &str) -> io::Result<String>;

    /// Create a directory entry below the batch folder.
    fn make_dir(&self, root: &str, rel: &[String]) -> io::Result<()>;

    /// Open (or resume) the part file of one entry.
    fn open_part(&self, root: &str, rel: &[String]) -> io::Result<Box<dyn PartWriter>>;

    /// Free space on the destination volume, when known.
    fn free_bytes(&self) -> Option<u64>;
}

/// Plain folder on a local disk.
/// Platform hook that reports free bytes on the volume holding a folder.
pub type FreeSpaceFn = fn(&Path) -> Option<u64>;

pub struct FsStore {
    dest: PathBuf,
    free_fn: Option<FreeSpaceFn>,
}

impl FsStore {
    /// Receive into `dest` (created when missing).
    pub fn new(dest: impl Into<PathBuf>) -> Self {
        Self {
            dest: dest.into(),
            free_fn: None,
        }
    }

    /// Answer `free_bytes` with a platform query (the engine crate has no OS dependency).
    #[must_use]
    pub fn with_free_space(mut self, f: FreeSpaceFn) -> Self {
        self.free_fn = Some(f);
        self
    }

    fn dir_of(&self, root: &str) -> PathBuf {
        if root.is_empty() {
            self.dest.clone()
        } else {
            self.dest.join(root)
        }
    }
}

fn exists_ci(dir: &Path, name: &str) -> bool {
    if dir.join(name).exists() {
        return true;
    }
    // Windows compares names without case; check explicitly so Linux-made tests behave the same.
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    })
}

impl IncomingStore for FsStore {
    fn begin_batch(&self, batch_name: &str) -> io::Result<String> {
        std::fs::create_dir_all(&self.dest)?;
        if batch_name.is_empty() {
            return Ok(String::new());
        }
        let name = unique_name(batch_name, |n| exists_ci(&self.dest, n));
        std::fs::create_dir_all(self.dest.join(&name))?;
        Ok(name)
    }

    fn make_dir(&self, root: &str, rel: &[String]) -> io::Result<()> {
        let mut p = self.dir_of(root);
        p.extend(rel);
        std::fs::create_dir_all(p)
    }

    fn open_part(&self, root: &str, rel: &[String]) -> io::Result<Box<dyn PartWriter>> {
        let Some((name, parents)) = rel.split_last() else {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path"));
        };
        let mut dir = self.dir_of(root);
        dir.extend(parents);
        std::fs::create_dir_all(&dir)?;
        let part_path = dir.join(format!("{name}{PART_SUFFIX}"));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&part_path)?;
        let len = file.metadata()?.len();
        Ok(Box::new(FsPart {
            dir,
            name: name.clone(),
            part_path,
            file,
            len,
        }))
    }

    fn free_bytes(&self) -> Option<u64> {
        self.free_fn.and_then(|f| f(&self.dest))
    }
}

struct FsPart {
    dir: PathBuf,
    name: String,
    part_path: PathBuf,
    file: File,
    len: u64,
}

impl PartWriter for FsPart {
    fn len(&self) -> u64 {
        self.len
    }

    fn hash_existing(&mut self, hasher: &mut Sha256) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    fn reset(&mut self) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.len = 0;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)?;
        self.len += data.len() as u64;
        Ok(())
    }

    fn finish(self: Box<Self>, mtime_secs: u64) -> io::Result<String> {
        let FsPart {
            dir,
            name,
            part_path,
            file,
            ..
        } = *self;
        file.sync_all()?;
        if mtime_secs > 0 {
            // Best effort: a file keeps the time it was created at the sender.
            let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs(mtime_secs));
        }
        drop(file);
        let final_name = unique_name(&name, |n| exists_ci(&dir, n));
        std::fs::rename(&part_path, dir.join(&final_name))?;
        Ok(final_name)
    }

    fn abort(self: Box<Self>) {
        let FsPart {
            part_path, file, ..
        } = *self;
        drop(file);
        let _ = std::fs::remove_file(part_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tc-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn rel(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn part_is_renamed_on_finish_and_conflicts_get_a_counter() {
        let d = tmp("conflict");
        let store = FsStore::new(&d);
        let root = store.begin_batch("").unwrap();
        for expect in ["a.txt", "a (1).txt", "a (2).txt"] {
            let mut w = store.open_part(&root, &rel(&["a.txt"])).unwrap();
            w.write(b"hello").unwrap();
            assert_eq!(w.finish(0).unwrap(), expect);
        }
        assert!(!d.join("a.txt.transclip-part").exists());
        assert_eq!(std::fs::read(d.join("a (2).txt")).unwrap(), b"hello");
    }

    #[test]
    fn existing_part_is_resumed_and_hashed() {
        let d = tmp("resume");
        let store = FsStore::new(&d);
        let root = store.begin_batch("").unwrap();
        let mut w = store.open_part(&root, &rel(&["f.bin"])).unwrap();
        w.write(b"abc").unwrap();
        drop(w); // interrupted
        let mut w = store.open_part(&root, &rel(&["f.bin"])).unwrap();
        assert_eq!(w.len(), 3);
        let mut h = Sha256::new();
        w.hash_existing(&mut h).unwrap();
        w.write(b"def").unwrap();
        h.update(b"def");
        assert_eq!(h.finalize(), Sha256::digest(b"abcdef"));
        assert_eq!(w.finish(0).unwrap(), "f.bin");
        assert_eq!(std::fs::read(d.join("f.bin")).unwrap(), b"abcdef");
    }

    #[test]
    fn abort_removes_the_part_and_folders_are_renamed_on_conflict() {
        let d = tmp("abort");
        let store = FsStore::new(&d);
        assert_eq!(store.begin_batch("Foto").unwrap(), "Foto");
        assert_eq!(store.begin_batch("Foto").unwrap(), "Foto (1)");
        let w = store.open_part("Foto", &rel(&["a", "b", "x.bin"])).unwrap();
        w.abort();
        assert!(!d.join("Foto/a/b/x.bin.transclip-part").exists());
        store.make_dir("Foto", &rel(&["kosong"])).unwrap();
        assert!(d.join("Foto/kosong").is_dir());
    }
}
