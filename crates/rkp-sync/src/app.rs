//! Sync-process plumbing that must not leak into the local-only layer: the
//! HTTP client, the age passphrase, provider credentials and sync-state
//! bookkeeping.

use std::path::Path;

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::state::{self, SyncState};
use rkp_mini_runtime::config::{Config, PasswordMode, Role};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_sync_core::secrecy::SecretString;
use rkp_mini_sync_core::secret;
use serde_json::Value;

use crate::http::Http;
use crate::remote::Credentials;
use crate::tty;

pub fn http(ctx: &Ctx, cfg: &Config) -> Result<Http> {
    Http::new(
        cfg.sync.request_timeout_seconds,
        &cfg.sync.proxy,
        ctx.url_policy(),
    )
}

/// age passphrase per `password_mode`. Automation never prompts.
pub fn passphrase(ctx: &Ctx, cfg: &Config, auto: bool) -> Result<SecretString> {
    match cfg.sync.password_mode {
        PasswordMode::SecretFile => {
            let p = cfg.password_file();
            secret::read_secret_file(&p, "age passphrase").map_err(|e| {
                let se = rkp_mini_core::error::to_sync_error(&e);
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
                    let se = rkp_mini_core::error::to_sync_error(&e);
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
                        let se = rkp_mini_core::error::to_sync_error(&e);
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
    let se = rkp_mini_core::error::to_sync_error(e);
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
