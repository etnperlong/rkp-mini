//! Clock, UTC date formatting and module-loop detection.

use crate::config::Config;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` (UTC) for human output.
pub fn date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant), proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Whether the module's `sync-loop.sh` (pid file) is alive.
pub fn loop_running(cfg: &Config) -> bool {
    let Ok(text) = std::fs::read_to_string(cfg.loop_pid_path()) else {
        return false;
    };
    match text.trim().parse::<i32>() {
        // SAFETY: signal 0 only checks for existence/permission.
        Ok(pid) if pid > 1 => unsafe { libc::kill(pid, 0) == 0 },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formatting() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_762_750_000), "2025-11-10");
        assert_eq!(date(951_782_400), "2000-02-29");
    }
}
