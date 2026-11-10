//! Receiver commands: import, fetch, deploy, run (design §11).

use anyhow::Result;
use rkp_mini_core::error::{err, to_sync_error, Stage, SyncError};
use rkp_mini_core::locator::{self, ImportSpec, Location};
use rkp_mini_core::state::{self, Staged};
use rkp_mini_core::PROTOCOL_VERSION;
use rkp_mini_sync_core::protocol::{self, FetchOutcome, FetchRequest, Received};
use rkp_mini_sync_core::secrecy::ExposeSecret;
use rkp_mini_sync_core::secret;
use rkp_mini_sync_core::transport::Fetch;
use serde_json::{json, Value as Json};

use crate::app;
use crate::common;
use crate::deploy::{self, ApplyOptions, Gate};
use crate::remote::Remote;
use crate::tty;
use rkp_mini_runtime::config::{self, Config, PasswordMode, Role, Value, RISK_VERSION};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::Output;
use rkp_mini_runtime::targets;
use rkp_mini_runtime::{lock, util};

/// `full` (lowercase hex) matches a confirmation value (full or >= 16-char prefix).
fn sha_confirms(full: &str, given: &str) -> bool {
    given.len() >= 16 && full.starts_with(given)
}

fn confirmation_mismatch(what: &str) -> SyncError {
    SyncError::new(
        Stage::State,
        "confirmation_mismatch",
        format!("the {what} changed since it was reviewed; review it again"),
    )
}

/// Credential-free location fields for previews.
fn location_json(loc: &Location) -> Json {
    match loc {
        Location::Gist {
            url,
            gist_id,
            filename,
        } => json!({ "provider": "gist", "url": url, "gist_id": gist_id, "filename": filename }),
        Location::Webdav {
            endpoint,
            path,
            user,
        } => json!({ "provider": "webdav", "endpoint": endpoint, "path": path, "user": user }),
        Location::Url { url } => json!({ "provider": "url", "url": url }),
    }
}

fn describe(spec: &ImportSpec) -> String {
    let mut s = format!(
        "provider:  {}\nlocation:  {}\nstream:    {}\n",
        spec.location.provider(),
        spec.location.display(),
        spec.stream_id
    );
    if let Location::Webdav { user: Some(u), .. } = &spec.location {
        s.push_str(&format!("user:      {u}\n"));
    }
    if let Some(l) = &spec.label {
        s.push_str(&format!("label:     {l}\n"));
    }
    s
}

/// Parse-only preview for the WebUI (`import --check`): writes nothing.
fn import_check(ctx: &Ctx, spec: &ImportSpec) -> Result<Output> {
    let path = ctx.config_path(Some(Role::Receiver))?;
    let current = config::load(&path, Some(Role::Receiver), ctx.url_policy()).ok();
    let current_stream = current
        .as_ref()
        .map(|c| c.sync.stream_id.clone())
        .filter(|s| !s.is_empty());
    let verified = current
        .as_ref()
        .and_then(|c| app::load_sync_state(c).ok().flatten())
        .filter(|s| s.has_verified());
    let stream_change = current_stream
        .as_deref()
        .is_some_and(|s| s != spec.stream_id);
    Ok(Output::ok(
        json!({
            "location": location_json(&spec.location),
            "display": spec.location.display(),
            "stream_id": spec.stream_id,
            "label": spec.label,
            "current_stream_id": current_stream,
            "stream_change": stream_change,
            "blocked": stream_change && verified.is_some(),
            "verified_seq": verified.map(|s| s.verified_seq),
        }),
        describe(spec),
    ))
}

pub fn import(ctx: &Ctx, input: &str, yes: bool, test: bool, check: bool) -> Result<Output> {
    let spec = locator::parse_import(input, ctx.url_policy())?;
    if check {
        return import_check(ctx, &spec);
    }
    let path = ctx.config_path(Some(Role::Receiver))?;
    if !path.exists() {
        let root = path.parent().filter(|p| p.is_absolute()).ok_or_else(|| {
            err(
                Stage::Config,
                "config_invalid",
                "config path must be absolute",
            )
        })?;
        config::write_config(&path, config::receiver_template(root).as_bytes())?;
        eprintln!("created {}", path.display());
    }
    let cfg = config::load(&path, Some(Role::Receiver), ctx.url_policy())?;
    let interactive = ctx.interactive(false);
    eprintln!("Import location (verify you expect this; a wrong link cannot decrypt without your passphrase):\n{}", describe(&spec));
    if !yes {
        if !interactive {
            return Err(err(
                Stage::Config,
                "confirmation_required",
                "review the location above and pass --yes to import it",
            ));
        }
        if !tty::confirm("Use this location?")? {
            return Err(err(Stage::Config, "cancelled", "import cancelled"));
        }
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let existing = app::load_sync_state(&cfg)?;
    if !cfg.sync.stream_id.is_empty()
        && cfg.sync.stream_id != spec.stream_id
        && existing.as_ref().is_some_and(|s| s.has_verified())
    {
        return Err(err(
            Stage::Config,
            "stream_change",
            format!(
                "this receiver follows stream {}; run `rkp-sync forget` before importing another stream",
                cfg.sync.stream_id
            ),
        ));
    }
    let (gist_url, gist_id, gist_filename, endpoint, wpath, user, url) = match &spec.location {
        Location::Gist {
            url,
            gist_id,
            filename,
        } => (
            url.clone().unwrap_or_default(),
            gist_id.clone().unwrap_or_default(),
            filename.clone(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
        Location::Webdav {
            endpoint,
            path,
            user,
        } => (
            String::new(),
            String::new(),
            rkp_mini_core::REMOTE_FILENAME.to_string(),
            endpoint.clone(),
            path.clone(),
            user.clone().unwrap_or_default(),
            String::new(),
        ),
        Location::Url { url } => (
            String::new(),
            String::new(),
            rkp_mini_core::REMOTE_FILENAME.to_string(),
            String::new(),
            String::new(),
            String::new(),
            url.clone(),
        ),
    };
    config::edit_table(
        &cfg.path,
        "sync",
        &[
            ("provider", Value::Str(spec.location.provider())),
            ("stream_id", Value::Str(&spec.stream_id)),
            ("label", Value::Str(spec.label.as_deref().unwrap_or(""))),
            ("gist_url", Value::Str(&gist_url)),
            ("gist_id", Value::Str(&gist_id)),
            ("gist_filename", Value::Str(&gist_filename)),
            ("webdav_endpoint", Value::Str(&endpoint)),
            ("webdav_path", Value::Str(&wpath)),
            ("webdav_user", Value::Str(&user)),
            ("url", Value::Str(&url)),
        ],
    )?;
    let cfg = config::load(&path, Some(Role::Receiver), ctx.url_policy())?;
    let mut notes: Vec<String> = Vec::new();
    if existing.as_ref().is_some_and(|s| s.has_verified()) {
        notes.push(format!(
            "kept verified seq {}; the new location must serve the same stream at an equal or higher seq",
            existing.as_ref().map_or(0, |s| s.verified_seq)
        ));
    }

    // Risk acceptance on first enable (§11.2 step 3).
    if cfg.sync.risk_ack_version < RISK_VERSION {
        if interactive && common::accept_risk(ctx, &cfg, None)? {
            config::edit_table(&cfg.path, "sync", &[("enabled", Value::Bool(true))])?;
            notes.push("risk accepted; sync enabled".into());
        } else {
            notes.push("sync stays disabled until `rkp-sync enable` (risk acceptance)".into());
        }
    }
    let cfg = config::load(&path, Some(Role::Receiver), ctx.url_policy())?;

    // Passphrase / WebDAV password (§11.2 step 4).
    if cfg.sync.password_mode == PasswordMode::SecretFile {
        let p = cfg.password_file();
        if !p.exists() {
            if interactive {
                eprintln!("Enter the age passphrase you received over a separate secure channel.\nIt will be stored in {} (0600).", p.display());
                let pw = tty::read_secret("age passphrase", true)?;
                secret::write_secret_file(&p, pw.expose_secret().as_bytes(), "age passphrase")?;
                notes.push(format!("age passphrase saved to {}", p.display()));
            } else {
                notes.push("set the passphrase with `rkp-sync secret set age`".into());
            }
        }
    }
    if matches!(spec.location, Location::Webdav { user: Some(_), .. }) {
        let p = cfg.webdav_password_file();
        if !p.exists() {
            if interactive {
                let pw = tty::read_secret("WebDAV app password", false)?;
                secret::write_secret_file(&p, pw.expose_secret().as_bytes(), "WebDAV password")?;
                notes.push(format!("WebDAV password saved to {}", p.display()));
            } else {
                notes.push(
                    "set the WebDAV password with `rkp-sync secret set webdav-password`".into(),
                );
            }
        }
    }

    // Optional reachability test: download only, no decryption. The location
    // is already saved, so a failing test is reported rather than fatal.
    let mut test_result = Json::Null;
    if test {
        let probe = || -> Result<Fetch> {
            let http = app::http(ctx, &cfg)?;
            let remote = Remote::open(&http, &cfg.location()?, app::credentials(&cfg)?)?;
            remote.transport().fetch(None)
        };
        test_result = match probe() {
            Ok(Fetch::Body { bytes, .. }) => {
                let armored = String::from_utf8_lossy(&bytes[..bytes.len().min(64)])
                    .trim_start()
                    .starts_with("-----BEGIN AGE ENCRYPTED FILE-----");
                notes.push(format!(
                    "test: downloaded {} bytes ({})",
                    bytes.len(),
                    if armored {
                        "age armor"
                    } else {
                        "NOT an age armor file"
                    }
                ));
                json!({ "bytes": bytes.len(), "age_armor": armored })
            }
            Ok(Fetch::Missing) => {
                notes.push("test: nothing stored at this location yet".into());
                json!({ "missing": true })
            }
            Ok(Fetch::NotModified) => Json::Null,
            Err(e) => {
                let se = to_sync_error(&e);
                notes.push(format!("test failed: {se}"));
                json!({ "error": { "code": se.code, "message": se.message } })
            }
        };
    }
    notes.push("nothing was applied; next: `rkp-sync fetch` then `rkp-sync deploy apply`".into());
    let mut text = format!("imported:\n{}", describe(&spec));
    for n in &notes {
        text.push_str(&format!("- {n}\n"));
    }
    Ok(Output::ok(
        json!({
            "provider": spec.location.provider(),
            "location": spec.location.display(),
            "stream_id": spec.stream_id,
            "label": spec.label,
            "enabled": cfg.sync.enabled,
            "test": test_result,
            "notes": notes,
        }),
        text.trim_end().to_string(),
    ))
}

fn received_summary(r: &Received) -> String {
    format!(
        "seq:       {}{}\nissued_at: {}\ndevice_id: {}\nexpires:   {} (leaf not_after)\nkeybox:    {}\n",
        r.manifest.seq,
        if r.gap { " (skipped versions)" } else { "" },
        util::date(r.manifest.issued_at),
        r.facts.device_id,
        util::date(r.facts.leaf_not_after),
        &hex::encode(r.facts.sha256)[..16],
    )
}

struct FetchResult {
    data: Json,
    text: String,
    /// Set when the fetch stopped for confirmation; `data` carries the facts.
    failure: Option<SyncError>,
}

fn fetch_inner(
    ctx: &Ctx,
    cfg: &Config,
    accept_first: Option<&str>,
    no_cache: bool,
    auto: bool,
) -> Result<FetchResult> {
    cfg.require_enabled()?;
    let stream_id = cfg.require_stream()?.to_string();
    let loc = cfg.location()?;
    let passphrase = app::passphrase(ctx, cfg, auto)?;
    let http = app::http(ctx, cfg)?;
    let remote = Remote::open(&http, &loc, app::credentials(cfg)?)?;
    let t = remote.transport();
    let state = app::load_sync_state(cfg)?;
    let now = util::now();
    let req = FetchRequest {
        stream_id: &stream_id,
        passphrase: &passphrase,
        state: state.as_ref(),
        now,
        min_validity_days: cfg.min_validity_days(),
        conditional: !no_cache,
    };
    let r = match protocol::fetch(t, &req)? {
        FetchOutcome::NotModified => {
            let mut s = state.expect("conditional fetch implies state");
            s.last_checked_at = Some(now);
            s.last_error = None;
            state::save(&cfg.sync_state_path(), &s)?;
            return Ok(FetchResult {
                data: json!({ "provider": t.provider(), "stream_id": stream_id, "changed": false, "seq": s.verified_seq }),
                text: format!("unchanged (seq {})", s.verified_seq),
                failure: None,
            });
        }
        FetchOutcome::Received(r) => *r,
    };
    let mut warnings = Vec::new();
    if r.first_import {
        // TOFU (§7.4 step 9): explicit confirmation of exactly what is trusted.
        let summary = received_summary(&r);
        let plaintext_hex = hex::encode(r.plaintext_sha256);
        let targets: Vec<String> = targets::resolve_targets(cfg)
            .map(|ts| ts.iter().map(|t| t.path.display().to_string()).collect())
            .unwrap_or_default();
        let facts = json!({
            "provider": t.provider(),
            "location": t.location(),
            "stream_id": stream_id,
            "seq": r.manifest.seq,
            "issued_at": r.manifest.issued_at,
            "device_id": r.facts.device_id,
            "leaf_not_before": r.facts.leaf_not_before,
            "leaf_not_after": r.facts.leaf_not_after,
            "keybox_sha256": hex::encode(r.facts.sha256),
            "plaintext_sha256": plaintext_hex,
            "targets": targets,
            "root_trust_checked": false,
        });
        let failure = match accept_first {
            Some(given) if sha_confirms(&plaintext_hex, given) => {
                eprintln!("first import (trust on first use):\n{summary}");
                None
            }
            Some(_) => Some(confirmation_mismatch("remote version")),
            None if ctx.interactive(auto) => {
                eprintln!(
                    "First import from this stream (trust on first use). Verify with the sender:\n{summary}\
                     Deployment targets: {}\n",
                    if targets.is_empty() { "none configured".into() } else { targets.join(", ") }
                );
                (!tty::confirm("Trust this version?")?).then(|| {
                    SyncError::new(Stage::State, "first_import_needs_confirmation", "first import declined")
                })
            }
            None => Some(SyncError::new(
                Stage::State,
                "first_import_needs_confirmation",
                "first import must be confirmed: review the facts and pass --accept-first <plaintext_sha256>",
            )),
        };
        if failure.is_some() {
            // Nothing staged, no state written.
            return Ok(FetchResult {
                data: facts,
                text: format!("first import needs confirmation:\n{summary}"),
                failure,
            });
        }
    }
    if r.gap {
        warnings.push(json!({ "code": "gap", "message": format!("skipped from seq {} to {}", r.previous_seq.unwrap_or(0), r.manifest.seq) }));
    }
    let staged = Staged {
        v: 1,
        stream_id: stream_id.clone(),
        seq: r.manifest.seq,
        keybox_sha256: hex::encode(r.facts.sha256),
        plaintext_sha256: hex::encode(r.plaintext_sha256),
        device_id: r.facts.device_id.clone(),
        leaf_not_before: r.facts.leaf_not_before,
        leaf_not_after: r.facts.leaf_not_after,
        staged_at: now,
    };
    let staged_path = state::write_staging(&cfg.staging_dir(), &staged, &r.manifest.keybox_xml)?;
    let st = protocol::record_received(state, t.provider(), &t.location(), &r, now);
    state::save(&cfg.sync_state_path(), &st)?;
    eprintln!(
        "rkp-sync fetch: provider={} seq={} keybox={}{}",
        t.provider(),
        r.manifest.seq,
        &staged.keybox_sha256[..16],
        if r.gap { " gap=1" } else { "" }
    );
    let data = json!({
        "provider": t.provider(),
        "stream_id": stream_id,
        "changed": !r.idempotent,
        "seq": r.manifest.seq,
        "plaintext_sha256": staged.plaintext_sha256,
        "keybox_sha256": staged.keybox_sha256,
        "device_id": staged.device_id,
        "leaf_not_after": staged.leaf_not_after,
        "staged_path": staged_path.display().to_string(),
        "previous_seq": r.previous_seq,
        "gap": r.gap,
        "first_import": r.first_import,
        "warnings": warnings,
        "root_trust_checked": false,
    });
    let text = format!(
        "verified and staged:\n{}staged at {} (0600)\nnote: certificate signatures/root trust/revocation are not checked",
        received_summary(&r),
        staged_path.display()
    );
    Ok(FetchResult {
        data,
        text,
        failure: None,
    })
}

pub fn fetch(
    ctx: &Ctx,
    accept_first: Option<String>,
    no_cache: bool,
    auto: bool,
) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    let _lock = lock::lock(&cfg.lock_path())?;
    match fetch_inner(ctx, &cfg, accept_first.as_deref(), no_cache, auto) {
        Ok(r) => Ok(Output {
            data: r.data,
            text: r.text,
            failure: r.failure,
        }),
        Err(e) => {
            app::record_error(&cfg, &e, util::now());
            Err(e)
        }
    }
}

fn gate_json(gate: &Gate) -> Json {
    match gate {
        Gate::Same => json!({ "kind": "same" }),
        Gate::FirstDeploy => json!({ "kind": "first_deploy" }),
        Gate::Allowed { reason } => json!({ "kind": "allowed", "reason": reason }),
        Gate::Blocked { reason } => json!({ "kind": "blocked", "reason": reason }),
    }
}

fn plan_json(plan: &deploy::Plan) -> Json {
    json!({
        "gate": gate_json(&plan.gate),
        "staged": plan.staged,
        "old_keybox_sha256": plan.old_sha256,
        "old_leaf_not_after": plan.old_not_after,
        "warnings": plan.warnings,
        "targets": plan.targets.iter().map(|t| json!({
            "id": t.id,
            "path": t.path.display().to_string(),
            "owner": t.owner_string(),
            "mode": t.mode_string(),
            "reload": t.reload.as_string(),
        })).collect::<Vec<_>>(),
        "root_trust_checked": false,
    })
}

/// Read-only preview of what `deploy apply` would do (§19.3).
pub fn deploy_plan(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    cfg.require_stream()?;
    let plan = deploy::plan(&cfg, util::now(), false)?;
    let text = format!(
        "staged seq {} keybox {} → gate {}",
        plan.staged.seq,
        &plan.staged.keybox_sha256[..16],
        gate_json(&plan.gate)
    );
    Ok(Output::ok(plan_json(&plan), text))
}

fn apply_inner(
    ctx: &Ctx,
    cfg: &Config,
    allow_identity_change: Option<&str>,
    confirm_first: Option<&str>,
    auto: bool,
) -> Result<Output> {
    cfg.require_enabled()?;
    cfg.require_stream()?;
    let now = util::now();
    let plan = deploy::plan(cfg, now, allow_identity_change.is_some())?;
    // Approvals name the reviewed keybox; a different staged one is refused.
    for given in [allow_identity_change, confirm_first].into_iter().flatten() {
        if !sha_confirms(&plan.staged.keybox_sha256, given) {
            return Ok(Output {
                data: plan_json(&plan),
                text: "the staged keybox changed since it was reviewed".into(),
                failure: Some(confirmation_mismatch("staged keybox")),
            });
        }
    }
    if let (Some(old), Gate::Allowed { reason } | Gate::Blocked { reason }) =
        (&plan.old_sha256, &plan.gate)
    {
        eprintln!(
            "identity change ({reason}): deployed keybox {} (expires {}) → staged {} (expires {})",
            &old[..16.min(old.len())],
            plan.old_not_after.map_or("-".into(), util::date),
            &plan.staged.keybox_sha256[..16],
            util::date(plan.staged.leaf_not_after)
        );
    }
    for w in &plan.warnings {
        eprintln!("warning [{}]: {}", w.code, w.message);
    }
    let mut confirm = confirm_first.is_some();
    if matches!(plan.gate, Gate::FirstDeploy) && !confirm && ctx.interactive(auto) {
        eprintln!(
            "First deployment of keybox {} (seq {}, device {}, expires {}) to:",
            &plan.staged.keybox_sha256[..16],
            plan.staged.seq,
            plan.staged.device_id,
            util::date(plan.staged.leaf_not_after)
        );
        for t in &plan.targets {
            eprintln!("  {} → {}", t.id, t.path.display());
        }
        confirm = tty::confirm("Write the keybox to these targets?")?;
    }
    let mut opts = ApplyOptions::new(now);
    opts.allow_identity_change = allow_identity_change.is_some();
    opts.confirm_first = confirm;
    let report = deploy::apply(cfg, &opts)?;
    let failed: Vec<&str> = report
        .targets
        .iter()
        .filter(|t| !t.ok)
        .map(|t| t.id.as_str())
        .collect();
    let mut text = String::new();
    if let Some(p) = &report.pending {
        text.push_str(&format!(
            "NOT deployed: seq {} held as pending ({}).\n",
            p.seq, p.reason
        ));
        text.push_str(match p.reason.as_str() {
            "first_deploy" => "Review it with `rkp-sync deploy plan`, then run `rkp-sync deploy apply --confirm-first <keybox_sha256>`.",
            _ => {
                "The identity change is outside the rotation policy; review `rkp-sync deploy plan` and run\n\
                  `rkp-sync deploy apply --allow-identity-change <keybox_sha256>` to deploy it now."
            }
        });
    } else {
        text.push_str(&format!(
            "keybox {} (seq {}):\n",
            &report.keybox_sha256[..16],
            report.seq
        ));
        for t in &report.targets {
            text.push_str(&format!(
                "  {} {}: {}{}\n",
                t.id,
                t.path.display(),
                if t.ok { "ok" } else { "FAILED" },
                t.error
                    .as_ref()
                    .map(|e| format!(" ({}: {})", e.code, e.message))
                    .or_else(|| t.reload_status.as_ref().map(|r| format!(" reload={r}")))
                    .unwrap_or_default()
            ));
        }
    }
    let data = serde_json::to_value(&report).expect("serializable report");
    eprintln!(
        "rkp-sync deploy: seq={} keybox={} deployed={} failed={}",
        report.seq,
        &report.keybox_sha256[..16.min(report.keybox_sha256.len())],
        report.deployed,
        failed.len()
    );
    let failure = (!failed.is_empty()).then(|| {
        SyncError::new(
            Stage::Deploy,
            "target_failed",
            format!("deployment failed for: {}", failed.join(", ")),
        )
    });
    Ok(Output {
        data,
        text: text.trim_end().to_string(),
        failure,
    })
}

pub fn deploy_apply(
    ctx: &Ctx,
    allow_identity_change: Option<String>,
    confirm_first: Option<String>,
    auto: bool,
) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    let _lock = lock::lock(&cfg.lock_path())?;
    apply_inner(
        ctx,
        &cfg,
        allow_identity_change.as_deref(),
        confirm_first.as_deref(),
        auto,
    )
}

pub fn add_target(
    ctx: &Ctx,
    id: &str,
    path: Option<String>,
    reload: Option<String>,
) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    let (default_path, owner) = match id {
        "omk" => ("/data/misc/keystore/omk/keybox.xml", "1017:1017"),
        "teesim" => ("/data/adb/teesim/keybox.xml", "0:0"),
        "tricky_store" => ("/data/adb/tricky_store/keybox.xml", "0:0"),
        _ => {
            return Err(err(
                Stage::Config,
                "usage",
                "built-in targets: omk, teesim, tricky_store (custom targets are added by editing the config)",
            ))
        }
    };
    let path = path.unwrap_or_else(|| default_path.to_string());
    let reload = reload.unwrap_or_else(|| "auto".to_string());
    config::add_target(
        &cfg.path,
        id,
        &[
            ("path", &path),
            ("owner", owner),
            ("mode", "0600"),
            ("reload", &reload),
        ],
    )?;
    // Validate the result; roll back on error.
    let reloaded = config::load(&cfg.path, Some(Role::Receiver), ctx.url_policy())?;
    if let Err(e) = targets::resolve_targets(&reloaded) {
        config::remove_target(&cfg.path, id)?;
        return Err(e);
    }
    let mut text = format!("added target {id} → {path}");
    if id == "omk" {
        text.push_str("\nnote: OMK retire_stale_keybox_bound_keys (default true) deletes client keys bound to a replaced keybox");
    }
    if id == "teesim" {
        text.push_str(
            "\nnote: the file name must be referenced by a profile in /data/adb/teesim/config.json",
        );
    }
    Ok(Output::ok(
        json!({ "id": id, "path": path, "reload": reload }),
        text,
    ))
}

pub fn remove_target(ctx: &Ctx, id: &str) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    if !config::remove_target(&cfg.path, id)? {
        return Err(err(
            Stage::Config,
            "unknown_target",
            format!("no target `{id}`"),
        ));
    }
    Ok(Output::ok(
        json!({ "id": id, "removed": true }),
        format!("removed target {id} from the config (the deployed file was not touched)"),
    ))
}

pub fn deploy_remove(ctx: &Ctx, targets: Vec<String>, yes: bool) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    // Two confirmations (§13.3): the flag, plus typing on a TTY when available.
    if !yes {
        return Err(err(
            Stage::Config,
            "confirmation_required",
            "removing deployed keyboxes needs --yes (tools may fall back to no keybox)",
        ));
    }
    if ctx.interactive(false) {
        let a = tty::ask("Type `REMOVE` to delete the deployed keybox file(s): ")?;
        if a.trim() != "REMOVE" {
            return Err(err(Stage::Config, "cancelled", "removal cancelled"));
        }
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let reports = deploy::remove(&cfg, &targets)?;
    let failed = reports.iter().filter(|r| !r.ok).count();
    let mut text = String::new();
    for r in &reports {
        text.push_str(&format!(
            "  {} {}: {}\n",
            r.id,
            r.path.display(),
            if r.ok {
                "removed".to_string()
            } else {
                r.error
                    .as_ref()
                    .map_or("failed".into(), |e| format!("{}: {}", e.code, e.message))
            }
        ));
    }
    Ok(Output {
        data: json!({ "targets": reports }),
        text: text.trim_end().to_string(),
        failure: (failed > 0).then(|| {
            SyncError::new(
                Stage::Deploy,
                "target_failed",
                format!("{failed} target(s) not removed"),
            )
        }),
    })
}

/// fetch + deploy. Deploy runs even without a new version so that pending
/// rotations re-evaluate and damaged targets are repaired.
pub fn run(ctx: &Ctx, auto: bool) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    cfg.require_enabled()?;
    if auto && app::load_sync_state(&cfg)?.is_none() {
        return Err(err(
            Stage::State,
            "first_import_needs_confirmation",
            "unattended runs start after the first manual `rkp-sync fetch`",
        ));
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let (fetch_data, fetch_err, fetch_text) = match fetch_inner(ctx, &cfg, None, false, auto) {
        Ok(r) => (r.data, r.failure, r.text),
        Err(e) => {
            app::record_error(&cfg, &e, util::now());
            let se = to_sync_error(&e);
            let text = format!("fetch failed: {se}");
            (Json::Null, Some(se), text)
        }
    };
    let staged_exists = cfg.staging_dir().join("latest.json").exists();
    let (deploy_data, deploy_failure, deploy_text) = if !cfg.deploy.auto_deploy && auto {
        (
            Json::Null,
            None,
            "auto_deploy = false; not deploying".to_string(),
        )
    } else if !staged_exists {
        (Json::Null, None, "nothing staged".to_string())
    } else {
        match apply_inner(ctx, &cfg, None, None, auto) {
            Ok(o) => (o.data, o.failure, o.text),
            Err(e) => {
                let se = to_sync_error(&e);
                (Json::Null, Some(se.clone()), format!("deploy failed: {se}"))
            }
        }
    };
    let err_json = |e: &Option<SyncError>| {
        e.as_ref()
            .map(|e| json!({ "stage": e.stage.as_str(), "code": e.code, "message": e.message }))
    };
    let data = json!({
        "protocol": PROTOCOL_VERSION,
        "fetch": fetch_data,
        "fetch_error": err_json(&fetch_err),
        "deploy": deploy_data,
        "deploy_error": err_json(&deploy_failure),
    });
    // Deploy failures (exit 9) take precedence over sync failures (exit 8).
    let failure = deploy_failure.or(fetch_err);
    Ok(Output {
        data,
        text: format!("{fetch_text}\n{deploy_text}"),
        failure,
    })
}
