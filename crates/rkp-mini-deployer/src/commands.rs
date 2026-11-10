//! Deployer commands: preview, apply, and the local target list.

use std::path::Path;

use anyhow::Result;
use rkp_mini_core::confirmation::{confirmation_mismatch, sha_confirms};
use rkp_mini_core::error::{err, Stage, SyncError};
use rkp_mini_runtime::config::{self, Config, Role};
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::Output;
use rkp_mini_runtime::targets::{self, BUILTINS, BUILTIN_MODE};
use rkp_mini_runtime::{lock, tty, util};
use serde_json::{json, Value as Json};

use crate::deploy::{self, ApplyOptions, Gate};
use crate::modules;

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

/// Read-only preview of what `apply` would do (§19.3).
pub fn plan(ctx: &Ctx) -> Result<Output> {
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

/// `true` only when `staging/latest.json` is definitely absent; any other
/// stat error is reported instead of being treated as "nothing staged".
fn staging_present(cfg: &Config) -> Result<bool> {
    let latest = cfg.staging_dir().join("latest.json");
    match std::fs::metadata(&latest) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(err(
            Stage::State,
            "staging_unreadable",
            format!("stat {}: {e}", latest.display()),
        )),
    }
}

fn skipped(reason: &str, text: &str) -> Output {
    Output::ok(json!({ "skipped": reason }), text)
}

fn apply_inner(
    ctx: &Ctx,
    cfg: &Config,
    allow_identity_change: Option<&str>,
    confirm_first: Option<&str>,
    auto: bool,
) -> Result<Output> {
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
            "first_deploy" => "Review it with `rkp-mini-deployer plan`, then run `rkp-mini-deployer apply --confirm-first <keybox_sha256>`.",
            _ => {
                "The identity change is outside the rotation policy; review `rkp-mini-deployer plan` and run\n\
                  `rkp-mini-deployer apply --allow-identity-change <keybox_sha256>` to deploy it now."
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
        "rkp-mini-deployer: seq={} keybox={} deployed={} failed={}",
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

/// `apply`: the identity gate, approvals and the local write.
///
/// `--auto` returns a successful no-op when `[deploy].auto_deploy` is false;
/// `--if-staged` returns a successful no-op when nothing was staged. Neither
/// approves anything: an explicit approval still has to name the staged sha.
pub fn apply(
    ctx: &Ctx,
    allow_identity_change: Option<String>,
    confirm_first: Option<String>,
    auto: bool,
    if_staged: bool,
) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    let _lock = lock::lock(&cfg.lock_path())?;
    cfg.require_enabled()?;
    cfg.require_stream()?;
    if auto && !cfg.deploy.auto_deploy {
        return Ok(skipped(
            "auto_deploy_false",
            "auto_deploy = false; nothing was deployed",
        ));
    }
    if if_staged && !staging_present(&cfg)? {
        return Ok(skipped(
            "nothing_staged",
            "nothing staged; nothing was deployed",
        ));
    }
    apply_inner(
        ctx,
        &cfg,
        allow_identity_change.as_deref(),
        confirm_first.as_deref(),
        auto,
    )
}

/// `targets`: the builtin targets and whether the module that reads each one
/// is installed. Read-only and independent of the config.
pub fn targets(modules_dir: &Path) -> Output {
    let mut text = String::new();
    let builtin: Vec<Json> = BUILTINS
        .iter()
        .map(|b| {
            let module = modules::detect(modules_dir, b.module_id);
            let state = match (module.installed, module.enabled) {
                (false, _) => "not installed".to_string(),
                (true, enabled) => format!(
                    "{}{}{}",
                    if enabled {
                        "installed"
                    } else {
                        "installed, disabled"
                    },
                    module
                        .name
                        .as_ref()
                        .map_or(String::new(), |n| format!(": {n}")),
                    module
                        .version
                        .as_ref()
                        .map_or(String::new(), |v| format!(" {v}")),
                ),
            };
            text.push_str(&format!(
                "{} → {} (module {}: {state})\n",
                b.id, b.path, b.module_id
            ));
            json!({
                "id": b.id,
                "path": b.path,
                "owner": format!("{}:{}", b.uid, b.gid),
                "mode": format!("{BUILTIN_MODE:04o}"),
                "module": module,
            })
        })
        .collect();
    Output::ok(json!({ "builtin": builtin }), text.trim_end())
}

pub fn add_target(
    ctx: &Ctx,
    id: &str,
    path: Option<String>,
    reload: Option<String>,
) -> Result<Output> {
    let cfg = ctx.load(Some(Role::Receiver))?;
    let Some(builtin) = targets::builtin(id) else {
        return Err(err(
            Stage::Config,
            "usage",
            "built-in targets: omk, teesim, tricky_store (custom targets are added by editing the config)",
        ));
    };
    let owner = format!("{}:{}", builtin.uid, builtin.gid);
    let path = path.unwrap_or_else(|| builtin.path.to_string());
    let reload = reload.unwrap_or_else(|| "auto".to_string());
    let _lock = lock::lock(&cfg.lock_path())?;
    config::add_target(
        &cfg.path,
        id,
        &[
            ("path", &path),
            ("owner", &owner),
            ("mode", &format!("{BUILTIN_MODE:04o}")),
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
    let _lock = lock::lock(&cfg.lock_path())?;
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

pub fn remove(ctx: &Ctx, targets: Vec<String>, yes: bool) -> Result<Output> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_mini_core::error::to_sync_error;
    use rkp_mini_core::keybox;
    use rkp_mini_core::locator::UrlPolicy;
    use rkp_mini_core::sha256_hex;
    use rkp_mini_core::state::{self, Staged};
    use rkp_mini_core::test_support::TestKeybox;
    use rkp_mini_runtime::config::Value;
    use rkp_mini_runtime::context::Global;
    use std::path::PathBuf;

    const STREAM: &str = "st_01JTESTSTREAM";

    fn uid() -> u32 {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    fn gid() -> u32 {
        // SAFETY: getgid has no preconditions and cannot fail.
        unsafe { libc::getgid() }
    }

    fn code_of(result: Result<Output>) -> String {
        match result {
            Err(e) => to_sync_error(&e).code,
            Ok(out) => panic!("expected an error, got ok: {}", out.data),
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        config: PathBuf,
        target: PathBuf,
    }

    impl Fixture {
        fn new(auto_deploy: bool) -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let config = dir.path().join("config.toml");
            let target = dir.path().join("targets").join("keybox.xml");
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&config, config::receiver_template(dir.path())).unwrap();
            config::edit_table(
                &config,
                "sync",
                &[
                    ("enabled", Value::Bool(true)),
                    ("risk_ack_version", Value::Int(2)),
                    ("stream_id", Value::Str(STREAM)),
                    ("provider", Value::Str("url")),
                    ("url", Value::Str("https://example.com/rkp-sync.age")),
                ],
            )
            .unwrap();
            config::edit_table(
                &config,
                "deploy",
                &[("auto_deploy", Value::Bool(auto_deploy))],
            )
            .unwrap();
            config::add_target(
                &config,
                "custom",
                &[
                    ("path", &target.display().to_string()),
                    ("owner", &format!("{}:{}", uid(), gid())),
                    ("mode", "0600"),
                    ("reload", "none"),
                ],
            )
            .unwrap();
            Fixture {
                dir,
                config,
                target,
            }
        }

        fn ctx(&self) -> Ctx {
            Ctx {
                global: Global {
                    config: Some(self.config.clone()),
                    ..Global::default()
                },
            }
        }

        fn cfg(&self) -> Config {
            config::load(&self.config, Some(Role::Receiver), UrlPolicy::default()).unwrap()
        }

        fn set(&self, table: &str, key: &str, value: Value) {
            config::edit_table(&self.config, table, &[(key, value)]).unwrap();
        }

        /// Write a real staged keybox; returns its sha256.
        fn stage(&self) -> (String, TestKeybox) {
            let cfg = self.cfg();
            let kb = TestKeybox::new(300);
            let facts = keybox::validate_structure(kb.xml.as_bytes()).unwrap();
            let sha = sha256_hex(kb.xml.as_bytes());
            let staged = Staged {
                v: 1,
                stream_id: STREAM.to_string(),
                seq: 1,
                keybox_sha256: sha.clone(),
                plaintext_sha256: sha.clone(),
                device_id: facts.device_id.clone(),
                leaf_not_before: facts.leaf_not_before,
                leaf_not_after: facts.leaf_not_after,
                staged_at: 0,
            };
            state::write_staging(&cfg.staging_dir(), &staged, kb.xml.as_bytes()).unwrap();
            (sha, kb)
        }

        fn deploy_state(&self) -> PathBuf {
            self.cfg().deploy_state_path()
        }

        fn latest(&self) -> PathBuf {
            self.dir.path().join("staging").join("latest.json")
        }
    }

    #[test]
    fn if_staged_skips_only_when_nothing_is_staged() {
        let fx = Fixture::new(true);
        let out = apply(&fx.ctx(), None, None, false, true).unwrap();
        assert_eq!(
            out.data.get("skipped").and_then(|v| v.as_str()),
            Some("nothing_staged")
        );
        assert!(!fx.target.exists(), "a skip must not write a target");

        // A damaged latest.json is an error, never a skip.
        std::fs::create_dir_all(fx.dir.path().join("staging")).unwrap();
        std::fs::write(fx.latest(), b"{not json").unwrap();
        assert_eq!(
            code_of(apply(&fx.ctx(), None, None, false, true)),
            "staging_corrupt"
        );
    }

    #[test]
    fn unreadable_staging_is_not_a_skip() {
        if uid() == 0 {
            return; // root ignores the permission bits
        }
        use std::os::unix::fs::PermissionsExt;
        let fx = Fixture::new(true);
        let staging = fx.dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o000)).unwrap();
        let code = code_of(apply(&fx.ctx(), None, None, false, true));
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_ne!(code, "", "permission errors must be reported");
        assert!(
            !fx.target.exists(),
            "nothing may be deployed on an unreadable staging"
        );
    }

    #[test]
    fn disabled_or_risk_comes_before_the_skip() {
        let fx = Fixture::new(true);
        fx.set("sync", "risk_ack_version", Value::Int(0));
        assert_eq!(
            code_of(apply(&fx.ctx(), None, None, false, true)),
            "risk_not_acknowledged"
        );
        fx.set("sync", "risk_ack_version", Value::Int(2));
        fx.set("sync", "enabled", Value::Bool(false));
        assert_eq!(
            code_of(apply(&fx.ctx(), None, None, false, true)),
            "sync_disabled"
        );
    }

    #[test]
    fn auto_deploy_false_changes_nothing() {
        let fx = Fixture::new(false);
        let (sha, _kb) = fx.stage();
        let sentinel = b"previous keybox".to_vec();
        std::fs::write(&fx.target, &sentinel).unwrap();
        let out = apply(&fx.ctx(), None, None, true, true).unwrap();
        assert_eq!(
            out.data.get("skipped").and_then(|v| v.as_str()),
            Some("auto_deploy_false")
        );
        assert_eq!(std::fs::read(&fx.target).unwrap(), sentinel);
        assert!(
            !fx.deploy_state().exists(),
            "a skip must not touch deploy-state"
        );
        // The manual path ignores auto_deploy and still needs the approval.
        let pending = apply(&fx.ctx(), None, None, false, false).unwrap();
        assert_eq!(
            pending.failure.as_ref().map(|e| e.code.as_str()),
            None,
            "a first deploy without approval is a normal pending result"
        );
        assert_eq!(pending.data["pending"]["seq"].as_u64(), Some(1));
        let applied = apply(&fx.ctx(), None, Some(sha[..16].to_string()), false, false).unwrap();
        assert!(applied.failure.is_none());
        assert_ne!(std::fs::read(&fx.target).unwrap(), sentinel);
    }

    #[test]
    fn missing_staging_without_the_flag_is_an_error() {
        let fx = Fixture::new(true);
        assert_eq!(
            code_of(apply(&fx.ctx(), None, None, false, false)),
            "nothing_staged"
        );
    }

    #[test]
    fn auto_first_deploy_stays_pending_and_later_repairs() {
        let fx = Fixture::new(true);
        let (sha, kb) = fx.stage();
        // --auto never approves a first deployment by itself.
        let out = apply(&fx.ctx(), None, None, true, true).unwrap();
        assert!(out.failure.is_none(), "{:?}", out.failure);
        assert_eq!(out.data["pending"]["seq"].as_u64(), Some(1));
        assert!(!fx.target.exists());

        // An explicit approval for the reviewed keybox deploys it.
        let applied = apply(&fx.ctx(), None, Some(sha[..16].to_string()), false, false).unwrap();
        assert!(applied.failure.is_none(), "{:?}", applied.failure);
        assert_eq!(std::fs::read(&fx.target).unwrap(), kb.xml.as_bytes());

        // The same keybox is now the deployed one: a later auto run repairs a
        // damaged target without any approval.
        std::fs::write(&fx.target, b"tampered").unwrap();
        let repaired = apply(&fx.ctx(), None, None, true, true).unwrap();
        assert!(repaired.failure.is_none(), "{:?}", repaired.failure);
        assert_eq!(std::fs::read(&fx.target).unwrap(), kb.xml.as_bytes());
    }

    #[test]
    fn targets_reports_only_installed_modules() {
        let dir = tempfile::tempdir().unwrap();
        // TrickyStore and TEESimulator-RS both install as `tricky_store`.
        let module = dir.path().join("tricky_store");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(
            module.join("module.prop"),
            "id=tricky_store\nname=TEESimulator-RS\nversion=v6.0.1\n",
        )
        .unwrap();

        let out = targets(dir.path());
        let builtin = out.data["builtin"].as_array().unwrap();
        let by_id = |id: &str| builtin.iter().find(|b| b["id"] == id).unwrap();
        assert_eq!(builtin.len(), 3);

        let ts = by_id("tricky_store");
        assert_eq!(ts["path"], "/data/adb/tricky_store/keybox.xml");
        assert_eq!(ts["owner"], "0:0");
        assert_eq!(ts["mode"], "0600");
        assert_eq!(ts["module"]["installed"], true);
        assert_eq!(ts["module"]["name"], "TEESimulator-RS");

        let omk = by_id("omk");
        assert_eq!(omk["owner"], "1017:1017");
        assert_eq!(omk["module"]["id"], "oh_my_keymint");
        assert_eq!(omk["module"]["installed"], false);
        assert_eq!(by_id("teesim")["module"]["installed"], false);
    }

    #[test]
    fn stale_approval_is_refused_and_leaves_the_target_alone() {
        let fx = Fixture::new(true);
        let sentinel = b"old".to_vec();
        std::fs::write(&fx.target, &sentinel).unwrap();
        let (sha, _kb) = fx.stage();
        let wrong = if sha.starts_with("0000") {
            "1111"
        } else {
            "0000"
        };
        let out = apply(
            &fx.ctx(),
            None,
            Some(format!("{wrong}0123456789ab")),
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            out.failure.as_ref().map(|e| e.code.as_str()),
            Some("confirmation_mismatch")
        );
        assert_eq!(std::fs::read(&fx.target).unwrap(), sentinel);
    }
}
