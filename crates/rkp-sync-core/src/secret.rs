//! Secret files (age passphrase, Gist token, WebDAV password), design §6.4/§11.7.
//!
//! - Must be a regular file owned by the current user with no group/other
//!   permission bits (0600 or 0400); otherwise automation stops.
//! - Only a single trailing `\n` or `\r\n` is removed; spaces are significant.
//! - Contents are never logged; error messages mention only the path.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use age::secrecy::SecretString;
use anyhow::Result;
use zeroize::Zeroizing;

use crate::error::{err, Stage};
use crate::fsutil;

/// Upper bound for any secret file.
pub const MAX_SECRET_FILE: usize = 4096;

/// Remove exactly one trailing `\n` or `\r\n`.
pub fn strip_line_ending(bytes: &[u8]) -> &[u8] {
    if let Some(b) = bytes.strip_suffix(b"\r\n") {
        b
    } else if let Some(b) = bytes.strip_suffix(b"\n") {
        b
    } else {
        bytes
    }
}

/// Convert raw secret bytes into a [`SecretString`] (UTF-8, non-empty).
pub fn secret_from_bytes(bytes: &[u8], what: &str) -> Result<SecretString> {
    let body = strip_line_ending(bytes);
    if body.is_empty() {
        return Err(err(
            Stage::Config,
            "secret_empty",
            format!("{what} is empty"),
        ));
    }
    let s = std::str::from_utf8(body).map_err(|_| {
        err(
            Stage::Config,
            "secret_invalid",
            format!("{what} is not UTF-8"),
        )
    })?;
    Ok(SecretString::from(s.to_string()))
}

/// Load a secret file after checking type, owner and permissions.
pub fn read_secret_file(path: &Path, what: &str) -> Result<SecretString> {
    let shown = path.display();
    let f = File::open(path).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            "secret_missing"
        } else {
            "secret_unreadable"
        };
        err(Stage::Config, code, format!("{what} file {shown}: {e}"))
    })?;
    let meta = f.metadata().map_err(|e| {
        err(
            Stage::Config,
            "secret_unreadable",
            format!("{what} file {shown}: {e}"),
        )
    })?;
    if !meta.is_file() {
        return Err(err(
            Stage::Config,
            "secret_permissions",
            format!("{what} file {shown} is not a regular file"),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(err(
            Stage::Config,
            "secret_permissions",
            format!(
                "{what} file {shown} has mode {:o}; require 0600",
                meta.mode() & 0o777
            ),
        ));
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let euid = unsafe { libc::geteuid() };
    if meta.uid() != euid {
        return Err(err(
            Stage::Config,
            "secret_permissions",
            format!("{what} file {shown} is not owned by uid {euid}"),
        ));
    }
    let mut buf = Zeroizing::new(Vec::new());
    f.take(MAX_SECRET_FILE as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| {
            err(
                Stage::Config,
                "secret_unreadable",
                format!("{what} file {shown}: {e}"),
            )
        })?;
    if buf.len() > MAX_SECRET_FILE {
        return Err(err(
            Stage::Config,
            "secret_invalid",
            format!("{what} file {shown} is larger than {MAX_SECRET_FILE} bytes"),
        ));
    }
    secret_from_bytes(&buf, what)
}

/// Store a secret as a 0600 file in a 0700 directory (raw bytes, no newline).
pub fn write_secret_file(path: &Path, secret: &[u8], what: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fsutil::ensure_private_dir(dir).map_err(|e| {
            err(
                Stage::State,
                "secret_write",
                format!("mkdir {}: {e}", dir.display()),
            )
        })?;
    }
    fsutil::atomic_write(path, secret, 0o600, None).map_err(|e| {
        err(
            Stage::State,
            "secret_write",
            format!("write {what} file {}: {e}", path.display()),
        )
    })
}

/// Remove a secret file; missing is fine.
pub fn remove_secret_file(path: &Path) -> std::io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_sync_error;
    use age::secrecy::ExposeSecret;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn strips_one_line_ending_only() {
        assert_eq!(strip_line_ending(b"pw\n"), b"pw");
        assert_eq!(strip_line_ending(b"pw\r\n"), b"pw");
        assert_eq!(strip_line_ending(b"pw\n\n"), b"pw\n");
        assert_eq!(strip_line_ending(b" pw "), b" pw ");
    }

    #[test]
    fn reads_0600_and_preserves_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("age.pass");
        write_secret_file(&p, b"  two spaces  \n", "age passphrase").unwrap();
        let s = read_secret_file(&p, "age passphrase").unwrap();
        assert_eq!(s.expose_secret(), "  two spaces  ");
    }

    #[test]
    fn rejects_loose_permissions_missing_and_empty() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("age.pass");
        assert_eq!(
            to_sync_error(&read_secret_file(&p, "x").unwrap_err()).code,
            "secret_missing"
        );
        fs::write(&p, b"pw").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        let e = to_sync_error(&read_secret_file(&p, "x").unwrap_err());
        assert_eq!(e.code, "secret_permissions");
        assert!(!e.message.contains("pw\""), "message must not echo content");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&p, b"\n").unwrap();
        assert_eq!(
            to_sync_error(&read_secret_file(&p, "x").unwrap_err()).code,
            "secret_empty"
        );
    }
}
