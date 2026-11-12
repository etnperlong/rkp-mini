//! Read-only remote inspection (`check-remote`) and remote deletion
//! (`remote-delete`). Both stop at the envelope: they never touch staging,
//! deploy state or the remote's neighbours.

use anyhow::Result;
use rkp_mini_core::error::{err, to_sync_error, Stage, SyncError};
use rkp_mini_core::state::SyncState;
use rkp_mini_sync_core::protocol::{self, RemoteHead};
use rkp_mini_sync_core::secrecy::SecretString;
use rkp_mini_sync_core::transport::Transport;
use serde_json::{json, Value};

use crate::app;
use crate::remote::Remote;
use crate::tty;
use rkp_mini_runtime::config::{self, Config, Role, Value as ConfigValue};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::Output;
use rkp_mini_runtime::{lock, util};

/// What the check compares against the remote, per role (contract C1).
#[derive(Default)]
struct LocalFacts {
    /// Sender: last pushed seq; receiver: last verified seq.
    seq: Option<u64>,
    keybox_sha256: Option<String>,
    /// Sender only: digest of the last pushed plaintext (divergence check).
    plaintext_sha256: Option<String>,
}

impl LocalFacts {
    fn of(cfg: &Config, state: Option<&SyncState>) -> LocalFacts {
        let state = state.filter(|s| s.has_verified());
        let mut f = LocalFacts {
            seq: state.map(|s| s.verified_seq),
            keybox_sha256: state.and_then(|s| s.keybox_sha256.clone()),
            plaintext_sha256: state.and_then(|s| s.plaintext_sha256.clone()),
        };
        if cfg.role == Role::Sender {
            // Push decides on today's keybox, not on the last upload.
            let kb = std::fs::read(cfg.keybox_path()).ok();
            f.keybox_sha256 = kb.map(|b| rkp_mini_core::sha256_hex(&b));
        }
        f
    }
}

/// Outcome rule; first match wins.
fn evaluate(role: Role, local: &LocalFacts, remote: Option<&RemoteHead>) -> &'static str {
    let Some(r) = remote else {
        return "missing";
    };
    let plaintext_hex = hex::encode(r.plaintext_sha256);
    let keybox_hex = hex::encode(r.keybox_sha256);
    let same_plaintext = local.plaintext_sha256.as_deref() == Some(plaintext_hex.as_str());
    match role {
        Role::Sender => {
            let mut keybox_differs = false;
            if let Some(h) = &local.keybox_sha256 {
                keybox_differs = h != &keybox_hex;
            }
            if local.seq.is_some_and(|s| r.seq > s) {
                "remote_ahead"
            } else if local.seq == Some(r.seq) && !same_plaintext {
                "diverged"
            } else if keybox_differs {
                "remote_behind"
            } else {
                "in_sync"
            }
        }
        Role::Receiver => {
            let Some(seq) = local.seq else {
                return "remote_ahead";
            };
            if r.seq > seq {
                "remote_ahead"
            } else if r.seq < seq {
                "remote_behind"
            } else if !same_plaintext {
                "diverged"
            } else {
                "in_sync"
            }
        }
    }
}

fn record(
    role: Role,
    local: &LocalFacts,
    outcome: &str,
    remote: Option<&RemoteHead>,
    error: Option<&SyncError>,
    at: i64,
) -> Value {
    let remote_json = remote.map_or(Value::Null, |r| {
        json!({
            "seq": r.seq,
            "plaintext_sha256": hex::encode(r.plaintext_sha256),
            "keybox_sha256": hex::encode(r.keybox_sha256),
            "device_id": r.device_id,
            "issued_at": r.issued_at,
            "keybox_not_after": r.keybox_not_after,
        })
    });
    let error_json = error.map_or(Value::Null, |e| {
        json!({
            "stage": e.stage.as_str(),
            "code": e.code.clone(),
            "message": e.message.clone(),
        })
    });
    json!({
        "at": at,
        "role": role.as_str(),
        "outcome": outcome,
        "remote": remote_json,
        "local": { "seq": local.seq, "keybox_sha256": local.keybox_sha256 },
        "error": error_json,
    })
}

pub fn check_remote(ctx: &Ctx) -> Result<Output> {
    let cfg = ctx.load(None)?;
    let now = util::now();
    let state = match app::load_sync_state(&cfg) {
        Ok(s) => s,
        Err(e) => {
            let rec = record(
                cfg.role,
                &LocalFacts::default(),
                "error",
                None,
                Some(&to_sync_error(&e)),
                now,
            );
            let _ = app::save_remote_check(&cfg, &rec);
            return Err(e);
        }
    };
    let local = LocalFacts::of(&cfg, state.as_ref());
    let rec = match inspect_remote(ctx, &cfg, &local, now) {
        Ok(r) => r,
        Err(e) => {
            let failed = record(
                cfg.role,
                &local,
                "error",
                None,
                Some(&to_sync_error(&e)),
                now,
            );
            let _ = app::save_remote_check(&cfg, &failed);
            return Err(e);
        }
    };
    app::save_remote_check(&cfg, &rec)?;
    let text = describe(&rec);
    Ok(Output::ok(rec, text))
}

fn describe(record: &Value) -> String {
    let outcome = record["outcome"].as_str().unwrap_or("-");
    let remote = &record["remote"];
    if remote.is_null() {
        return format!("{outcome}: nothing stored at the remote location");
    }
    let local_seq = record["local"]["seq"].to_string();
    let local_kb = record["local"]["keybox_sha256"].as_str().unwrap_or("-");
    format!(
        "{outcome}: remote seq {} keybox {}\nlocal seq {local_seq} keybox {}",
        remote["seq"].as_u64().unwrap_or(0),
        trunc(remote["keybox_sha256"].as_str().unwrap_or("-")),
        trunc(local_kb),
    )
}

fn trunc(hash: &str) -> &str {
    &hash[..16.min(hash.len())]
}

/// Open the configured remote, then [`inspect`] it.
fn inspect_remote(ctx: &Ctx, cfg: &Config, local: &LocalFacts, now: i64) -> Result<Value> {
    let http = app::http(ctx, cfg)?;
    let remote = Remote::open(&http, &cfg.location()?, app::credentials(cfg)?)?;
    inspect(ctx, cfg, local, remote.transport(), now)
}

/// Read + decrypt the head and compare it with the local facts.
fn inspect(
    ctx: &Ctx,
    cfg: &Config,
    local: &LocalFacts,
    t: &dyn Transport,
    now: i64,
) -> Result<Value> {
    let stream_id = cfg.require_stream()?;
    let passphrase = app::passphrase(ctx, cfg, false)?;
    let head = protocol::read_remote_head(t, &passphrase, stream_id)?;
    let outcome = evaluate(cfg.role, local, head.as_ref());
    Ok(record(cfg.role, local, outcome, head.as_ref(), None, now))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteOutcome {
    Missing,
    Deleted,
}

/// Decrypt the head first, then delete it with compare-and-swap on that
/// token: an unreadable or foreign remote is never deleted.
fn delete_checked(
    t: &dyn Transport,
    passphrase: &SecretString,
    stream_id: &str,
) -> Result<DeleteOutcome> {
    match protocol::read_remote_head(t, passphrase, stream_id)? {
        None => Ok(DeleteOutcome::Missing),
        Some(h) => {
            t.delete(h.token.as_deref())?;
            Ok(DeleteOutcome::Deleted)
        }
    }
}

/// A gist cannot be emptied; clearing the handle makes the next push create a
/// new one.
fn clear_gist(cfg: &Config) -> Result<()> {
    config::edit_table(
        &cfg.path,
        "sync",
        &[
            ("gist_id", ConfigValue::Str("")),
            ("gist_url", ConfigValue::Str("")),
        ],
    )
}

pub fn remote_delete(ctx: &Ctx, yes: bool) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Sender))?;
    if !yes
        && (!ctx.interactive(false)
            || !tty::confirm("Delete the remote envelope? Local state and secrets are kept.")?)
    {
        return Err(err(
            Stage::Config,
            "confirmation_required",
            "pass --yes to confirm",
        ));
    }
    let _lock = lock::lock(&cfg.lock_path())?;
    let stream_id = cfg.require_stream()?.to_string();
    let passphrase = app::passphrase(ctx, &cfg, false)?;
    let http = app::http(ctx, &cfg)?;
    let remote = Remote::open(&http, &cfg.location()?, app::credentials(&cfg)?)?;
    let t = remote.transport();
    let outcome = delete_checked(t, &passphrase, &stream_id)?;
    let provider = t.provider();
    let location = t.location();
    if outcome == DeleteOutcome::Deleted && provider == "gist" {
        clear_gist(&cfg)?;
    }
    let state = app::load_sync_state(&cfg)?;
    let local = LocalFacts::of(&cfg, state.as_ref());
    app::save_remote_check(
        &cfg,
        &record(Role::Sender, &local, "missing", None, None, util::now()),
    )?;
    let (data, text) = match outcome {
        DeleteOutcome::Missing => (
            json!({ "deleted": false, "reason": "missing", "provider": provider, "location": location }),
            format!("nothing stored at {location}; nothing was deleted"),
        ),
        DeleteOutcome::Deleted => {
            let mut text = format!(
                "deleted the remote envelope at {location}\nlocal sync state kept (seq continuity)"
            );
            if provider == "gist" {
                text.push_str("; the next push creates a new gist");
            }
            (
                json!({ "deleted": true, "provider": provider, "location": location }),
                text,
            )
        }
    };
    Ok(Output::ok(data, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_mini_core::locator::UrlPolicy;
    use rkp_mini_runtime::context::Global;
    use rkp_mini_sync_core::envelope;
    use rkp_mini_sync_core::manifest::{KeyboxSummary, Manifest};
    use rkp_mini_sync_core::transport::DirTransport;

    const STREAM: &str = "st_01JTESTSTREAM";

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    /// Seal one manifest at `path`; returns its plaintext digest.
    fn write_remote(
        path: &std::path::Path,
        pass: &SecretString,
        seq: u64,
        kb: [u8; 32],
    ) -> [u8; 32] {
        let m = Manifest {
            stream_id: STREAM.into(),
            seq,
            prev: None,
            issued_at: util::now(),
            keybox: KeyboxSummary {
                sha256: kb,
                device_id: "rkp-mini-test".into(),
                leaf_not_before: 0,
                leaf_not_after: 1,
                chain_sha256: [0u8; 32],
            },
            keybox_xml: b"<x/>".to_vec(),
            meta_json: None,
        };
        let plaintext = m.encode().unwrap();
        let armored = envelope::encrypt(&plaintext, pass).unwrap();
        std::fs::write(path, armored.as_bytes()).unwrap();
        rkp_mini_core::sha256(&plaintext)
    }

    #[test]
    fn outcomes_follow_the_contract() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let pass = pw("password");
        assert_eq!(
            evaluate(Role::Sender, &LocalFacts::default(), None),
            "missing"
        );
        assert!(protocol::read_remote_head(&t, &pass, STREAM)
            .unwrap()
            .is_none());

        let kb = [7u8; 32];
        let digest = write_remote(&t.path, &pass, 4, kb);
        let head = protocol::read_remote_head(&t, &pass, STREAM)
            .unwrap()
            .unwrap();
        assert_eq!(head.seq, 4);
        assert_eq!(head.device_id, "rkp-mini-test");
        let remote = Some(&head);
        let kb_hex = hex::encode(kb);

        let mut sender = LocalFacts {
            seq: Some(4),
            keybox_sha256: Some(kb_hex.clone()),
            plaintext_sha256: Some(hex::encode(digest)),
        };
        assert_eq!(evaluate(Role::Sender, &sender, remote), "in_sync");
        sender.seq = Some(3);
        assert_eq!(evaluate(Role::Sender, &sender, remote), "remote_ahead");
        sender.seq = Some(4);
        sender.plaintext_sha256 = Some(hex::encode([9u8; 32]));
        assert_eq!(evaluate(Role::Sender, &sender, remote), "diverged");
        sender.plaintext_sha256 = Some(hex::encode(digest));
        sender.keybox_sha256 = Some(hex::encode([8u8; 32]));
        assert_eq!(evaluate(Role::Sender, &sender, remote), "remote_behind");
        // No state at all, current keybox matches the remote.
        let fresh = LocalFacts {
            keybox_sha256: Some(kb_hex.clone()),
            ..Default::default()
        };
        assert_eq!(evaluate(Role::Sender, &fresh, remote), "in_sync");

        let receiver = LocalFacts {
            seq: Some(4),
            keybox_sha256: Some(kb_hex.clone()),
            plaintext_sha256: Some(hex::encode(digest)),
        };
        assert_eq!(evaluate(Role::Receiver, &receiver, remote), "in_sync");
        assert_eq!(
            evaluate(Role::Receiver, &LocalFacts::default(), remote),
            "remote_ahead",
            "no verified state"
        );
        let behind = LocalFacts {
            seq: Some(3),
            ..Default::default()
        };
        assert_eq!(evaluate(Role::Receiver, &behind, remote), "remote_ahead");
        let ahead = LocalFacts {
            seq: Some(5),
            ..Default::default()
        };
        assert_eq!(evaluate(Role::Receiver, &ahead, remote), "remote_behind");
        let diverged = LocalFacts {
            seq: Some(4),
            plaintext_sha256: Some(hex::encode([1u8; 32])),
            ..Default::default()
        };
        assert_eq!(evaluate(Role::Receiver, &diverged, remote), "diverged");
    }

    /// A sender config with a secret-file passphrase, a keybox and state.
    fn sender(dir: &std::path::Path) -> (Ctx, Config) {
        let config = dir.join("config.toml");
        std::fs::write(
            &config,
            format!(
                "output_dir = \"{}\"\n{}",
                dir.join("output").display(),
                config::sender_sync_template(STREAM, "gist", dir)
            ),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("output")).unwrap();
        let keybox = dir.join("output/keybox.xml");
        std::fs::write(&keybox, b"<keybox/>").unwrap();
        let ctx = Ctx {
            global: Global {
                config: Some(config),
                ..Global::default()
            },
        };
        let cfg = ctx.load(Some(Role::Sender)).unwrap();
        (ctx, cfg)
    }

    /// Serve `body` for every request; returns the base URL.
    fn serve(body: &[u8]) -> String {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        let body = body.to_vec();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                if r.read_line(&mut line).unwrap() == 0 {
                    continue;
                }
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                s.write_all(head.as_bytes()).unwrap();
                s.write_all(&body).unwrap();
            }
        });
        base
    }

    #[test]
    fn check_remote_reports_without_writing_state_or_remote() {
        let dir = tempfile::tempdir().unwrap();
        let pass = pw("password");
        let sealed = dir.path().join("served.age");
        let digest = write_remote(&sealed, &pass, 3, [5u8; 32]);
        let body = std::fs::read(&sealed).unwrap();
        let base = serve(&body);

        let config = dir.path().join("config.toml");
        std::fs::write(&config, config::receiver_template(dir.path())).unwrap();
        let url = format!("{base}/rkp-sync.age");
        config::edit_table(
            &config,
            "sync",
            &[
                ("stream_id", ConfigValue::Str(STREAM)),
                ("provider", ConfigValue::Str("url")),
                ("url", ConfigValue::Str(&url)),
            ],
        )
        .unwrap();
        let ctx = Ctx {
            global: Global {
                config: Some(config),
                allow_insecure_http: true,
                allow_private: true,
                ..Global::default()
            },
        };
        // `enabled` stays false: check-remote must not need it.
        let cfg = ctx.load(Some(Role::Receiver)).unwrap();
        rkp_mini_sync_core::secret::write_secret_file(
            &cfg.password_file(),
            b"password",
            "age passphrase",
        )
        .unwrap();
        let mut state = SyncState::new("url", &url, STREAM);
        state.verified_seq = 2;
        state.plaintext_sha256 = Some(hex::encode([1u8; 32]));
        state.keybox_sha256 = Some(hex::encode([9u8; 32]));
        rkp_mini_core::state::save(&cfg.sync_state_path(), &state).unwrap();
        let state_before = std::fs::read(cfg.sync_state_path()).unwrap();

        let out = check_remote(&ctx).unwrap();
        assert_eq!(out.data["outcome"], "remote_ahead");
        assert_eq!(out.data["remote"]["seq"].as_u64(), Some(3));
        assert_eq!(std::fs::read(cfg.sync_state_path()).unwrap(), state_before);
        assert_eq!(std::fs::read(&sealed).unwrap(), body);
        let cached = app::load_remote_check(&cfg).unwrap();
        let cached_plaintext = cached["remote"]["plaintext_sha256"].as_str();
        assert_eq!(cached_plaintext, Some(hex::encode(digest).as_str()));
    }

    #[test]
    fn remote_delete_refuses_foreign_remote_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let pass = pw("password");
        assert_eq!(
            delete_checked(&t, &pass, STREAM).unwrap(),
            DeleteOutcome::Missing
        );
        assert!(!t.path.exists(), "a missing remote is not created");

        write_remote(&t.path, &pass, 1, [1u8; 32]);
        let before = std::fs::read(&t.path).unwrap();
        let e = delete_checked(&t, &pw("other"), STREAM).unwrap_err();
        assert_eq!(to_sync_error(&e).code, "decrypt_failed");
        assert_eq!(std::fs::read(&t.path).unwrap(), before);
        let e = delete_checked(&t, &pass, "st_01JOTHERSTREAM").unwrap_err();
        assert_eq!(to_sync_error(&e).code, "stream_mismatch");
        assert_eq!(std::fs::read(&t.path).unwrap(), before);

        assert_eq!(
            delete_checked(&t, &pass, STREAM).unwrap(),
            DeleteOutcome::Deleted
        );
        assert!(!t.path.exists());
        assert_eq!(
            delete_checked(&t, &pass, STREAM).unwrap(),
            DeleteOutcome::Missing
        );
    }

    #[test]
    fn gist_delete_clears_the_config_handle() {
        let dir = tempfile::tempdir().unwrap();
        let (_, cfg) = sender(dir.path());
        config::edit_table(
            &cfg.path,
            "sync",
            &[
                ("gist_id", ConfigValue::Str("abc123")),
                ("gist_url", ConfigValue::Str("https://gist.example/x")),
            ],
        )
        .unwrap();
        let cfg = config::load(&cfg.path, Some(Role::Sender), UrlPolicy::default()).unwrap();
        clear_gist(&cfg).unwrap();
        let cfg = config::load(&cfg.path, Some(Role::Sender), UrlPolicy::default()).unwrap();
        assert!(cfg.sync.gist_id.is_empty() && cfg.sync.gist_url.is_empty());
        let text = std::fs::read_to_string(&cfg.path).unwrap();
        assert!(text.contains("auto_push = true"), "rest of the file kept");
        assert!(text.contains("gist_filename"));
    }
}
