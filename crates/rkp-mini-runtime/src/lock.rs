//! The exclusive data lock shared by every command that writes sync/deploy
//! state, staging or a target file.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::fsutil;

/// Exclusive, non-blocking lock held for the lifetime of the guard.
pub struct Lock {
    _file: std::fs::File,
}

pub fn lock(path: &Path) -> Result<Lock> {
    if let Some(dir) = path.parent() {
        fsutil::ensure_private_dir(dir).map_err(|e| {
            err(
                Stage::State,
                "lock",
                format!("mkdir {}: {e}", dir.display()),
            )
        })?;
    }
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| {
            err(
                Stage::State,
                "lock",
                format!("open {}: {e}", path.display()),
            )
        })?;
    // SAFETY: flock on a valid, owned descriptor.
    let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err(err(
            Stage::State,
            "locked",
            "another rkp-mini sync/deploy run holds the lock; try again later",
        ));
    }
    Ok(Lock { _file: f })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_mini_core::error::to_sync_error;

    #[test]
    fn lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sync").join("sync.lock");
        let a = lock(&p).unwrap();
        let e = lock(&p).err().unwrap();
        assert_eq!(to_sync_error(&e).code, "locked");
        drop(a);
        assert!(lock(&p).is_ok());
    }
}
