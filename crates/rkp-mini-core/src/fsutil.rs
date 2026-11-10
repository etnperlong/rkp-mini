//! Atomic, permission-correct file writes (design §11.6).
//!
//! `atomic_write`: same-directory temp file created with `O_CREAT|O_EXCL`
//! and mode 0600, write + fsync, fchmod/fchown, rename over the target,
//! fsync the directory. On failure the target keeps its previous content and
//! the temp file is unlinked.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{fchown, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Create `path` (and parents) and force mode 0700 on the leaf directory.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn temp_path(path: &Path) -> io::Result<PathBuf> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    Ok(dir.join(format!(
        ".{}.tmp.{}",
        name.to_string_lossy(),
        std::process::id()
    )))
}

/// Atomically replace `path` with `bytes`, final mode `mode`, optional owner.
pub fn atomic_write(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    owner: Option<(u32, u32)>,
) -> io::Result<()> {
    let tmp = temp_path(path)?;
    // A stale temp from a crashed process with the same pid is ours to remove.
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        if let Some((uid, gid)) = owner {
            fchown(&f, Some(uid), Some(gid))?;
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        sync_dir(path.parent().expect("checked in temp_path"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// fsync a directory so a completed rename is durable.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Read a file of at most `max` bytes; larger files are rejected unread.
pub fn read_limited(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let f = File::open(path)?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file larger than {max} bytes"),
        ));
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn writes_with_mode_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.xml");
        atomic_write(&p, b"one", 0o600, None).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        atomic_write(&p, b"two", 0o640, None).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::metadata(&p).unwrap().mode() & 0o777, 0o640);
        // No temp files left behind.
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn failure_keeps_old_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.xml");
        atomic_write(&p, b"old", 0o600, None).unwrap();
        // Make the directory read-only so the temp file cannot be created.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let r = atomic_write(&p, b"new", 0o600, None);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        if r.is_ok() {
            // Running as root: permissions are not enforced; nothing to assert.
            return;
        }
        assert_eq!(fs::read(&p).unwrap(), b"old");
    }

    #[test]
    fn read_limited_rejects_large() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        fs::write(&p, vec![0u8; 11]).unwrap();
        assert!(read_limited(&p, 10).is_err());
        assert_eq!(read_limited(&p, 11).unwrap().len(), 11);
    }
}
