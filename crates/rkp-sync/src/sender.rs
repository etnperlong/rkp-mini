//! Sender commands: push / sync-now, share, rotate-password (design §10).

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use anyhow::Result;
use rkp_mini_core::error::{err, to_sync_error, Stage};
use rkp_mini_core::locator::{encode_import, ImportSpec, Location};
use rkp_mini_core::{fsutil, limits, state};
use rkp_mini_sync_core::protocol::{self, PushOutcome, PushRequest};
use rkp_mini_sync_core::secrecy::{ExposeSecret, SecretString};
use rkp_mini_sync_core::secret;
use serde_json::json;

use crate::app;
use crate::common;
use crate::remote::Remote;
use rkp_mini_runtime::config::{self, Config, PasswordMode, Role, Value};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::Output;
use rkp_mini_runtime::{lock, util};

/// Retries after a `remote_conflict` (re-read, new seq, try again).
const CONFLICT_RETRIES: usize = 2;

struct LocalKeybox {
    xml: Vec<u8>,
    meta: Option<Vec<u8>>,
}

/// Read keybox.xml (must be a 0600 regular file) and optional meta.json.
fn read_local(cfg: &Config) -> Result<LocalKeybox> {
    let path = cfg.keybox_path();
    let meta = std::fs::symlink_metadata(&path).map_err(|e| {
        err(
            Stage::Validate,
            "keybox_missing",
            format!("{}: {e}; run `rkp-mini issue` first", path.display()),
        )
    })?;
    if !meta.is_file() {
        return Err(err(
            Stage::Validate,
            "keybox_invalid",
            format!("{} is not a regular file", path.display()),
        ));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(err(
            Stage::Validate,
            "keybox_permissions",
            format!(
                "{} has mode {:o}; require 0600",
                path.display(),
                meta.mode() & 0o777
            ),
        ));
    }
    let xml = fsutil::read_limited(&path, limits::MAX_KEYBOX_XML).map_err(|e| {
        err(
            Stage::Validate,
            "keybox_invalid",
            format!("{}: {e}", path.display()),
        )
    })?;
    let meta = match fsutil::read_limited(&cfg.meta_path(), limits::MAX_META_JSON) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("warning: meta.json skipped ({e}); it is diagnostic only");
            None
        }
    };
    Ok(LocalKeybox { xml, meta })
}

struct PushArgs {
    force: bool,
    allow_expiring: bool,
    auto: bool,
    /// Rotation: the passphrase for the new envelope.
    new_passphrase: Option<SecretString>,
}

fn push_inner(ctx: &Ctx, cfg: &Config, args: &PushArgs) -> Result<(PushOutcome, String)> {
    cfg.require_enabled()?;
    let stream_id = cfg.require_stream()?.to_string();
    let loc = cfg.location()?;
    let local = read_local(cfg)?;
    let creds = app::credentials(cfg)?;
    let passphrase = app::passphrase(ctx, cfg, args.auto)?;
    let http = app::http(ctx, cfg)?;
    let remote = Remote::open(&http, &loc, creds)?;
    let t = remote.transport();
    let mut attempt = 0;
    let out = loop {
        let state = app::load_sync_state(cfg)?;
        let now = util::now();
        let req = PushRequest {
            stream_id: &stream_id,
            keybox_xml: &local.xml,
            meta_json: local.meta.as_deref(),
            remote_passphrase: &passphrase,
            passphrase: args.new_passphrase.as_ref().unwrap_or(&passphrase),
            state: state.as_ref(),
            now,
            min_validity_days: cfg.min_validity_days(),
            allow_expiring: args.allow_expiring,
            force: args.force,
        };
        match protocol::push(t, &req) {
            Ok(out) => {
                let st = protocol::record_push(state, t, &stream_id, &out, now);
                state::save(&cfg.sync_state_path(), &st)?;
                break out;
            }
            Err(e) if to_sync_error(&e).code == "remote_conflict" && attempt < CONFLICT_RETRIES => {
                attempt += 1;
                eprintln!("rkp-sync: remote changed concurrently; retrying ({attempt})");
            }
            Err(e) => return Err(e),
        }
    };
    if let Some(created) = remote.created_gist() {
        config::edit_table(
            &cfg.path,
            "sync",
            &[
                ("gist_id", Value::Str(&created.id)),
                ("gist_url", Value::Str(&created.url)),
            ],
        )?;
    }
    Ok((out, t.location()))
}

fn push_output(cfg: &Config, out: &PushOutcome, url: &str, allow_expiring: bool) -> Output {
    let data = json!({
        "provider": cfg.sync.provider,
        "stream_id": cfg.sync.stream_id,
        "seq": out.seq,
        "noop": out.noop,
        "ciphertext_sha256": out.ciphertext_sha256,
        "plaintext_sha256": hex::encode(out.plaintext_sha256),
        "keybox_sha256": hex::encode(out.facts.sha256),
        "device_id": out.facts.device_id,
        "leaf_not_after": out.facts.leaf_not_after,
        "url": url,
        "remote_seq": out.remote_seq,
        "base": out.base.as_str(),
        "uploaded_at": (!out.noop).then(util::now),
    });
    // Log line: provider/seq/hash only.
    eprintln!(
        "rkp-sync push: provider={} seq={} keybox={} {}{}",
        cfg.sync.provider,
        out.seq,
        &hex::encode(out.facts.sha256)[..16],
        if out.noop { "noop" } else { "uploaded" },
        if allow_expiring {
            " allow_expiring=1"
        } else {
            ""
        }
    );
    let text = if out.noop {
        format!(
            "remote already has this keybox (seq {}); nothing uploaded. Use --force to republish.",
            out.seq
        )
    } else {
        format!(
            "uploaded seq {} to {url}\nkeybox {} expires {}",
            out.seq,
            out.facts.device_id,
            util::date(out.facts.leaf_not_after)
        )
    };
    Output::ok(data, text)
}

pub fn push(ctx: &Ctx, force: bool, allow_expiring: bool, auto: bool) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Sender))?;
    if auto && allow_expiring {
        return Err(err(
            Stage::Config,
            "usage",
            "--allow-expiring is for manual emergency use only",
        ));
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let args = PushArgs {
        force,
        allow_expiring,
        auto,
        new_passphrase: None,
    };
    match push_inner(ctx, &cfg, &args) {
        Ok((out, url)) => Ok(push_output(&cfg, &out, &url, allow_expiring)),
        Err(e) => {
            app::record_error(&cfg, &e, util::now());
            Err(e)
        }
    }
}

pub fn share(ctx: &Ctx, label: Option<String>) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Sender))?;
    let stream_id = cfg.require_stream()?.to_string();
    let label = label.or_else(|| (!cfg.sync.label.is_empty()).then(|| cfg.sync.label.clone()));
    let location = match cfg.location()? {
        Location::Gist {
            url: None,
            gist_id: None,
            ..
        } => {
            return Err(err(
                Stage::Config,
                "not_published",
                "the gist does not exist yet; run `rkp-sync push` first",
            ))
        }
        Location::Gist {
            url,
            gist_id,
            filename,
        } => Location::Gist {
            url,
            gist_id,
            filename,
        },
        other => other,
    };
    let spec = ImportSpec {
        stream_id,
        label,
        location,
    };
    // Re-validate through the receiver's parser so share never emits a link
    // the receiver would reject.
    let (uri, json_text) = encode_import(&spec);
    rkp_mini_core::locator::parse_import(&uri, ctx.url_policy())?;
    let code = qrcode::QrCode::new(uri.as_bytes())
        .map_err(|e| err(Stage::Generic, "qr", format!("QR encode: {e}")))?;
    let qr = code
        .render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(true)
        .build();
    // Module matrix for the WebUI to draw locally (no QR library in the page).
    let rows: Vec<String> = code
        .to_colors()
        .chunks(code.width())
        .map(|row| {
            row.iter()
                .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect();
    let text = format!(
        "Import link (location only — it does NOT contain the passphrase):\n\n{uri}\n\n{qr}\n\nJSON: {json_text}\n\n\
         The link and QR code may be shared. Send the age passphrase separately over a\n\
         secure channel; the receiver enters it with `rkp-sync secret set age`.\n\
         Prefer scanning/pasting inside the receiver over opening rkpsync:// links in a browser."
    );
    Ok(Output::ok(
        json!({
            "uri": uri,
            "json": serde_json::from_str::<serde_json::Value>(&json_text).expect("json"),
            "contains_password": false,
            "qr": { "size": code.width(), "rows": rows },
        }),
        text,
    ))
}

pub fn rotate_password(ctx: &Ctx, from_file: Option<PathBuf>, hex: bool) -> Result<Output> {
    // Consume an inbox file first so it never outlives this command.
    let inboxed = from_file
        .as_deref()
        .map(|f| rkp_mini_sync_core::secret::take_inbox_file(f, hex, "new age passphrase"))
        .transpose()?;
    let cfg = ctx.load(Some(Role::Sender))?;
    if !ctx.interactive(false) && cfg.sync.password_mode == PasswordMode::Prompt {
        return Err(err(
            Stage::Config,
            "usage",
            "rotate-password with password_mode = \"prompt\" needs a TTY",
        ));
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let pending: PathBuf = {
        let mut p = cfg.password_file().into_os_string();
        p.push(".new");
        PathBuf::from(p)
    };
    if pending.exists() {
        return Err(err(
            Stage::State,
            "rotation_pending",
            format!(
                "{} exists from an interrupted rotation; check which passphrase opens the remote, \
                 then move or delete it",
                pending.display()
            ),
        ));
    }
    let current = app::passphrase(ctx, &cfg, false)?;
    let new = match inboxed {
        Some(n) => n,
        None => {
            eprintln!("Enter the NEW age passphrase (6+ random words or 20+ random characters).");
            common::read_secret_input(ctx, None, false, "new age passphrase", true)?
        }
    };
    if new.expose_secret() == current.expose_secret() {
        return Err(err(
            Stage::Config,
            "usage",
            "the new passphrase equals the current one",
        ));
    }
    let file_mode = cfg.sync.password_mode == PasswordMode::SecretFile;
    if file_mode {
        secret::write_secret_file(
            &pending,
            new.expose_secret().as_bytes(),
            "new age passphrase",
        )?;
    }
    let args = PushArgs {
        force: true,
        allow_expiring: false,
        auto: false,
        new_passphrase: Some(new),
    };
    let result = push_inner(ctx, &cfg, &args);
    match result {
        Ok((out, url)) => {
            if file_mode {
                std::fs::rename(&pending, cfg.password_file()).map_err(|e| {
                    err(
                        Stage::State,
                        "secret_write",
                        format!(
                            "remote now uses the NEW passphrase but replacing {} failed: {e}; \
                             the new passphrase is in {}",
                            cfg.password_file().display(),
                            pending.display()
                        ),
                    )
                })?;
            }
            let mut o = push_output(&cfg, &out, &url, false);
            o.text = format!(
                "re-encrypted seq {} with the new passphrase.\n\
                 Receivers must now run `rkp-sync secret set age` with the new passphrase.\n\
                 Old envelopes (e.g. Gist revisions) still open with the old passphrase and contain\n\
                 the same private key: to really revoke, re-issue the keybox (`rkp-mini issue`) and push.",
                out.seq
            );
            Ok(o)
        }
        Err(e) => {
            if file_mode {
                let _ = std::fs::remove_file(&pending);
            }
            app::record_error(&cfg, &e, util::now());
            Err(e)
        }
    }
}
