//! Shared command plumbing: context, output, locks, secret loading.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rkp_sync_core::error::{err, Stage, SyncError};
use rkp_sync_core::locator::UrlPolicy;
use rkp_sync_core::secrecy::SecretString;
use rkp_sync_core::secret;
use rkp_sync_core::state::{self, SyncState};
use serde_json::Value;

use crate::cli::Global;
use crate::config::{self, Config, PasswordMode, Role};
use crate::http::Http;
use crate::remote::Credentials;
use crate::tty;

/// Command result: JSON `data`, human text, and an optional failure that
/// still carries data (e.g. a deploy where one target failed).
pub struct Output {
    pub data: Value,
    pub text: String,
    pub failure: Option<SyncError>,
}

impl Output {
    pub fn ok(data: Value, text: impl Into<String>) -> Output {
        Output {
            data,
            text: text.into(),
            failure: None,
        }
    }
}

pub struct Ctx {
    pub global: Global,
}

impl Ctx {
    pub fn url_policy(&self) -> UrlPolicy {
        UrlPolicy {
            allow_insecure_http: self.global.allow_insecure_http,
            allow_private: self.global.allow_private,
        }
    }

    pub fn config_path(&self, role: Option<Role>) -> Result<PathBuf> {
        config::resolve_path(self.global.config.as_deref(), role.or(self.global.role))
    }

    pub fn load(&self, role: Option<Role>) -> Result<Config> {
        let path = self.config_path(role)?;
        config::load(&path, role.or(self.global.role), self.url_policy())
    }

    /// Interactive prompts are allowed (TTY, not JSON, not automation).
    pub fn interactive(&self, auto: bool) -> bool {
        !auto && !self.global.json && tty::stdin_is_tty()
    }

    pub fn http(&self, cfg: &Config) -> Result<Http> {
        Http::new(
            cfg.sync.request_timeout_seconds,
            &cfg.sync.proxy,
            self.url_policy(),
        )
    }
}

/// Exclusive, non-blocking lock held for the lifetime of the guard.
pub struct Lock {
    _file: std::fs::File,
}

pub fn lock(path: &Path) -> Result<Lock> {
    if let Some(dir) = path.parent() {
        rkp_sync_core::fsutil::ensure_private_dir(dir).map_err(|e| {
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
            "another rkp-sync run holds the lock; try again later",
        ));
    }
    Ok(Lock { _file: f })
}

/// age passphrase per `password_mode`. Automation never prompts.
pub fn passphrase(ctx: &Ctx, cfg: &Config, auto: bool) -> Result<SecretString> {
    match cfg.sync.password_mode {
        PasswordMode::SecretFile => {
            let p = cfg.password_file();
            secret::read_secret_file(&p, "age passphrase").map_err(|e| {
                let se = rkp_sync_core::error::to_sync_error(&e);
                err(
                    se.stage,
                    &se.code,
                    format!("{}; set it with `rkp-sync secret set age`", se.message),
                )
            })
        }
        PasswordMode::Prompt if auto => Err(err(
            Stage::Config,
            "prompt_in_auto",
            "password_mode = \"prompt\" cannot run unattended; use secret-file",
        )),
        PasswordMode::Prompt => {
            if ctx.global.json && tty::stdin_is_tty() {
                return Err(err(
                    Stage::Config,
                    "prompt_in_json",
                    "password_mode = \"prompt\" with --json needs the passphrase on stdin",
                ));
            }
            tty::read_secret("age passphrase", false)
        }
    }
}

/// Provider credentials from secret files. `sender` decides whether a Gist token is required.
pub fn credentials(cfg: &Config) -> Result<Credentials> {
    let mut c = Credentials::default();
    match cfg.sync.provider.as_str() {
        "gist" if cfg.role == Role::Sender => {
            c.gist_token = Some(
                secret::read_secret_file(&cfg.gist_token_file(), "Gist token").map_err(|e| {
                    let se = rkp_sync_core::error::to_sync_error(&e);
                    err(
                        se.stage,
                        &se.code,
                        format!(
                            "{}; set it with `rkp-sync secret set gist-token`",
                            se.message
                        ),
                    )
                })?,
            );
        }
        "webdav" if !cfg.sync.webdav_user.is_empty() => {
            c.webdav_password = Some(
                secret::read_secret_file(&cfg.webdav_password_file(), "WebDAV password").map_err(
                    |e| {
                        let se = rkp_sync_core::error::to_sync_error(&e);
                        err(
                            se.stage,
                            &se.code,
                            format!(
                                "{}; set it with `rkp-sync secret set webdav-password`",
                                se.message
                            ),
                        )
                    },
                )?,
            );
        }
        _ => {}
    }
    Ok(c)
}

pub fn load_sync_state(cfg: &Config) -> Result<Option<SyncState>> {
    state::load::<SyncState>(&cfg.sync_state_path())
}

/// Record a failure in sync-state without touching verified fields.
pub fn record_error(cfg: &Config, e: &anyhow::Error, now: i64) {
    let Ok(Some(mut s)) = load_sync_state(cfg) else {
        return;
    };
    let se = rkp_sync_core::error::to_sync_error(e);
    s.last_checked_at = Some(now);
    s.set_error(&se.code, &se.message, now);
    let _ = state::save(&cfg.sync_state_path(), &s);
}

/// Secret-file presence/permission summary (never contents).
pub fn secret_status(path: &Path) -> Value {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(m) => serde_json::json!({
            "path": path.display().to_string(),
            "present": true,
            "mode": format!("{:04o}", m.mode() & 0o7777),
            "ok": m.is_file() && m.mode() & 0o077 == 0,
        }),
        Err(_) => serde_json::json!({
            "path": path.display().to_string(),
            "present": false,
            "ok": false,
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_formatting() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_762_750_000), "2025-11-10");
        assert_eq!(date(951_782_400), "2000-02-29");
    }

    #[test]
    fn lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sync").join("sync.lock");
        let a = lock(&p).unwrap();
        let e = lock(&p).err().unwrap();
        assert_eq!(rkp_sync_core::error::to_sync_error(&e).code, "locked");
        drop(a);
        assert!(lock(&p).is_ok());
    }
}
