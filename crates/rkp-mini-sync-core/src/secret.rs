//! Secret files (age passphrase, Gist token, WebDAV password), design §6.4/§11.7,
//! and WebUI inbox files (§19.2).
//!
//! - Must be a regular file (symlinks refused) owned by the current user with
//!   no group/other permission bits (0600 or 0400); otherwise automation stops.
//! - Only a single trailing `\n` or `\r\n` is removed; spaces are significant.
//! - Contents are never logged; error messages mention only the path.

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use age::secrecy::SecretString;
use anyhow::Result;
use zeroize::Zeroizing;

use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::fsutil;

/// Upper bound for any secret file.
pub const MAX_SECRET_FILE: usize = 4096;
/// Upper bound for an inbox file (hex doubles the size).
pub const MAX_INBOX_FILE: usize = 2 * MAX_SECRET_FILE + 2;

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

/// Read a private file: no symlink, regular, owned by euid, no group/other bits.
fn read_private(path: &Path, what: &str, max: usize) -> Result<Zeroizing<Vec<u8>>> {
    let shown = path.display();
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            let code = match e.kind() {
                std::io::ErrorKind::NotFound => "secret_missing",
                _ if e.raw_os_error() == Some(libc::ELOOP) => "secret_permissions",
                _ => "secret_unreadable",
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
    f.take(max as u64 + 1).read_to_end(&mut buf).map_err(|e| {
        err(
            Stage::Config,
            "secret_unreadable",
            format!("{what} file {shown}: {e}"),
        )
    })?;
    if buf.len() > max {
        return Err(err(
            Stage::Config,
            "secret_invalid",
            format!("{what} file {shown} is larger than {max} bytes"),
        ));
    }
    Ok(buf)
}

/// Load a secret file after checking type, owner and permissions.
pub fn read_secret_file(path: &Path, what: &str) -> Result<SecretString> {
    let buf = read_private(path, what, MAX_SECRET_FILE)?;
    secret_from_bytes(&buf, what)
}

/// Consume a WebUI inbox file (§19.2): same checks as a secret file, optional
/// lowercase/uppercase hex decoding, and the file is deleted whether or not
/// reading succeeds.
pub fn take_inbox_file(path: &Path, hex_encoded: bool, what: &str) -> Result<SecretString> {
    let result = read_private(path, what, MAX_INBOX_FILE).and_then(|raw| {
        if !hex_encoded {
            return secret_from_bytes(&raw, what);
        }
        let body = strip_line_ending(&raw);
        let decoded = Zeroizing::new(hex::decode(body).map_err(|_| {
            err(
                Stage::Config,
                "secret_invalid",
                format!("{what} inbox file is not valid hex"),
            )
        })?);
        if decoded.len() > MAX_SECRET_FILE {
            return Err(err(
                Stage::Config,
                "secret_invalid",
                format!("{what} is larger than {MAX_SECRET_FILE} bytes"),
            ));
        }
        // Hex input carries the exact secret: no line-ending stripping inside.
        if decoded.is_empty() {
            return Err(err(
                Stage::Config,
                "secret_empty",
                format!("{what} is empty"),
            ));
        }
        let s = std::str::from_utf8(&decoded).map_err(|_| {
            err(
                Stage::Config,
                "secret_invalid",
                format!("{what} is not UTF-8"),
            )
        })?;
        if s.contains(['\n', '\r']) {
            return Err(err(
                Stage::Config,
                "secret_invalid",
                format!("{what} must be a single line"),
            ));
        }
        Ok(SecretString::from(s.to_string()))
    });
    // Never leave plaintext behind; a symlink is removed, not followed.
    let _ = fs::remove_file(path);
    result
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
    use age::secrecy::ExposeSecret;
    use rkp_mini_core::error::to_sync_error;
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

    #[test]
    fn inbox_is_consumed_and_decoded() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("age.hex");
        // " pw ü" as hex, with the trailing newline `echo` adds.
        let hex = format!("{}\n", hex::encode(" pw ü".as_bytes()));
        write_secret_file(&p, hex.as_bytes(), "x").unwrap();
        let s = take_inbox_file(&p, true, "x").unwrap();
        assert_eq!(s.expose_secret(), " pw ü");
        assert!(!p.exists(), "inbox file must be deleted");

        // Invalid content and loose permissions still delete the file.
        write_secret_file(&p, b"zz\n", "x").unwrap();
        assert_eq!(
            to_sync_error(&take_inbox_file(&p, true, "x").unwrap_err()).code,
            "secret_invalid"
        );
        assert!(!p.exists());
        fs::write(&p, hex::encode("a\nb")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(take_inbox_file(&p, true, "x").is_err(), "multi-line secret");
        fs::write(&p, hex::encode("pw")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            to_sync_error(&take_inbox_file(&p, true, "x").unwrap_err()).code,
            "secret_permissions"
        );
        assert!(!p.exists());
    }

    #[test]
    fn symlinked_secret_refused() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        write_secret_file(&real, b"pw", "x").unwrap();
        let link = dir.path().join("age.pass");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            to_sync_error(&read_secret_file(&link, "x").unwrap_err()).code,
            "secret_permissions"
        );
        assert!(take_inbox_file(&link, false, "x").is_err());
        assert!(real.exists(), "symlink target must not be deleted");
        assert!(!link.exists());
    }
}
