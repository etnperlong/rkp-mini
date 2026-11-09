//! Small shared helpers: system properties, atomic writes, hex, time.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use time::{format_description::FormatItem, macros::format_description, OffsetDateTime};

/// Read an Android system property via the `getprop` binary. Empty if absent.
pub fn getprop(key: &str) -> String {
    std::process::Command::new("getprop")
        .arg(key)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Read a file, trimmed, if present and valid UTF-8.
pub fn read_file_trim(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Atomic write: temp file in same dir, `sync_all`, `rename`. Sets `mode`.
pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // Ensure mode even if file pre-existed with different perms.
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
    fs::rename(&tmp, path)?;
    let _ = dir; // dir fsync omitted: good enough for module data
    Ok(())
}

/// Create a directory (and parents) with mode 0700 on the leaf.
pub fn ensure_dir_0700(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

const DATE_FMT: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");
const DATETIME_FMT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
const RFC3339_FMT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

fn odt(unix: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(unix).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// `YYYY-MM-DD` (UTC).
pub fn format_date(unix: i64) -> String {
    odt(unix).format(DATE_FMT).unwrap_or_default()
}

/// `YYYY-MM-DD HH:MM:SS UTC`.
pub fn format_datetime(unix: i64) -> String {
    odt(unix).format(DATETIME_FMT).unwrap_or_default()
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_rfc3339(unix: i64) -> String {
    odt(unix).format(RFC3339_FMT).unwrap_or_default()
}

/// Whole days from `now` to `not_after`, floored; negative = expired.
pub fn days_left(not_after: i64, now: i64) -> i64 {
    (not_after - now).div_euclid(86_400)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Escape text for XML attribute/element content.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Collapse whitespace, strip newlines, truncate to `max` chars (single line).
pub fn single_line(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}
