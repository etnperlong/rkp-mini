//! Role-independent commands: init, risk/enable/disable, forget, secrets,
//! config show/set, status.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::state::{self, DeployState};
use rkp_mini_sync_core::secrecy::{ExposeSecret, SecretString};
use rkp_mini_sync_core::secret;
use serde_json::json;

use crate::app;
use crate::cli::SecretKind;
use crate::tty;
use rkp_mini_runtime::config::{self, Config, PasswordMode, Role, Value, RISK_VERSION};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::Output;
use rkp_mini_runtime::targets;
use rkp_mini_runtime::{lock, util};

/// One risk statement item (§13.2), English and Chinese.
pub struct RiskItem {
    pub id: &'static str,
    pub en: &'static str,
    pub zh: &'static str,
}

/// Risk statement. Bump `config::RISK_VERSION` whenever an item changes, so
/// every UI/CLI must ask again (`enable --accept-risk` checks the version).
pub const RISK_ITEMS: &[RiskItem] = &[
    RiskItem {
        id: "cloning",
        en: "The keybox is the issuing device's RKP identity. Using it on another device is keybox cloning/spoofing, not legitimate RKP provisioning.",
        zh: "该 keybox 是签发设备的 RKP 身份；在新设备上使用属于 keybox 克隆/冒充，不是合法的 RKP provisioning。",
    },
    RiskItem {
        id: "integrity",
        en: "Play Integrity, Key Attestation and banking/payment apps may reject it, and Google may revoke it.",
        zh: "Play Integrity、Key Attestation、银行/支付应用可能拒绝；Google 可能吊销。",
    },
    RiskItem {
        id: "key_copy",
        en: "The private key is copied to another device; if either device leaks, both are compromised.",
        zh: "私钥会被复制到新设备；任一台设备泄露，另一台一起失守。",
    },
    RiskItem {
        id: "passphrase_on_disk",
        en: "For unattended runs the age passphrase is stored in a 0600 file. Losing the device or a root compromise leaks it; then change the passphrase AND re-issue the keybox.",
        zh: "为支持自动运行，age 密码保存在 0600 文件；设备丢失或 root 被攻破即视为泄露，必须换密码并重新签发 keybox。",
    },
    RiskItem {
        id: "debug_manager",
        en: "Secrets entered in the WebUI pass through the KernelSU manager's root shell; a debug build of the manager logs shell commands to logcat.",
        zh: "在 WebUI 输入的密钥会经过 KernelSU 管理器的 root shell；debug 版管理器会把 shell 命令写入 logcat。",
    },
    RiskItem {
        id: "omk_retire",
        en: "OhMyKeymint's retire_stale_keybox_bound_keys (default true) deletes client keys bound to the previous keybox when the keybox changes.",
        zh: "OMK 的 retire_stale_keybox_bound_keys=true 会在轮换 keybox 时删除绑定旧 keybox 的 key。",
    },
    RiskItem {
        id: "no_revocation",
        en: "Disabling sync does not revoke anything; this is not legitimate cross-device provisioning and the risk is yours.",
        zh: "禁用不等于撤销；这不是合法的跨设备 provisioning，风险由使用者承担。",
    },
];

pub const ACCEPT_PHRASE: &str = "I ACCEPT";

fn risk_text() -> String {
    let mut s = format!(
        "Remote keybox sync — risk statement (v{RISK_VERSION}) / 远程 keybox 同步风险说明\n\n"
    );
    for (i, item) in RISK_ITEMS.iter().enumerate() {
        s.push_str(&format!("{:2}. {}\n    {}\n", i + 1, item.en, item.zh));
    }
    s
}

pub fn risk(ctx: &Ctx) -> Result<Output> {
    // Works before init too; the passphrase path is shown when known.
    let cfg = ctx.load(None).ok();
    let items: Vec<_> = RISK_ITEMS
        .iter()
        .map(|i| json!({ "id": i.id, "en": i.en, "zh": i.zh }))
        .collect();
    let passphrase_path = cfg
        .as_ref()
        .filter(|c| c.sync.password_mode == PasswordMode::SecretFile)
        .map(|c| c.password_file().display().to_string());
    Ok(Output::ok(
        json!({
            "version": RISK_VERSION,
            "items": items,
            "passphrase_path": passphrase_path,
            "accepted_version": cfg.as_ref().map(|c| c.sync.risk_ack_version),
        }),
        risk_text().trim_end().to_string(),
    ))
}

pub fn init(ctx: &Ctx, provider: Option<String>, stream_id: Option<String>) -> Result<Output> {
    let role = ctx
        .global
        .role
        .ok_or_else(|| err(Stage::Config, "usage", "init needs --role sender|receiver"))?;
    let path = ctx.config_path(Some(role))?;
    let root = path
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| {
            err(
                Stage::Config,
                "config_invalid",
                "config path must be absolute",
            )
        })?
        .to_path_buf();
    let existing = std::fs::read_to_string(&path).ok();
    if existing
        .as_deref()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .is_some_and(|t| t.contains_key("sync"))
    {
        let cfg = config::load(&path, Some(role), ctx.url_policy())?;
        return Ok(Output::ok(
            json!({ "config": path.display().to_string(), "created": false, "stream_id": cfg.sync.stream_id }),
            format!("already initialized: {}", path.display()),
        ));
    }
    let text = match role {
        Role::Sender => {
            let provider = provider.unwrap_or_else(|| "gist".into());
            if !matches!(provider.as_str(), "gist" | "webdav") {
                return Err(err(
                    Stage::Config,
                    "usage",
                    "--provider must be gist or webdav",
                ));
            }
            let stream = match stream_id {
                Some(s) if rkp_mini_core::valid_stream_id(&s) => s,
                Some(_) => {
                    return Err(err(
                        Stage::Config,
                        "usage",
                        "--stream-id has invalid format",
                    ))
                }
                None => config::new_stream_id(),
            };
            let mut base = existing.unwrap_or_default();
            if !base.is_empty() && !base.ends_with('\n') {
                base.push('\n');
            }
            base + &config::sender_sync_template(&stream, &provider, &root)
        }
        Role::Receiver => {
            if provider.is_some() || stream_id.is_some() {
                return Err(err(
                    Stage::Config,
                    "usage",
                    "a receiver gets provider/stream from `rkp-mini-sync import`",
                ));
            }
            config::receiver_template(&root)
        }
    };
    config::write_config(&path, text.as_bytes())?;
    let cfg = config::load(&path, Some(role), ctx.url_policy())?;
    rkp_mini_core::fsutil::ensure_private_dir(&cfg.sync_dir()).map_err(|e| {
        err(
            Stage::State,
            "mkdir",
            format!("{}: {e}", cfg.sync_dir().display()),
        )
    })?;
    let next = match role {
        Role::Sender => "next: `rkp-mini-sync secret set age`, `rkp-mini-sync secret set gist-token` (or webdav-password), `rkp-mini-sync enable`, `rkp-mini-sync push`",
        Role::Receiver => "next: `rkp-mini-sync import '<rkpsync://…>'`, `rkp-mini-deployer add-target <omk|teesim|tricky_store>`",
    };
    Ok(Output::ok(
        json!({
            "config": path.display().to_string(),
            "created": true,
            "role": role.as_str(),
            "stream_id": cfg.sync.stream_id,
        }),
        format!("wrote {} ({})\n{next}", path.display(), role.as_str()),
    ))
}

/// Show the risk statement and record acceptance. Returns false if declined.
///
/// Non-interactive acceptance must name the exact current version, so a UI or
/// script written for an older statement cannot accept a newer one.
pub fn accept_risk(ctx: &Ctx, cfg: &Config, accept_version: Option<u32>) -> Result<bool> {
    let accepted = match accept_version {
        Some(v) if v == RISK_VERSION => true,
        Some(v) => {
            return Err(err(
                Stage::Config,
                "risk_version_mismatch",
                format!(
                    "--accept-risk {v} does not match the current statement v{RISK_VERSION}; review `rkp-mini-sync risk`"
                ),
            ))
        }
        None if ctx.interactive(false) => {
            eprintln!("{}", risk_text());
            if cfg.sync.password_mode == PasswordMode::SecretFile {
                eprintln!(
                    "The age passphrase will be stored at {} (0600) for unattended runs.\n",
                    cfg.password_file().display()
                );
            }
            tty::ask(&format!("Type `{ACCEPT_PHRASE}` to accept: "))?.trim() == ACCEPT_PHRASE
        }
        None => {
            return Err(err(
                Stage::Config,
                "risk_not_acknowledged",
                format!(
                    "risk acceptance needs a TTY (type the phrase) or --accept-risk {RISK_VERSION} after reviewing `rkp-mini-sync risk`"
                ),
            ))
        }
    };
    if !accepted {
        return Ok(false);
    }
    config::edit_table(
        &cfg.path,
        "sync",
        &[
            ("risk_ack_version", Value::Int(i64::from(RISK_VERSION))),
            ("risk_ack_at", Value::Int(util::now())),
        ],
    )?;
    Ok(true)
}

pub fn enable(ctx: &Ctx, accept_version: Option<u32>) -> Result<Output> {
    let cfg = ctx.load(None)?;
    if cfg.sync.risk_ack_version < RISK_VERSION && !accept_risk(ctx, &cfg, accept_version)? {
        return Err(err(
            Stage::Config,
            "risk_not_acknowledged",
            "risk statement not accepted; sync stays disabled",
        ));
    }
    config::edit_table(&cfg.path, "sync", &[("enabled", Value::Bool(true))])?;
    eprintln!("rkp-mini-sync: enabled ({})", cfg.role.as_str());
    Ok(Output::ok(
        json!({ "enabled": true, "risk_ack_version": RISK_VERSION, "role": cfg.role.as_str() }),
        "sync enabled. Unattended runs start at next boot (or run the module's sync loop).",
    ))
}

pub fn disable(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    config::edit_table(&cfg.path, "sync", &[("enabled", Value::Bool(false))])?;
    Ok(Output::ok(
        json!({ "enabled": false }),
        "sync disabled; automatic runs stop at their next wake-up. Deployed keyboxes are kept.\n\
         Disabling does not revoke: change the passphrase and re-issue the keybox for that.",
    ))
}

fn remove_file(path: &Path, removed: &mut Vec<String>) -> Result<()> {
    if secret::remove_secret_file(path)
        .map_err(|e| err(Stage::State, "remove", format!("{}: {e}", path.display())))?
    {
        removed.push(path.display().to_string());
    }
    Ok(())
}

pub fn forget(ctx: &Ctx, yes: bool, staging: bool) -> Result<Output> {
    let cfg = ctx.load(None)?;
    if !yes
        && (!ctx.interactive(false)
            || !tty::confirm(
                "Delete local secrets and sync state? The remote copy is NOT deleted.",
            )?)
    {
        return Err(err(
            Stage::Config,
            "confirmation_required",
            "pass --yes to confirm",
        ));
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    config::edit_table(&cfg.path, "sync", &[("enabled", Value::Bool(false))])?;
    let mut removed = Vec::new();
    for p in [
        cfg.password_file(),
        cfg.gist_token_file(),
        cfg.webdav_password_file(),
        cfg.sync_state_path(),
    ] {
        remove_file(&p, &mut removed)?;
    }
    if cfg.role == Role::Receiver {
        remove_file(&cfg.deploy_state_path(), &mut removed)?;
        config::edit_table(
            &cfg.path,
            "sync",
            &[
                ("provider", Value::Str("")),
                ("stream_id", Value::Str("")),
                ("label", Value::Str("")),
                ("gist_url", Value::Str("")),
                ("gist_id", Value::Str("")),
                ("webdav_endpoint", Value::Str("")),
                ("webdav_path", Value::Str("")),
                ("webdav_user", Value::Str("")),
                ("url", Value::Str("")),
            ],
        )?;
        if staging {
            state::prune_staging(&cfg.staging_dir(), None)
                .map_err(|e| err(Stage::State, "remove", format!("staging: {e}")))?;
            removed.push(cfg.staging_dir().display().to_string());
        }
    }
    let mut text = String::from("forgot local sync data (remote untouched):");
    for r in &removed {
        text.push_str(&format!("\n  removed {r}"));
    }
    if cfg.role == Role::Receiver && !staging {
        text.push_str("\nstaged keybox kept; add --staging to delete it");
    }
    Ok(Output::ok(
        json!({ "removed": removed, "enabled": false }),
        text,
    ))
}

/// Read a secret from an inbox file (§19.2) or, without one, the TTY/stdin.
pub fn read_secret_input(
    ctx: &Ctx,
    from_file: Option<&Path>,
    hex: bool,
    what: &str,
    twice: bool,
) -> Result<SecretString> {
    if let Some(f) = from_file {
        return secret::take_inbox_file(f, hex, what);
    }
    if ctx.global.json && tty::stdin_is_tty() {
        return Err(err(
            Stage::Config,
            "usage",
            "with --json pipe the secret on stdin or use --from-file",
        ));
    }
    tty::read_secret(what, twice)
}

pub fn secret_set(
    ctx: &Ctx,
    kind: SecretKind,
    from_file: Option<PathBuf>,
    hex: bool,
) -> Result<Output> {
    // Consume the inbox even when the command is refused below.
    let cfg = match ctx.load(None) {
        Ok(c) => c,
        Err(e) => {
            if let Some(f) = &from_file {
                let _ = std::fs::remove_file(f);
            }
            return Err(e);
        }
    };
    let target = match kind {
        SecretKind::Age if cfg.sync.password_mode != PasswordMode::SecretFile => Err(err(
            Stage::Config,
            "usage",
            "password_mode is \"prompt\"; nothing is stored",
        )),
        SecretKind::Age => Ok((cfg.password_file(), "age passphrase", true)),
        SecretKind::GistToken if cfg.role != Role::Sender => Err(err(
            Stage::Config,
            "usage",
            "receivers read Gists without a token",
        )),
        SecretKind::GistToken => Ok((cfg.gist_token_file(), "Gist token", false)),
        SecretKind::WebdavPassword => {
            Ok((cfg.webdav_password_file(), "WebDAV app password", false))
        }
    };
    let (path, what, twice) = match target {
        Ok(t) => t,
        Err(e) => {
            if let Some(f) = &from_file {
                let _ = std::fs::remove_file(f);
            }
            return Err(e);
        }
    };
    let value = read_secret_input(ctx, from_file.as_deref(), hex, what, twice)?;
    let short = kind == SecretKind::Age && value.expose_secret().chars().count() < 12;
    if short {
        eprintln!(
            "warning: short passphrase; use 6+ random words or 20+ random characters, unique per stream"
        );
    }
    secret::write_secret_file(&path, value.expose_secret().as_bytes(), what)?;
    Ok(Output::ok(
        json!({ "path": path.display().to_string(), "mode": "0600", "weak": short }),
        format!("{what} saved to {} (0600)", path.display()),
    ))
}

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Uint,
    Bool,
}

/// Keys the WebUI/CLI may change with `config set` (§19.4), per role.
/// Location (receiver), enablement, risk acknowledgement, secrets and targets
/// have dedicated commands and are deliberately absent.
fn settable(role: Role, key: &str) -> Option<(&'static str, Kind)> {
    let common = match key {
        "label" | "proxy" => Some(("sync", Kind::Str)),
        "min_validity_days" | "request_timeout_seconds" => Some(("sync", Kind::Uint)),
        "password_mode" => Some(("sync", Kind::Str)),
        _ => None,
    };
    common.or(match (role, key) {
        (
            Role::Sender,
            "provider" | "gist_id" | "gist_filename" | "webdav_endpoint" | "webdav_path"
            | "webdav_user",
        ) => Some(("sync", Kind::Str)),
        (Role::Sender, "auto_issue_days") => Some(("", Kind::Uint)),
        (Role::Receiver, "identity_change_policy") => Some(("deploy", Kind::Str)),
        (Role::Receiver, "rotation_lead_days") => Some(("deploy", Kind::Uint)),
        (Role::Receiver, "auto_deploy") => Some(("deploy", Kind::Bool)),
        _ => None,
    })
}

pub fn config_set(ctx: &Ctx, key: &str, value: &str) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let (table, kind) = settable(cfg.role, key).ok_or_else(|| {
        err(
            Stage::Config,
            "key_not_settable",
            format!(
                "`{key}` cannot be changed with `config set` for a {}",
                cfg.role.as_str()
            ),
        )
    })?;
    let int;
    let v = match kind {
        Kind::Str => Value::Str(value),
        Kind::Uint => {
            int = value.parse::<u32>().map_err(|_| {
                err(
                    Stage::Config,
                    "config_invalid",
                    format!("`{key}` needs a non-negative integer"),
                )
            })?;
            Value::Int(i64::from(int))
        }
        Kind::Bool => match value {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => {
                return Err(err(
                    Stage::Config,
                    "config_invalid",
                    format!("`{key}` needs true or false"),
                ))
            }
        },
    };
    // Field-level checks that `load` does not perform for incomplete locations.
    if !value.is_empty() {
        use rkp_mini_core::locator;
        match key {
            "webdav_endpoint" => {
                locator::check_url(value, ctx.url_policy(), "webdav_endpoint")?;
            }
            "webdav_path" => {
                locator::normalize_path(value)?;
            }
            "gist_id" => locator::check_gist_id(value)?,
            _ => {}
        }
    }
    let backup = std::fs::read(&cfg.path).map_err(|e| {
        err(
            Stage::Config,
            "config_invalid",
            format!("read {}: {e}", cfg.path.display()),
        )
    })?;
    let mut values = vec![(key, v)];
    // A hand-set gist id invalidates the owner-specific raw URL from an earlier push.
    if key == "gist_id" {
        values.push(("gist_url", Value::Str("")));
    }
    config::edit_table(&cfg.path, table, &values)?;
    if let Err(e) = config::load(&cfg.path, Some(cfg.role), ctx.url_policy()) {
        config::write_config(&cfg.path, &backup)?;
        return Err(e);
    }
    Ok(Output::ok(
        json!({ "key": key, "value": value }),
        format!("{key} = {value:?}"),
    ))
}

pub fn config_show(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let s = &cfg.sync;
    let mut data = json!({
        "role": cfg.role.as_str(),
        "config": cfg.path.display().to_string(),
        "enabled": s.enabled,
        "risk_ack_version": s.risk_ack_version,
        "risk_version": RISK_VERSION,
        "provider": s.provider,
        "stream_id": s.stream_id,
        "label": s.label,
        "min_validity_days": cfg.min_validity_days(),
        "request_timeout_seconds": s.request_timeout_seconds,
        "proxy": s.proxy,
        "gist_id": s.gist_id,
        "gist_filename": s.gist_filename,
        "gist_url": s.gist_url,
        "webdav_endpoint": s.webdav_endpoint,
        "webdav_path": s.webdav_path,
        "webdav_user": s.webdav_user,
        "url": s.url,
        "password_mode": match s.password_mode { PasswordMode::SecretFile => "secret-file", PasswordMode::Prompt => "prompt" },
        "secret_files": {
            "age": cfg.password_file().display().to_string(),
            "gist_token": cfg.gist_token_file().display().to_string(),
            "webdav_password": cfg.webdav_password_file().display().to_string(),
        },
        "inbox": cfg.inbox_dir().display().to_string(),
    });
    match cfg.role {
        Role::Sender => {
            data["output_dir"] = json!(cfg.output_dir.display().to_string());
            data["auto_issue_days"] = json!(cfg.auto_issue_days);
        }
        Role::Receiver => {
            data["identity_change_policy"] = json!(cfg.deploy.identity_change_policy.as_str());
            data["rotation_lead_days"] = json!(cfg.deploy.rotation_lead_days);
            data["auto_deploy"] = json!(cfg.deploy.auto_deploy);
            data["targets"] = json!(cfg
                .deploy
                .targets
                .iter()
                .map(|t| json!({ "id": t.id, "path": t.path, "owner": t.owner, "mode": t.mode, "reload": t.reload }))
                .collect::<Vec<_>>());
        }
    }
    let text = serde_json::to_string_pretty(&data).expect("json");
    Ok(Output::ok(data, text))
}

/// Exit 0 when unattended runs may proceed; reason on stderr otherwise.
///
/// `require_verified` is the first-round gate for the combined receiver
/// flow: it additionally needs a completed first import (a verified sync
/// state). It never contacts the remote. Plain `check-auto` is unchanged.
pub fn check_auto(ctx: &Ctx, require_verified: bool) -> Result<Output> {
    let cfg = ctx.load(None)?;
    if require_verified && cfg.role != Role::Receiver {
        return Err(err(
            Stage::Config,
            "wrong_role",
            "--require-verified only applies to a receiver configuration",
        ));
    }
    cfg.require_enabled()?;
    cfg.require_stream()?;
    if cfg.sync.password_mode != PasswordMode::SecretFile {
        return Err(err(
            Stage::Config,
            "prompt_in_auto",
            "unattended runs need password_mode = \"secret-file\"",
        ));
    }
    if require_verified {
        // `load_sync_state` reports a corrupt state file with its own error.
        let verified = app::load_sync_state(&cfg)?.is_some_and(|s| s.has_verified());
        if !verified {
            return Err(err(
                Stage::State,
                "first_import_needs_confirmation",
                "unattended runs start after the first manual `rkp-mini-sync fetch`",
            ));
        }
    }
    Ok(Output::ok(
        json!({ "auto": true, "role": cfg.role.as_str() }),
        format!("auto: yes ({})", cfg.role.as_str()),
    ))
}

pub fn status(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let now = util::now();
    let sync_state = app::load_sync_state(&cfg)?;
    let loc = cfg.location().ok().map(|l| l.display());
    let mut secrets = serde_json::Map::new();
    if cfg.sync.password_mode == PasswordMode::SecretFile {
        secrets.insert("age".into(), app::secret_status(&cfg.password_file()));
    }
    if cfg.role == Role::Sender && cfg.sync.provider == "gist" {
        secrets.insert(
            "gist_token".into(),
            app::secret_status(&cfg.gist_token_file()),
        );
    }
    if cfg.sync.provider == "webdav" && !cfg.sync.webdav_user.is_empty() {
        secrets.insert(
            "webdav_password".into(),
            app::secret_status(&cfg.webdav_password_file()),
        );
    }
    let mut data = json!({
        "role": cfg.role.as_str(),
        "config": cfg.path.display().to_string(),
        "enabled": cfg.sync.enabled,
        "risk_ack_version": cfg.sync.risk_ack_version,
        "risk_ack_current": cfg.sync.risk_ack_version >= RISK_VERSION,
        "risk_version": RISK_VERSION,
        "loop_running": util::loop_running(&cfg),
        "provider": cfg.sync.provider,
        "location": loc,
        "stream_id": cfg.sync.stream_id,
        "password_mode": match cfg.sync.password_mode { PasswordMode::SecretFile => "secret-file", PasswordMode::Prompt => "prompt" },
        "secrets": secrets,
        "sync_state": sync_state,
    });
    let mut text = format!(
        "role: {}\nenabled: {} (risk accepted: {})\nprovider: {}\nlocation: {}\nstream: {}\n",
        cfg.role.as_str(),
        cfg.sync.enabled,
        cfg.sync.risk_ack_version >= RISK_VERSION,
        if cfg.sync.provider.is_empty() {
            "-"
        } else {
            &cfg.sync.provider
        },
        loc.as_deref().unwrap_or("-"),
        if cfg.sync.stream_id.is_empty() {
            "-"
        } else {
            &cfg.sync.stream_id
        },
    );
    for (k, v) in data["secrets"].as_object().cloned().unwrap_or_default() {
        text.push_str(&format!(
            "secret {k}: {}\n",
            if v["ok"].as_bool() == Some(true) {
                "ok (0600)".to_string()
            } else if v["present"].as_bool() == Some(true) {
                format!("BAD MODE {}", v["mode"].as_str().unwrap_or("?"))
            } else {
                "missing".to_string()
            }
        ));
    }
    match &sync_state {
        Some(s) => {
            text.push_str(&format!(
                "{}: seq {} keybox {} device {} expires {}\n",
                if cfg.role == Role::Sender {
                    "last push"
                } else {
                    "verified"
                },
                s.verified_seq,
                s.keybox_sha256
                    .as_deref()
                    .map_or("-", |h| &h[..12.min(h.len())]),
                s.device_id.as_deref().unwrap_or("-"),
                s.keybox_not_after.map_or("-".into(), util::date),
            ));
            if let Some(e) = &s.last_error {
                text.push_str(&format!("last error: {} ({})\n", e.code, util::date(e.at)));
            }
        }
        None => text.push_str("no sync state yet\n"),
    }
    match cfg.role {
        Role::Sender => {
            let kb = sender_keybox_status(&cfg, now);
            text.push_str(&format!(
                "local keybox: {}\nauto_issue_days: {}\n",
                kb["summary"].as_str().unwrap_or("-"),
                cfg.auto_issue_days
            ));
            data["local_keybox"] = kb;
            data["auto_issue_days"] = json!(cfg.auto_issue_days);
        }
        Role::Receiver => {
            let deploy_state: Option<DeployState> = state::load(&cfg.deploy_state_path())?;
            let staged = state::load::<state::Staged>(&cfg.staging_dir().join("latest.json"))?;
            let targets = match targets::resolve_targets(&cfg) {
                Ok(t) => json!(t
                    .iter()
                    .map(|t| json!({"id": t.id, "path": t.path.display().to_string()}))
                    .collect::<Vec<_>>()),
                Err(e) => json!({ "error": rkp_mini_core::error::to_sync_error(&e).message }),
            };
            if let Some(st) = &staged {
                text.push_str(&format!(
                    "staged: seq {} keybox {}\n",
                    st.seq,
                    &st.keybox_sha256[..12]
                ));
            }
            if let Some(d) = &deploy_state {
                text.push_str(&format!(
                    "deployed: seq {} keybox {}\n",
                    d.deployed_seq.map_or("-".into(), |s| s.to_string()),
                    d.deployed_keybox_sha256
                        .as_deref()
                        .map_or("-", |h| &h[..12.min(h.len())]),
                ));
                if let Some(p) = &d.pending {
                    text.push_str(&format!(
                        "PENDING: seq {} ({}) — run `rkp-mini-deployer apply` to review\n",
                        p.seq, p.reason
                    ));
                }
                for t in &d.targets {
                    text.push_str(&format!(
                        "  target {} {}: {}\n",
                        t.id,
                        t.path,
                        t.last_error.as_ref().map_or_else(
                            || t.reload_status.clone().unwrap_or_else(|| "ok".into()),
                            |e| format!("error {}", e.code)
                        )
                    ));
                }
            }
            text.push_str(&format!(
                "identity_change_policy: {} (lead {} days), auto_deploy: {}\n",
                cfg.deploy.identity_change_policy.as_str(),
                cfg.deploy.rotation_lead_days,
                cfg.deploy.auto_deploy
            ));
            data["deploy_state"] = json!(deploy_state);
            data["staged"] = json!(staged);
            data["targets"] = targets;
            data["identity_change_policy"] = json!(cfg.deploy.identity_change_policy.as_str());
        }
    }
    text.push_str("note: certificate signatures, root trust and revocation are not verified");
    Ok(Output::ok(data, text))
}

fn sender_keybox_status(cfg: &Config, now: i64) -> serde_json::Value {
    let path = cfg.keybox_path();
    let bytes = match rkp_mini_core::fsutil::read_limited(
        &path,
        rkp_mini_core::limits::MAX_KEYBOX_XML,
    ) {
        Ok(b) => b,
        Err(e) => {
            return json!({ "path": path.display().to_string(), "valid": false, "summary": format!("unreadable ({e})") })
        }
    };
    match rkp_mini_core::keybox::validate_structure(&bytes) {
        Ok(f) => {
            let days = (f.leaf_not_after - now).div_euclid(rkp_mini_core::DAY);
            json!({
                "path": path.display().to_string(),
                "valid": true,
                "device_id": f.device_id,
                "keybox_sha256": hex::encode(f.sha256),
                "leaf_not_after": f.leaf_not_after,
                "days_left": days,
                "summary": format!("{} expires {} ({days} days)", f.device_id, util::date(f.leaf_not_after)),
            })
        }
        Err(e) => {
            let se = rkp_mini_core::error::to_sync_error(&e);
            json!({ "path": path.display().to_string(), "valid": false, "summary": format!("invalid: {}", se.message) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_mini_core::error::to_sync_error;
    use rkp_mini_core::locator::UrlPolicy;
    use rkp_mini_core::state::SyncState;
    use rkp_mini_runtime::context::Global;
    use std::path::{Path, PathBuf};

    /// A complete, enabled receiver config with a secret-file passphrase.
    fn receiver(dir: &Path) -> (PathBuf, Ctx) {
        let config = dir.join("config.toml");
        std::fs::write(&config, config::receiver_template(dir)).unwrap();
        config::edit_table(
            &config,
            "sync",
            &[
                ("enabled", Value::Bool(true)),
                ("risk_ack_version", Value::Int(RISK_VERSION as i64)),
                ("stream_id", Value::Str("st_01JTESTSTREAM")),
                ("provider", Value::Str("url")),
                ("url", Value::Str("https://example.com/rkp-sync.age")),
            ],
        )
        .unwrap();
        let ctx = Ctx {
            global: Global {
                config: Some(config.clone()),
                ..Global::default()
            },
        };
        (config, ctx)
    }

    fn load_cfg(config: &Path) -> Config {
        config::load(config, Some(Role::Receiver), UrlPolicy::default()).unwrap()
    }

    fn write_state(config: &Path, verified: bool) {
        let cfg = load_cfg(config);
        let mut s = SyncState::new(
            "url",
            "https://example.com/rkp-sync.age",
            "st_01JTESTSTREAM",
        );
        if verified {
            s.verified_seq = 3;
            s.plaintext_sha256 = Some(hex::encode([7u8; 32]));
        }
        state::save(&cfg.sync_state_path(), &s).unwrap();
    }

    fn code_of(result: Result<Output>) -> String {
        match result {
            Err(e) => to_sync_error(&e).code,
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn require_verified_needs_a_completed_first_import() {
        let dir = tempfile::tempdir().unwrap();
        let (config, ctx) = receiver(dir.path());
        // Plain check-auto stays a pure configuration check.
        assert!(check_auto(&ctx, false).is_ok());
        assert_eq!(
            code_of(check_auto(&ctx, true)),
            "first_import_needs_confirmation"
        );

        write_state(&config, false);
        assert_eq!(
            code_of(check_auto(&ctx, true)),
            "first_import_needs_confirmation"
        );

        write_state(&config, true);
        let out = check_auto(&ctx, true).unwrap();
        assert_eq!(out.data["auto"], serde_json::Value::Bool(true));

        // A damaged state file keeps its own error instead of "needs confirmation".
        let cfg = load_cfg(&config);
        std::fs::write(cfg.sync_state_path(), b"{not json").unwrap();
        assert_eq!(code_of(check_auto(&ctx, true)), "state_corrupt");
    }

    #[test]
    fn require_verified_rejects_a_sender_and_keeps_plain_check_auto() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            format!(
                "output_dir = \"{}\"\n{}",
                dir.path().join("output").display(),
                config::sender_sync_template("st_01JTESTSTREAM", "gist", dir.path())
            ),
        )
        .unwrap();
        config::edit_table(
            &config,
            "sync",
            &[
                ("enabled", Value::Bool(true)),
                ("risk_ack_version", Value::Int(RISK_VERSION as i64)),
            ],
        )
        .unwrap();
        let ctx = Ctx {
            global: Global {
                config: Some(config),
                ..Global::default()
            },
        };
        assert_eq!(code_of(check_auto(&ctx, true)), "wrong_role");
        assert!(check_auto(&ctx, false).is_ok());
    }
}
