//! Role-independent commands: init, enable/disable, forget, secrets, status.

use std::path::Path;

use anyhow::Result;
use rkp_sync_core::error::{err, Stage};
use rkp_sync_core::secret;
use rkp_sync_core::state::{self, DeployState};
use serde_json::json;

use crate::app::{self, Ctx, Output};
use crate::cli::SecretKind;
use crate::config::{self, Config, PasswordMode, Role, Value, RISK_VERSION};
use crate::{deploy, tty};

/// Risk statement (§13.2). Bump `RISK_VERSION` whenever this text changes.
pub const RISK_STATEMENT: &str = "\
Remote keybox sync — risk statement (v1) / 远程 keybox 同步风险说明

 1. The keybox is the issuing device's RKP identity. Using it on another
    device is keybox cloning/spoofing, not legitimate RKP provisioning.
    该 keybox 是签发设备的 RKP 身份；在新设备上使用属于克隆/冒充，不是合法 provisioning。
 2. Play Integrity, Key Attestation and banking/payment apps may reject it,
    and Google may revoke it.
    Play Integrity、Key Attestation、银行/支付应用可能拒绝；Google 可能吊销。
 3. The private key is copied to another device; if either device leaks,
    both are compromised.
    私钥会被复制到新设备；任一台泄露，另一台一起失守。
 4. For unattended runs the age passphrase is stored in a 0600 file. Losing
    the device or root compromise leaks it; then change the passphrase AND
    re-issue the keybox.
    自动运行时 age 密码保存在 0600 文件；设备丢失/root 失守即泄露，需换密码并重新签发。
 5. OhMyKeymint's retire_stale_keybox_bound_keys (default true) deletes
    client keys bound to the previous keybox when the keybox changes.
    OMK 的 retire_stale_keybox_bound_keys=true 会在换 keybox 时删除旧 key。
 6. Disabling sync does not revoke anything; the risk is yours.
    禁用不等于撤销；风险由使用者承担。
";

pub const ACCEPT_PHRASE: &str = "I ACCEPT";

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
                Some(s) if rkp_sync_core::valid_stream_id(&s) => s,
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
                    "a receiver gets provider/stream from `rkp-sync import`",
                ));
            }
            config::receiver_template(&root)
        }
    };
    config::write_config(&path, text.as_bytes())?;
    let cfg = config::load(&path, Some(role), ctx.url_policy())?;
    rkp_sync_core::fsutil::ensure_private_dir(&cfg.sync_dir()).map_err(|e| {
        err(
            Stage::State,
            "mkdir",
            format!("{}: {e}", cfg.sync_dir().display()),
        )
    })?;
    let next = match role {
        Role::Sender => "next: `rkp-sync secret set age`, `rkp-sync secret set gist-token` (or webdav-password), `rkp-sync enable`, `rkp-sync push`",
        Role::Receiver => "next: `rkp-sync import '<rkpsync://…>'`, `rkp-sync deploy add-target <omk|teesim|tricky_store>`",
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
pub fn accept_risk(ctx: &Ctx, cfg: &Config, accept_flag: bool) -> Result<bool> {
    eprintln!("{RISK_STATEMENT}");
    if cfg.sync.password_mode == PasswordMode::SecretFile {
        eprintln!(
            "The age passphrase will be stored at {} (0600) for unattended runs.\n",
            cfg.password_file().display()
        );
    }
    let accepted = if accept_flag {
        true
    } else if ctx.interactive(false) {
        tty::ask(&format!("Type `{ACCEPT_PHRASE}` to accept: "))?.trim() == ACCEPT_PHRASE
    } else {
        return Err(err(
            Stage::Config,
            "risk_not_acknowledged",
            "risk acceptance needs a TTY (type the phrase) or an explicit --accept-risk",
        ));
    };
    if !accepted {
        return Ok(false);
    }
    config::edit_table(
        &cfg.path,
        "sync",
        &[
            ("risk_ack_version", Value::Int(i64::from(RISK_VERSION))),
            ("risk_ack_at", Value::Int(app::now())),
        ],
    )?;
    Ok(true)
}

pub fn enable(ctx: &Ctx, accept_flag: bool) -> Result<Output> {
    let cfg = ctx.load(None)?;
    if cfg.sync.risk_ack_version < RISK_VERSION && !accept_risk(ctx, &cfg, accept_flag)? {
        return Err(err(
            Stage::Config,
            "risk_not_acknowledged",
            "risk statement not accepted; sync stays disabled",
        ));
    }
    config::edit_table(&cfg.path, "sync", &[("enabled", Value::Bool(true))])?;
    eprintln!("rkp-sync: enabled ({})", cfg.role.as_str());
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
    let _lock = app::lock(&cfg.lock_path())?;
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

pub fn secret_set(ctx: &Ctx, kind: SecretKind) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let (path, what, twice) = match kind {
        SecretKind::Age => {
            if cfg.sync.password_mode != PasswordMode::SecretFile {
                return Err(err(
                    Stage::Config,
                    "usage",
                    "password_mode is \"prompt\"; nothing is stored",
                ));
            }
            (cfg.password_file(), "age passphrase", true)
        }
        SecretKind::GistToken => {
            if cfg.role != Role::Sender {
                return Err(err(
                    Stage::Config,
                    "usage",
                    "receivers read Gists without a token",
                ));
            }
            (cfg.gist_token_file(), "Gist token", false)
        }
        SecretKind::WebdavPassword => (cfg.webdav_password_file(), "WebDAV app password", false),
    };
    if ctx.global.json && tty::stdin_is_tty() {
        return Err(err(
            Stage::Config,
            "usage",
            "with --json pipe the secret on stdin",
        ));
    }
    let value = tty::read_secret(what, twice)?;
    if kind == SecretKind::Age {
        use rkp_sync_core::secrecy::ExposeSecret;
        if value.expose_secret().chars().count() < 12 {
            eprintln!(
                "warning: short passphrase; use 6+ random words or 20+ random characters, unique per stream"
            );
        }
    }
    {
        use rkp_sync_core::secrecy::ExposeSecret;
        secret::write_secret_file(&path, value.expose_secret().as_bytes(), what)?;
    }
    Ok(Output::ok(
        json!({ "path": path.display().to_string(), "mode": "0600" }),
        format!("{what} saved to {} (0600)", path.display()),
    ))
}

/// Exit 0 when unattended runs may proceed; reason on stderr otherwise.
pub fn check_auto(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    cfg.require_enabled()?;
    cfg.require_stream()?;
    if cfg.sync.password_mode != PasswordMode::SecretFile {
        return Err(err(
            Stage::Config,
            "prompt_in_auto",
            "unattended runs need password_mode = \"secret-file\"",
        ));
    }
    Ok(Output::ok(
        json!({ "auto": true, "role": cfg.role.as_str() }),
        format!("auto: yes ({})", cfg.role.as_str()),
    ))
}

pub fn status(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let now = app::now();
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
                s.keybox_not_after.map_or("-".into(), app::date),
            ));
            if let Some(e) = &s.last_error {
                text.push_str(&format!("last error: {} ({})\n", e.code, app::date(e.at)));
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
            let deploy_state: Option<DeployState> = deploy::status(&cfg)?;
            let staged = state::load::<state::Staged>(&cfg.staging_dir().join("latest.json"))?;
            let targets = match deploy::resolve_targets(&cfg) {
                Ok(t) => json!(t
                    .iter()
                    .map(|t| json!({"id": t.id, "path": t.path.display().to_string()}))
                    .collect::<Vec<_>>()),
                Err(e) => json!({ "error": rkp_sync_core::error::to_sync_error(&e).message }),
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
                        "PENDING: seq {} ({}) — run `rkp-sync deploy apply` to review\n",
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
    let bytes = match rkp_sync_core::fsutil::read_limited(
        &path,
        rkp_sync_core::limits::MAX_KEYBOX_XML,
    ) {
        Ok(b) => b,
        Err(e) => {
            return json!({ "path": path.display().to_string(), "valid": false, "summary": format!("unreadable ({e})") })
        }
    };
    match rkp_sync_core::keybox::validate_structure(&bytes) {
        Ok(f) => {
            let days = (f.leaf_not_after - now).div_euclid(rkp_sync_core::DAY);
            json!({
                "path": path.display().to_string(),
                "valid": true,
                "device_id": f.device_id,
                "keybox_sha256": hex::encode(f.sha256),
                "leaf_not_after": f.leaf_not_after,
                "days_left": days,
                "summary": format!("{} expires {} ({days} days)", f.device_id, app::date(f.leaf_not_after)),
            })
        }
        Err(e) => {
            let se = rkp_sync_core::error::to_sync_error(&e);
            json!({ "path": path.display().to_string(), "valid": false, "summary": format!("invalid: {}", se.message) })
        }
    }
}
