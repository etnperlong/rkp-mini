//! Receiver deployment layer (design §7.2, §7.3, §11.4–§11.6, §14).
//!
//! `fetch` leaves a verified keybox in `staging/`; this module decides whether
//! it may replace the device keybox and writes it to every locally configured
//! target. Paths, owners, modes and reload hooks come **only** from local
//! configuration — the remote side can never select a target.
//!
//! Boundaries:
//! - Targets are resolved and validated up front ([`resolve_targets`]).
//! - [`plan`] is read-only against target files and reports the identity gate;
//!   [`apply`] performs the writes and records [`DeployState`].
//! - Per-target failures never block other targets; a target file is replaced
//!   atomically (same-directory temp + fsync + rename + fsync(dir) + readback)
//!   and keeps its previous content on any pre-rename failure.
//! - `sync-state.json` is never touched.

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{fchown, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rkp_mini_core::error::{err, to_sync_error, Stage};
use rkp_mini_core::fsutil::{read_limited, sync_dir};
use rkp_mini_core::keybox::{self, KeyboxFacts};
use rkp_mini_core::state::{self, DeployState, LastError, Pending, Staged, TargetState};
use rkp_mini_core::{limits, sha256_hex, DAY};
use rkp_mini_runtime::config::{Config, IdentityPolicy};
use rkp_mini_runtime::targets::{
    resolve_targets, Reload, Target, TargetKind, DEFAULT_OMK_RESTART, DEFAULT_TEESIM_CONFIG,
};
use serde::Serialize;

/// Upper bound for the TEESimulator profile file.
const MAX_TEESIM_CONFIG: usize = 256 * 1024;
/// SELinux extended-attribute name copied onto a replaced target.
const SELINUX_XATTR: &str = "security.selinux";

// ---------------------------------------------------------------------------
// Identity gate / plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// The staged keybox is already the deployed one (idempotent repair).
    Same,
    /// No keybox has ever been deployed; requires explicit confirmation.
    FirstDeploy,
    Allowed {
        reason: &'static str,
    },
    Blocked {
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Warning {
    pub code: &'static str,
    pub message: String,
}

/// Read-only deployment plan derived from staging and local state.
#[derive(Debug, Clone)]
pub struct Plan {
    pub staged: Staged,
    pub staged_path: PathBuf,
    pub facts: KeyboxFacts,
    pub gate: Gate,
    /// Previously deployed identity, kept for CLI/UI reporting.
    pub old_sha256: Option<String>,
    /// Previously deployed leaf expiry, kept for CLI/UI reporting.
    pub old_not_after: Option<i64>,
    pub warnings: Vec<Warning>,
    pub targets: Vec<Target>,
}

fn nothing_staged() -> anyhow::Error {
    err(
        Stage::State,
        "nothing_staged",
        "no staged keybox; run `rkp-sync fetch` first",
    )
}

fn staging_corrupt(msg: impl Into<String>) -> anyhow::Error {
    err(Stage::State, "staging_corrupt", msg)
}

/// Load `staging/latest.json` plus the staged keybox bytes.
fn load_staged(dir: &Path) -> Result<(Staged, PathBuf, Vec<u8>)> {
    let latest = dir.join("latest.json");
    let staged: Staged = match state::load(&latest) {
        Ok(Some(s)) => s,
        Ok(None) => return Err(nothing_staged()),
        Err(e) => return Err(staging_corrupt(to_sync_error(&e).message)),
    };
    let path = staged.keybox_path(dir);
    let bytes = match read_limited(&path, limits::MAX_KEYBOX_XML) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(nothing_staged()),
        Err(e) => return Err(staging_corrupt(format!("read {}: {e}", path.display()))),
    };
    Ok((staged, path, bytes))
}

/// Recompute and re-validate the staged keybox (staging is local trust, but
/// this is cheap and catches a half-written or swapped file).
fn verify_staged(staged: &Staged, bytes: &[u8], path: &Path) -> Result<KeyboxFacts> {
    if sha256_hex(bytes) != staged.keybox_sha256 {
        return Err(staging_corrupt(format!(
            "staged keybox sha256 does not match latest.json ({})",
            path.display()
        )));
    }
    keybox::validate_structure(bytes).map_err(|e| {
        staging_corrupt(format!(
            "staged keybox invalid: {}",
            to_sync_error(&e).message
        ))
    })
}

fn read_staged_bytes(path: &Path, sha: &str) -> Result<Vec<u8>> {
    let bytes = read_limited(path, limits::MAX_KEYBOX_XML)
        .map_err(|e| staging_corrupt(format!("read {}: {e}", path.display())))?;
    if sha256_hex(&bytes) != sha {
        return Err(staging_corrupt(format!(
            "staged keybox changed while deploying ({})",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Load `deploy-state.json` and reject a stream owned by another deployment.
fn load_deploy_state(cfg: &Config, stream_id: &str) -> Result<Option<DeployState>> {
    let path = cfg.deploy_state_path();
    let state: Option<DeployState> = state::load(&path)?;
    if let Some(s) = &state {
        if s.stream_id != stream_id {
            return Err(err(
                Stage::State,
                "state_stream_mismatch",
                format!(
                    "{} belongs to stream `{}`, expected `{stream_id}`",
                    path.display(),
                    s.stream_id
                ),
            ));
        }
    }
    Ok(state)
}

fn identity_gate(
    cfg: &Config,
    old_sha256: Option<&str>,
    old_not_after: Option<i64>,
    staged: &Staged,
    facts: &KeyboxFacts,
    now: i64,
    allow_identity_change: bool,
) -> Gate {
    match old_sha256 {
        None => Gate::FirstDeploy,
        Some(old) if old == staged.keybox_sha256 => Gate::Same,
        Some(_) => match cfg.deploy.identity_change_policy {
            IdentityPolicy::Always => Gate::Allowed {
                reason: "policy_always",
            },
            IdentityPolicy::Never => {
                if allow_identity_change {
                    Gate::Allowed {
                        reason: "manual_allow",
                    }
                } else {
                    Gate::Blocked {
                        reason: "identity_change",
                    }
                }
            }
            IdentityPolicy::Scheduled => {
                if allow_identity_change {
                    return Gate::Allowed {
                        reason: "manual_allow",
                    };
                }
                let old_not_after = match old_not_after {
                    Some(t) => t,
                    None => {
                        return Gate::Blocked {
                            reason: "identity_change",
                        }
                    }
                };
                let lead = i64::from(cfg.deploy.rotation_lead_days) * DAY;
                let inside_window = old_not_after.saturating_sub(now) <= lead;
                if inside_window && facts.leaf_not_after > old_not_after {
                    Gate::Allowed {
                        reason: "rotation_window",
                    }
                } else {
                    Gate::Blocked {
                        reason: "identity_change",
                    }
                }
            }
        },
    }
}

fn plan_warnings(gate: Gate, targets: &[Target]) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let changes_identity = !matches!(gate, Gate::Same | Gate::Blocked { .. });
    if changes_identity {
        if targets.iter().any(|t| t.kind == TargetKind::Omk) {
            warnings.push(Warning {
                code: "omk_key_retirement",
                message: "OhMyKeymint `retire_stale_keybox_bound_keys` (default true) deletes \
                          client keys bound to the previous keybox after a keybox change; \
                          affected apps must provision new keys."
                    .to_string(),
            });
        }
        if targets.iter().any(|t| t.kind == TargetKind::Teesim) {
            warnings.push(Warning {
                code: "teesim_reattest",
                message: "TEESimulator re-attests existing client keys with the new keybox; \
                          attestations issued with the previous keybox may stop verifying."
                    .to_string(),
            });
        }
    }
    warnings.push(Warning {
        code: "no_root_trust_check",
        message: "keybox certificates were checked structurally only; signatures, root trust \
                  and revocation were not verified."
            .to_string(),
    });
    warnings
}

/// Build the identity-gated deployment plan without writing anything.
pub fn plan(cfg: &Config, now: i64, allow_identity_change: bool) -> Result<Plan> {
    let targets = resolve_targets(cfg)?;
    let stream_id = cfg.require_stream()?.to_string();
    let state = load_deploy_state(cfg, &stream_id)?;
    let (staged, staged_path, bytes) = load_staged(&cfg.staging_dir())?;
    if staged.stream_id != stream_id {
        return Err(err(
            Stage::State,
            "staging_stream_mismatch",
            format!(
                "staged keybox belongs to stream `{}`, expected `{stream_id}`",
                staged.stream_id
            ),
        ));
    }
    let facts = verify_staged(&staged, &bytes, &staged_path)?;
    let old_sha256 = state
        .as_ref()
        .and_then(|s| s.deployed_keybox_sha256.clone());
    let old_not_after = state.as_ref().and_then(|s| s.deployed_leaf_not_after);
    let gate = identity_gate(
        cfg,
        old_sha256.as_deref(),
        old_not_after,
        &staged,
        &facts,
        now,
        allow_identity_change,
    );
    let warnings = plan_warnings(gate, &targets);
    Ok(Plan {
        staged,
        staged_path,
        facts,
        gate,
        old_sha256,
        old_not_after,
        warnings,
        targets,
    })
}

// ---------------------------------------------------------------------------
// Report types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TargetError {
    pub code: String,
    pub message: String,
}

impl TargetError {
    fn new(code: &str, message: impl Into<String>) -> TargetError {
        TargetError {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetReport {
    pub id: String,
    pub path: PathBuf,
    pub ok: bool,
    pub sha256: Option<String>,
    pub reload: String,
    pub reload_status: Option<String>,
    pub error: Option<TargetError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub keybox_sha256: String,
    pub seq: u64,
    pub deployed: bool,
    pub pending: Option<Pending>,
    pub warnings: Vec<Warning>,
    pub targets: Vec<TargetReport>,
}

/// Runtime switches for [`apply`] (plus test hooks for device paths).
#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub now: i64,
    pub allow_identity_change: bool,
    pub confirm_first: bool,
    /// TEESimulator `config.json` (read only).
    pub teesim_config: PathBuf,
    /// OhMyKeymint restart trigger file.
    pub omk_restart_flag: PathBuf,
}

impl ApplyOptions {
    pub fn new(now: i64) -> ApplyOptions {
        ApplyOptions {
            now,
            allow_identity_change: false,
            confirm_first: false,
            teesim_config: PathBuf::from(DEFAULT_TEESIM_CONFIG),
            omk_restart_flag: PathBuf::from(DEFAULT_OMK_RESTART),
        }
    }
}

// ---------------------------------------------------------------------------
// Atomic target write
// ---------------------------------------------------------------------------

/// Internal failure with a stable code; converted to [`TargetError`].
#[derive(Debug)]
struct OpsError {
    code: &'static str,
    message: String,
}

impl OpsError {
    fn new(code: &'static str, message: impl Into<String>) -> OpsError {
        OpsError {
            code,
            message: message.into(),
        }
    }
}

fn cstring(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

/// Read the SELinux label of `path` (following no symlinks). Any failure —
/// missing file, unsupported filesystem, unavailable attribute — means "none".
fn read_selinux_label(path: &Path) -> Option<Vec<u8>> {
    let path = cstring(path).ok()?;
    let name = CString::new(SELINUX_XATTR).ok()?;
    // SAFETY: both pointers are valid NUL-terminated strings; a NULL buffer with
    // length 0 is the documented size query for lgetxattr.
    let size = unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
    if size <= 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` has the exact capacity reported by the size query.
    let n = unsafe {
        libc::lgetxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr().cast::<libc::c_void>(),
            buf.len(),
        )
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

/// Best-effort copy of a SELinux label onto the temp file. Failures (EPERM,
/// ENOTSUP, ENODATA, …) are ignored: the content write is already the goal and
/// a label mismatch is reported by the tool's own watcher, not by deploy.
fn set_selinux_label(file: &File, label: &[u8]) {
    if label.is_empty() {
        return;
    }
    let Ok(name) = CString::new(SELINUX_XATTR) else {
        return;
    };
    // SAFETY: `name` and `label` are valid for their lengths and the fd is open.
    let _ = unsafe {
        libc::fsetxattr(
            file.as_raw_fd(),
            name.as_ptr(),
            label.as_ptr().cast::<libc::c_void>(),
            label.len(),
            0,
        )
    };
}

/// Write the temp file and rename it over `path` (design §11.6). The caller
/// removes the temp file on error.
fn write_temp_and_rename(
    tmp: &Path,
    path: &Path,
    bytes: &[u8],
    mode: u32,
    uid: u32,
    gid: u32,
    label: Option<&[u8]>,
) -> std::result::Result<(), OpsError> {
    let map = |code: &'static str, what: &str, e: io::Error| {
        OpsError::new(code, format!("{what} {}: {e}", tmp.display()))
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp)
        .map_err(|e| map("target_write", "create", e))?;
    file.write_all(bytes)
        .map_err(|e| map("target_write", "write", e))?;
    file.sync_all()
        .map_err(|e| map("target_write", "fsync", e))?;
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(|e| map("target_chmod", "chmod", e))?;
    fchown(&file, Some(uid), Some(gid)).map_err(|e| {
        OpsError::new(
            "target_chown",
            format!("chown {uid}:{gid} {}: {e}", tmp.display()),
        )
    })?;
    if let Some(label) = label {
        set_selinux_label(&file, label);
    }
    drop(file);
    fs::rename(tmp, path).map_err(|e| map("target_rename", "rename to", e))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    sync_dir(dir).map_err(|e| OpsError::new("target_sync", format!("fsync {}: {e}", dir.display())))
}

fn write_target(target: &Target, bytes: &[u8], sha: &str) -> std::result::Result<(), TargetError> {
    let fail = |code: &'static str, msg: String| TargetError::new(code, msg);
    let path = &target.path;
    let dir = path.parent().ok_or_else(|| {
        fail(
            "target_path_invalid",
            format!("target path {} has no parent", path.display()),
        )
    })?;
    match fs::metadata(dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(fail(
                "target_dir_missing",
                format!("{} is not a directory", dir.display()),
            ))
        }
        Err(e) => {
            return Err(fail(
                "target_dir_missing",
                format!("target directory {} is unusable: {e}", dir.display()),
            ))
        }
    }
    let existing = match fs::symlink_metadata(path) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(fail("target_stat", format!("stat {}: {e}", path.display()))),
    };
    if existing.as_ref().is_some_and(|m| !m.is_file()) {
        return Err(TargetError::new(
            "target_not_regular",
            format!("{} exists and is not a regular file", path.display()),
        ));
    }
    let label = if existing.is_some() {
        read_selinux_label(path)
    } else {
        None
    };
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        fail(
            "target_path_invalid",
            format!("{} has no file name", path.display()),
        )
    })?;
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));
    let result = write_temp_and_rename(
        &tmp,
        path,
        bytes,
        target.mode,
        target.uid,
        target.gid,
        label.as_deref(),
    );
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(match result {
            Err(e) => TargetError::new(e.code, e.message),
            Ok(()) => unreachable!("checked above"),
        });
    }
    let readback = read_limited(path, limits::MAX_KEYBOX_XML).map_err(|e| {
        fail(
            "readback_failed",
            format!("read back {}: {e}", path.display()),
        )
    })?;
    if sha256_hex(&readback) != sha {
        return Err(fail(
            "readback_mismatch",
            format!(
                "{} does not match the staged keybox after write",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn fix_attrs(path: &Path, target: &Target) -> std::result::Result<(), TargetError> {
    fs::set_permissions(path, fs::Permissions::from_mode(target.mode)).map_err(|e| {
        TargetError::new(
            "target_chmod",
            format!("chmod {:04o} {}: {e}", target.mode, path.display()),
        )
    })?;
    std::os::unix::fs::chown(path, Some(target.uid), Some(target.gid)).map_err(|e| {
        TargetError::new(
            "target_chown",
            format!(
                "chown {}:{} {}: {e}",
                target.uid,
                target.gid,
                path.display()
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Reload
// ---------------------------------------------------------------------------

fn touch_path(path: &Path, now: i64) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    drop(file);
    let cpath = cstring(path)?;
    let times = [
        libc::timespec {
            tv_sec: now as libc::time_t,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: now as libc::time_t,
            tv_nsec: 0,
        },
    ];
    // SAFETY: `cpath` is NUL-terminated and `times` points at two valid
    // timespecs; AT_FDCWD plus an absolute path selects the file directly.
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, cpath.as_ptr(), times.as_ptr(), 0) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn reload_target(target: &Target, opts: &ApplyOptions) -> (String, Option<TargetError>) {
    let degraded = |msg: String| {
        (
            "degraded".to_string(),
            Some(TargetError::new("reload_failed", msg)),
        )
    };
    match &target.reload {
        Reload::Auto => ("watcher".to_string(), None),
        Reload::None => ("skipped".to_string(), None),
        Reload::Restart => {
            // A fresh file (atomic rename) is what OhMyKeymint watches for.
            match rkp_mini_core::fsutil::atomic_write(&opts.omk_restart_flag, b"", 0o600, None) {
                Ok(()) => ("ok".to_string(), None),
                Err(e) => degraded(format!(
                    "create {} to restart keymint: {e}",
                    opts.omk_restart_flag.display()
                )),
            }
        }
        Reload::Touch(path) => match touch_path(path, opts.now) {
            Ok(()) => ("ok".to_string(), None),
            Err(e) => degraded(format!("touch {}: {e}", path.display())),
        },
    }
}

// ---------------------------------------------------------------------------
// TEESimulator profile check
// ---------------------------------------------------------------------------

fn collect_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

/// Every string reachable under an object key named `keybox`.
fn collect_keybox_refs(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                if key == "keybox" {
                    collect_strings(item, out);
                }
                collect_keybox_refs(item, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keybox_refs(item, out);
            }
        }
        _ => {}
    }
}

/// Confirm that TEESimulator actually loads the target's basename.
///
/// Returns an optional failure and an optional warning. `config.json` is only
/// read, never modified.
fn teesim_precheck(target: &Target, config: &Path) -> (Option<TargetError>, Option<Warning>) {
    let basename = match target.path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => {
            return (
                Some(TargetError::new(
                    "target_unreferenced",
                    format!(
                        "teesim keybox path {} has no file name",
                        target.path.display()
                    ),
                )),
                None,
            )
        }
    };
    let bytes = match read_limited(config, MAX_TEESIM_CONFIG) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if basename == "keybox.xml" {
                return (
                    None,
                    Some(Warning {
                        code: "teesim_config_missing",
                        message: format!(
                            "{} is missing; assuming TEESimulator's default keybox.xml profile",
                            config.display()
                        ),
                    }),
                );
            }
            return (
                Some(TargetError::new(
                    "target_unreferenced",
                    format!(
                        "{} is missing and the target basename `{basename}` is not the default keybox.xml",
                        config.display()
                    ),
                )),
                None,
            );
        }
        Err(e) => {
            return (
                Some(TargetError::new(
                    "teesim_config_invalid",
                    format!("read {}: {e}", config.display()),
                )),
                None,
            )
        }
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return (
                Some(TargetError::new(
                    "teesim_config_invalid",
                    format!("parse {}: {e}", config.display()),
                )),
                None,
            )
        }
    };
    let mut refs = Vec::new();
    collect_keybox_refs(&value, &mut refs);
    if refs.iter().any(|r| r == basename) {
        (None, None)
    } else {
        (
            Some(TargetError::new(
                "target_unreferenced",
                format!(
                    "no profile in {} references the target basename `{basename}`",
                    config.display()
                ),
            )),
            None,
        )
    }
}

// ---------------------------------------------------------------------------
// Per-target deployment
// ---------------------------------------------------------------------------

fn fail_report(target: &Target, error: TargetError) -> TargetReport {
    TargetReport {
        id: target.id.clone(),
        path: target.path.clone(),
        ok: false,
        sha256: None,
        reload: target.reload.as_string(),
        reload_status: None,
        error: Some(error),
    }
}

fn deploy_one(target: &Target, bytes: &[u8], sha: &str, opts: &ApplyOptions) -> TargetReport {
    if let Err(error) = write_target(target, bytes, sha) {
        return fail_report(target, error);
    }
    let (reload_status, error) = reload_target(target, opts);
    TargetReport {
        id: target.id.clone(),
        path: target.path.clone(),
        ok: true,
        sha256: Some(sha.to_string()),
        reload: target.reload.as_string(),
        reload_status: Some(reload_status),
        error,
    }
}

/// Deploy every target independently. TEESim profile checks run for the whole
/// target list first so warnings are recorded before the first write.
fn deploy_all(
    targets: &[Target],
    bytes: &[u8],
    sha: &str,
    opts: &ApplyOptions,
    warnings: &mut Vec<Warning>,
) -> Vec<TargetReport> {
    let mut prechecked: Vec<Option<TargetReport>> = Vec::with_capacity(targets.len());
    for target in targets {
        let failure = if target.kind == TargetKind::Teesim {
            let (failure, warning) = teesim_precheck(target, &opts.teesim_config);
            if let Some(w) = warning {
                warnings.push(w);
            }
            failure
        } else {
            None
        };
        prechecked.push(failure.map(|error| fail_report(target, error)));
    }
    targets
        .iter()
        .zip(prechecked)
        .map(|(target, failure)| match failure {
            Some(report) => report,
            None => deploy_one(target, bytes, sha, opts),
        })
        .collect()
}

/// Idempotent pass: rewrite targets whose content is missing/different and fix
/// permissions in place otherwise. Reload only fires for rewritten targets.
fn repair_all(
    targets: &[Target],
    bytes: &[u8],
    sha: &str,
    opts: &ApplyOptions,
) -> Vec<TargetReport> {
    targets
        .iter()
        .map(|target| {
            let path = &target.path;
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {
                    let current =
                        read_limited(path, limits::MAX_KEYBOX_XML).map(|b| sha256_hex(&b));
                    match current {
                        Ok(current) if current == sha => {
                            let attrs_ok = meta.mode() & 0o777 == target.mode
                                && meta.uid() == target.uid
                                && meta.gid() == target.gid;
                            if !attrs_ok {
                                if let Err(error) = fix_attrs(path, target) {
                                    return fail_report(target, error);
                                }
                            }
                            TargetReport {
                                id: target.id.clone(),
                                path: target.path.clone(),
                                ok: true,
                                sha256: Some(sha.to_string()),
                                reload: target.reload.as_string(),
                                reload_status: Some("skipped".to_string()),
                                error: None,
                            }
                        }
                        _ => deploy_one(target, bytes, sha, opts),
                    }
                }
                Ok(_) => fail_report(
                    target,
                    TargetError::new(
                        "target_not_regular",
                        format!("{} exists and is not a regular file", path.display()),
                    ),
                ),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    deploy_one(target, bytes, sha, opts)
                }
                Err(e) => fail_report(
                    target,
                    TargetError::new("target_stat", format!("stat {}: {e}", path.display())),
                ),
            }
        })
        .collect()
}

fn update_target_states(
    state: &mut DeployState,
    targets: &[Target],
    reports: &[TargetReport],
    now: i64,
) {
    for (target, report) in targets.iter().zip(reports) {
        let pos = match state.targets.iter().position(|s| s.id == target.id) {
            Some(pos) => pos,
            None => {
                state.targets.push(TargetState {
                    id: target.id.clone(),
                    path: String::new(),
                    owner: String::new(),
                    mode: String::new(),
                    deployed_sha256: None,
                    deployed_at: None,
                    reload: String::new(),
                    reload_status: None,
                    last_error: None,
                });
                state.targets.len() - 1
            }
        };
        let entry = &mut state.targets[pos];
        entry.path = target.path.display().to_string();
        entry.owner = target.owner_string();
        entry.mode = target.mode_string();
        entry.reload = target.reload.as_string();
        entry.reload_status = report.reload_status.clone();
        entry.last_error = report.error.as_ref().map(|e| LastError {
            code: e.code.clone(),
            message: e.message.clone(),
            at: now,
        });
        if report.ok {
            if let Some(sha) = &report.sha256 {
                entry.deployed_sha256 = Some(sha.clone());
            }
            entry.deployed_at = Some(now);
        }
    }
}

/// Deploy the staged keybox according to the identity gate.
///
/// Returns `Ok` with per-target failures recorded even when some targets fail;
/// the CLI turns a `targets[].ok == false` report into exit code 9.
pub fn apply(cfg: &Config, opts: &ApplyOptions) -> Result<Report> {
    let prepared = plan(cfg, opts.now, opts.allow_identity_change)?;
    let stream_id = cfg.require_stream()?.to_string();
    let mut st =
        load_deploy_state(cfg, &stream_id)?.unwrap_or_else(|| DeployState::new(&stream_id));
    st.last_seen_keybox_sha256 = Some(prepared.staged.keybox_sha256.clone());
    st.last_seen_seq = Some(prepared.staged.seq);

    let report = |targets: Vec<TargetReport>,
                  deployed: bool,
                  pending: Option<Pending>,
                  warnings: Vec<Warning>| Report {
        keybox_sha256: prepared.staged.keybox_sha256.clone(),
        seq: prepared.staged.seq,
        deployed,
        pending,
        warnings,
        targets,
    };

    match prepared.gate {
        Gate::FirstDeploy if !opts.confirm_first => {
            let pending = Pending {
                seq: prepared.staged.seq,
                keybox_sha256: prepared.staged.keybox_sha256.clone(),
                leaf_not_after: prepared.facts.leaf_not_after,
                reason: "first_deploy".to_string(),
                at: opts.now,
            };
            st.pending = Some(pending.clone());
            state::save(&cfg.deploy_state_path(), &st)?;
            Ok(report(Vec::new(), false, Some(pending), prepared.warnings))
        }
        Gate::Blocked { .. } => {
            let pending = Pending {
                seq: prepared.staged.seq,
                keybox_sha256: prepared.staged.keybox_sha256.clone(),
                leaf_not_after: prepared.facts.leaf_not_after,
                reason: "identity_change".to_string(),
                at: opts.now,
            };
            st.pending = Some(pending.clone());
            state::save(&cfg.deploy_state_path(), &st)?;
            Ok(report(Vec::new(), false, Some(pending), prepared.warnings))
        }
        gate => {
            let bytes = read_staged_bytes(&prepared.staged_path, &prepared.staged.keybox_sha256)?;
            let mut warnings = prepared.warnings;
            let reports = if gate == Gate::Same {
                repair_all(
                    &prepared.targets,
                    &bytes,
                    &prepared.staged.keybox_sha256,
                    opts,
                )
            } else {
                deploy_all(
                    &prepared.targets,
                    &bytes,
                    &prepared.staged.keybox_sha256,
                    opts,
                    &mut warnings,
                )
            };
            update_target_states(&mut st, &prepared.targets, &reports, opts.now);
            let any_ok = reports.iter().any(|r| r.ok);
            if any_ok {
                st.deployed_keybox_sha256 = Some(prepared.staged.keybox_sha256.clone());
                st.deployed_seq = Some(prepared.staged.seq);
                st.deployed_at = Some(opts.now);
                st.deployed_leaf_not_after = Some(prepared.facts.leaf_not_after);
                st.pending = None;
            }
            let pending = st.pending.clone();
            state::save(&cfg.deploy_state_path(), &st)?;
            Ok(report(reports, any_ok, pending, warnings))
        }
    }
}

/// Remove deployed keybox files for configured targets (all when `ids` is
/// empty). A file is only deleted when its SHA-256 still matches what this
/// layer deployed; staging and `sync-state.json` are untouched.
pub fn remove(cfg: &Config, ids: &[String]) -> Result<Vec<TargetReport>> {
    let targets = resolve_targets(cfg)?;
    let stream_id = cfg.require_stream()?.to_string();
    let mut st = load_deploy_state(cfg, &stream_id)?;
    let now = unix_now();

    let selected: Vec<&Target> = if ids.is_empty() {
        targets.iter().collect()
    } else {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let target = targets.iter().find(|t| &t.id == id).ok_or_else(|| {
                err(
                    Stage::Config,
                    "unknown_target_id",
                    format!("no configured target `{id}`"),
                )
            })?;
            out.push(target);
        }
        out
    };

    let mut reports = Vec::with_capacity(selected.len());
    for target in selected {
        let recorded = st
            .as_ref()
            .and_then(|s| s.targets.iter().find(|t| t.id == target.id))
            .and_then(|t| t.deployed_sha256.clone());
        let mut report = TargetReport {
            id: target.id.clone(),
            path: target.path.clone(),
            ok: false,
            sha256: None,
            reload: target.reload.as_string(),
            reload_status: Some("skipped".to_string()),
            error: None,
        };
        match fs::symlink_metadata(&target.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => report.ok = true,
            Err(e) => {
                report.error = Some(TargetError::new(
                    "target_stat",
                    format!("stat {}: {e}", target.path.display()),
                ))
            }
            Ok(meta) if !meta.is_file() => {
                report.error = Some(TargetError::new(
                    "target_not_regular",
                    format!("{} is not a regular file", target.path.display()),
                ))
            }
            Ok(_) => match recorded {
                None => {
                    report.error = Some(TargetError::new(
                        "not_deployed",
                        format!(
                            "{} has no recorded deployment; refusing to delete it",
                            target.path.display()
                        ),
                    ))
                }
                Some(sha) => match read_limited(&target.path, limits::MAX_KEYBOX_XML) {
                    Ok(bytes) if sha256_hex(&bytes) == sha => match fs::remove_file(&target.path) {
                        Ok(()) => {
                            report.ok = true;
                            report.sha256 = Some(sha);
                        }
                        Err(e) => {
                            report.error = Some(TargetError::new(
                                "target_delete",
                                format!("remove {}: {e}", target.path.display()),
                            ))
                        }
                    },
                    Ok(_) => {
                        report.error = Some(TargetError::new(
                            "target_modified",
                            format!(
                                "{} changed after deployment; refusing to delete it",
                                target.path.display()
                            ),
                        ))
                    }
                    Err(e) => {
                        report.error = Some(TargetError::new(
                            "target_read",
                            format!("read {}: {e}", target.path.display()),
                        ))
                    }
                },
            },
        }

        if let Some(state) = st.as_mut() {
            if let Some(entry) = state.targets.iter_mut().find(|t| t.id == target.id) {
                if report.ok {
                    entry.deployed_sha256 = None;
                    entry.deployed_at = None;
                    entry.last_error = None;
                } else if let Some(error) = &report.error {
                    entry.last_error = Some(LastError {
                        code: error.code.clone(),
                        message: error.message.clone(),
                        at: now,
                    });
                }
            }
        }
        reports.push(report);
    }

    if let Some(state) = st.as_mut() {
        if state.targets.iter().all(|t| t.deployed_sha256.is_none()) {
            state.deployed_keybox_sha256 = None;
            state.deployed_seq = None;
            state.deployed_at = None;
            state.deployed_leaf_not_after = None;
        }
    }
    if st.is_some() {
        state::save(&cfg.deploy_state_path(), &st)?;
    }
    Ok(reports)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    use base64::Engine;
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};
    use p256::SecretKey;
    use rand_core::OsRng;
    use rcgen::{
        BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
        PKCS_ECDSA_P256_SHA256,
    };
    use rkp_mini_core::locator::UrlPolicy;
    use rkp_mini_core::state::STATE_VERSION;
    use time::{Duration, OffsetDateTime};

    use rkp_mini_runtime::config::{DeploySection, Role, SyncSection, TargetConfig};

    // -- fixtures ----------------------------------------------------------

    fn now() -> i64 {
        OffsetDateTime::now_utc().unix_timestamp()
    }

    fn uid() -> u32 {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    fn gid() -> u32 {
        // SAFETY: getgid has no preconditions and cannot fail.
        unsafe { libc::getgid() }
    }

    fn pem(label: &str, der: &[u8]) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let mut body = String::new();
        for chunk in b64.as_bytes().chunks(64) {
            body.push_str(std::str::from_utf8(chunk).unwrap());
            body.push('\n');
        }
        format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
    }

    fn keybox_xml(key_pem: &str, chain_pems: &[String], device_id: &str) -> String {
        let mut certs = String::new();
        for p in chain_pems {
            certs.push_str("                <Certificate format=\"pem\">\n");
            certs.push_str(p);
            certs.push_str("                </Certificate>\n");
        }
        format!(
            "<?xml version=\"1.0\"?>\n\
<AndroidAttestation>\n\
    <NumberOfKeyboxes>1</NumberOfKeyboxes>\n\
    <Keybox DeviceID=\"{device_id}\">\n\
        <Key algorithm=\"ecdsa\">\n\
            <PrivateKey format=\"pem\">\n\
{key_pem}\
            </PrivateKey>\n\
            <CertificateChain>\n\
                <NumberOfCertificates>{n}</NumberOfCertificates>\n\
{certs}\
            </CertificateChain>\n\
        </Key>\n\
    </Keybox>\n\
</AndroidAttestation>\n",
            n = chain_pems.len(),
        )
    }

    /// Minimal copy of `rkp-sync-core`'s test fixture generator.
    fn make_keybox(leaf_days: i64) -> String {
        let now = OffsetDateTime::now_utc();
        let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "RKP Sync Deploy Test Root");
        ca_params.distinguished_name = dn;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.not_before = now - Duration::days(1);
        ca_params.not_after = now + Duration::days(3650);
        let ca = ca_params.self_signed(&ca_key).unwrap();

        let secret = SecretKey::random(&mut OsRng);
        let leaf_key = KeyPair::try_from(secret.to_pkcs8_der().unwrap().as_bytes()).unwrap();
        let mut leaf_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "RKP Sync Deploy Test Leaf");
        leaf_params.distinguished_name = dn;
        leaf_params.not_before = now - Duration::days(1);
        leaf_params.not_after = now + Duration::days(leaf_days);
        let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).unwrap();

        let chain_pems = vec![
            pem("CERTIFICATE", leaf.der().as_ref()),
            pem("CERTIFICATE", ca.der().as_ref()),
        ];
        let sec1 = secret.public_key().to_encoded_point(false);
        let device_id = format!("rkp-mini-{}", &sha256_hex(sec1.as_bytes())[..12]);
        let key_pem = secret.to_sec1_pem(LineEnding::LF).unwrap().to_string();
        keybox_xml(&key_pem, &chain_pems, &device_id)
    }

    const STREAM: &str = "st_deploytest01";

    fn test_cfg(root: &Path) -> Config {
        let sync = SyncSection {
            stream_id: STREAM.to_string(),
            ..SyncSection::default()
        };
        Config {
            path: root.join("config.toml"),
            root: root.to_path_buf(),
            role: Role::Receiver,
            sync,
            deploy: DeploySection::default(),
            output_dir: root.join("out"),
            auto_issue_days: 0,
            url_policy: UrlPolicy::default(),
        }
    }

    fn custom(id: &str, path: &Path, reload: &str) -> TargetConfig {
        TargetConfig {
            id: id.to_string(),
            path: Some(path.display().to_string()),
            owner: Some(format!("{}:{}", uid(), gid())),
            mode: Some("0600".to_string()),
            reload: Some(reload.to_string()),
        }
    }

    fn stage_keybox(cfg: &Config, leaf_days: i64, seq: u64) -> (String, String, KeyboxFacts) {
        let xml = make_keybox(leaf_days);
        let facts = keybox::validate_structure(xml.as_bytes()).unwrap();
        let sha = sha256_hex(xml.as_bytes());
        let staged = Staged {
            v: STATE_VERSION,
            stream_id: cfg.sync.stream_id.clone(),
            seq,
            keybox_sha256: sha.clone(),
            plaintext_sha256: sha.clone(),
            device_id: facts.device_id.clone(),
            leaf_not_before: facts.leaf_not_before,
            leaf_not_after: facts.leaf_not_after,
            staged_at: facts.leaf_not_before,
        };
        state::write_staging(&cfg.staging_dir(), &staged, xml.as_bytes()).unwrap();
        (xml, sha, facts)
    }

    fn save_deployed(cfg: &Config, sha: &str, seq: u64, leaf_not_after: Option<i64>) {
        let mut st = DeployState::new(&cfg.sync.stream_id);
        st.last_seen_keybox_sha256 = Some(sha.to_string());
        st.last_seen_seq = Some(seq);
        st.deployed_keybox_sha256 = Some(sha.to_string());
        st.deployed_seq = Some(seq);
        st.deployed_at = Some(now());
        st.deployed_leaf_not_after = leaf_not_after;
        state::save(&cfg.deploy_state_path(), &st).unwrap();
    }

    fn opts_for(dir: &Path) -> ApplyOptions {
        let mut opts = ApplyOptions::new(now());
        opts.teesim_config = dir.join("teesim").join("config.json");
        opts.omk_restart_flag = dir.join("omk").join("restart.keymint");
        opts
    }

    fn code_of<T: std::fmt::Debug>(result: Result<T>) -> String {
        to_sync_error(&result.unwrap_err()).code
    }

    fn fail_setup(dir: &Path) -> (PathBuf, PathBuf, Config) {
        let root = dir.join("root");
        fs::create_dir_all(&root).unwrap();
        let targets = dir.join("targets");
        fs::create_dir_all(&targets).unwrap();
        (root.clone(), targets, test_cfg(&root))
    }

    // -- staging -----------------------------------------------------------

    #[test]
    fn missing_or_corrupt_staging_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let (root, targets, cfg) = fail_setup(dir.path());
        let mut cfg_mut = cfg.clone();
        cfg_mut.deploy.targets = vec![custom("one", &targets.join("keybox.xml"), "none")];
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        let _ = root;

        assert_eq!(code_of(apply(&cfg_mut, &opts)), "nothing_staged");

        let (_, sha, _) = stage_keybox(&cfg_mut, 300, 1);
        let staged_path = cfg_mut.staging_dir().join(format!("{}.xml", sha));
        fs::write(&staged_path, b"not a keybox").unwrap();
        assert_eq!(code_of(apply(&cfg_mut, &opts)), "staging_corrupt");
        assert!(!targets.join("keybox.xml").exists());
    }

    // -- first deploy ------------------------------------------------------

    #[test]
    fn first_deploy_needs_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        let (xml, sha, _) = stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());

        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.deployed);
        assert_eq!(
            report.pending.as_ref().unwrap().reason.as_str(),
            "first_deploy"
        );
        assert!(report.targets.is_empty());
        assert!(!target_path.exists());
        assert!(report
            .warnings
            .iter()
            .any(|w| w.code == "no_root_trust_check"));

        opts.confirm_first = true;
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.deployed);
        assert_eq!(report.keybox_sha256, sha);
        assert_eq!(report.seq, 1);
        assert_eq!(report.targets.len(), 1);
        assert!(report.targets[0].ok);
        assert!(report.pending.is_none());
        assert_eq!(fs::read(&target_path).unwrap(), xml.as_bytes());

        let meta = fs::metadata(&target_path).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o600);
        assert_eq!(meta.uid(), uid());
        assert_eq!(meta.gid(), gid());

        let state = state::load::<DeployState>(&cfg.deploy_state_path())
            .unwrap()
            .unwrap();
        assert_eq!(state.deployed_keybox_sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(state.deployed_seq, Some(1));
        assert_eq!(state.targets.len(), 1);
        assert_eq!(state.targets[0].owner, format!("{}:{}", uid(), gid()));
    }

    #[test]
    fn report_json_matches_contract() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        cfg.deploy.targets = vec![custom("custom", &targets.join("keybox.xml"), "none")];
        stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        let report = apply(&cfg, &opts).unwrap();
        let json = serde_json::to_value(&report).unwrap();
        for key in [
            "keybox_sha256",
            "seq",
            "deployed",
            "pending",
            "warnings",
            "targets",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        let target = &json["targets"][0];
        for key in [
            "id",
            "path",
            "ok",
            "sha256",
            "reload",
            "reload_status",
            "error",
        ] {
            assert!(target.get(key).is_some(), "missing target.{key}");
        }
        assert_eq!(target["reload"], "none");
        assert_eq!(target["reload_status"], "skipped");
    }

    // -- idempotent repair -------------------------------------------------

    #[test]
    fn same_keybox_repairs_tampered_and_missing_targets() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        let reload_flag = targets.join("reload.flag");
        let reload = format!("touch:{}", reload_flag.display());
        cfg.deploy.targets = vec![custom("custom", &target_path, &reload)];
        let (xml, sha, facts) = stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        apply(&cfg, &opts).unwrap();
        assert_eq!(fs::read(&target_path).unwrap(), xml.as_bytes());
        assert!(reload_flag.exists());

        // Unchanged content: no rewrite (same inode) and no reload.
        let inode = fs::metadata(&target_path).unwrap().ino();
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.deployed);
        assert!(report.targets[0].ok);
        assert_eq!(report.targets[0].reload_status.as_deref(), Some("skipped"));
        assert_eq!(fs::metadata(&target_path).unwrap().ino(), inode);

        // Tampered content is rewritten and reloaded.
        fs::write(&target_path, b"tampered").unwrap();
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.targets[0].ok);
        assert_eq!(report.targets[0].reload_status.as_deref(), Some("ok"));
        assert_eq!(fs::read(&target_path).unwrap(), xml.as_bytes());
        assert_ne!(fs::metadata(&target_path).unwrap().ino(), inode);

        // Missing target is restored.
        fs::remove_file(&target_path).unwrap();
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.deployed);
        assert_eq!(fs::read(&target_path).unwrap(), xml.as_bytes());

        let state = state::load::<DeployState>(&cfg.deploy_state_path())
            .unwrap()
            .unwrap();
        assert_eq!(state.deployed_keybox_sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(state.deployed_leaf_not_after, Some(facts.leaf_not_after));
    }

    #[test]
    fn same_keybox_repairs_permissions_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        let reload_flag = targets.join("reload.flag");
        let reload = format!("touch:{}", reload_flag.display());
        cfg.deploy.targets = vec![custom("custom", &target_path, &reload)];
        let (_, _, _) = stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        apply(&cfg, &opts).unwrap();

        fs::set_permissions(&target_path, fs::Permissions::from_mode(0o644)).unwrap();
        let inode = fs::metadata(&target_path).unwrap().ino();
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.targets[0].ok);
        assert_eq!(report.targets[0].reload_status.as_deref(), Some("skipped"));
        assert_eq!(fs::metadata(&target_path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(&target_path).unwrap().ino(), inode);
    }

    // -- identity gate -----------------------------------------------------

    #[test]
    fn scheduled_policy_blocks_far_from_expiry_and_allows_inside_window() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        cfg.deploy.identity_change_policy = IdentityPolicy::Scheduled;
        cfg.deploy.rotation_lead_days = 30;

        let now = now();
        let old_xml = make_keybox(100);
        let old_sha = sha256_hex(old_xml.as_bytes());
        save_deployed(&cfg, &old_sha, 1, Some(now + 100 * DAY));
        fs::write(&target_path, old_xml.as_bytes()).unwrap();
        let (new_xml, _, _) = stage_keybox(&cfg, 300, 2);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;

        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.deployed);
        assert_eq!(
            report.pending.as_ref().unwrap().reason.as_str(),
            "identity_change"
        );
        assert_eq!(fs::read(&target_path).unwrap(), old_xml.as_bytes());

        // Old keybox now inside the rotation window: deploy.
        save_deployed(&cfg, &old_sha, 1, Some(now + 10 * DAY));
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.deployed, "expected rotation window to allow deploy");
        assert_eq!(fs::read(&target_path).unwrap(), new_xml.as_bytes());
    }

    #[test]
    fn scheduled_policy_requires_new_leaf_to_outlive_old() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        cfg.deploy.identity_change_policy = IdentityPolicy::Scheduled;
        cfg.deploy.rotation_lead_days = 30;

        let now = now();
        let old_sha = sha256_hex(make_keybox(100).as_bytes());
        save_deployed(&cfg, &old_sha, 1, Some(now + 10 * DAY));
        stage_keybox(&cfg, 5, 2);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;

        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.deployed);
        assert!(!target_path.exists());
    }

    #[test]
    fn never_policy_needs_allow_flag_and_always_policy_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        cfg.deploy.identity_change_policy = IdentityPolicy::Never;

        let old_sha = sha256_hex(b"unrelated-old-keybox");
        save_deployed(&cfg, &old_sha, 1, Some(now() + 10 * DAY));
        stage_keybox(&cfg, 300, 2);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;

        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.deployed);
        assert!(!target_path.exists());

        opts.allow_identity_change = true;
        let report = apply(&cfg, &opts).unwrap();
        assert!(report.deployed);
        assert!(target_path.exists());

        // A fresh setup for the `always` policy.
        let dir2 = tempfile::tempdir().unwrap();
        let (_, targets2, mut cfg2) = fail_setup(dir2.path());
        let target2 = targets2.join("keybox.xml");
        cfg2.deploy.targets = vec![custom("custom", &target2, "none")];
        cfg2.deploy.identity_change_policy = IdentityPolicy::Always;
        let old_sha2 = sha256_hex(b"another-old-keybox");
        save_deployed(&cfg2, &old_sha2, 1, Some(now() + 200 * DAY));
        stage_keybox(&cfg2, 300, 2);
        let mut opts2 = opts_for(dir2.path());
        opts2.confirm_first = true;
        let report = apply(&cfg2, &opts2).unwrap();
        assert!(report.deployed);
        assert!(target2.exists());
    }

    #[test]
    fn first_deploy_is_not_skipped_by_always_policy() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        cfg.deploy.identity_change_policy = IdentityPolicy::Always;
        stage_keybox(&cfg, 300, 1);
        let opts = opts_for(dir.path());
        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.deployed);
        assert!(!target_path.exists());
    }

    // -- target failures ---------------------------------------------------

    #[test]
    fn failed_target_keeps_old_content_and_other_targets_continue() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let good = targets.join("good.xml");
        let linked = targets.join("linked.xml");
        let real = targets.join("real.xml");
        fs::write(&real, b"old content").unwrap();
        std::os::unix::fs::symlink(&real, &linked).unwrap();

        cfg.deploy.targets = vec![
            custom("linked", &linked, "none"),
            custom("good", &good, "none"),
        ];
        let (xml, _, _) = stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;

        let report = apply(&cfg, &opts).unwrap();
        assert_eq!(report.targets.len(), 2);
        assert!(!report.targets[0].ok);
        assert_eq!(
            report.targets[0].error.as_ref().unwrap().code.as_str(),
            "target_not_regular"
        );
        assert!(report.targets[1].ok);
        assert_eq!(fs::read(&real).unwrap(), b"old content");
        assert_eq!(fs::read(&good).unwrap(), xml.as_bytes());

        let state = state::load::<DeployState>(&cfg.deploy_state_path())
            .unwrap()
            .unwrap();
        let linked_state = state.targets.iter().find(|t| t.id == "linked").unwrap();
        assert!(linked_state.deployed_sha256.is_none());
        assert_eq!(
            linked_state.last_error.as_ref().unwrap().code.as_str(),
            "target_not_regular"
        );
    }

    #[test]
    fn chown_failure_leaves_old_content() {
        if uid() == 0 {
            return; // root would be allowed to change the owner
        }
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        fs::write(&target_path, b"old content").unwrap();
        let mut target = custom("custom", &target_path, "none");
        target.owner = Some(format!("{}:{}", uid() + 424242, gid()));
        cfg.deploy.targets = vec![target];
        let (_, sha, _) = stage_keybox(&cfg, 300, 1);
        let opts = opts_for(dir.path());

        let prepared = plan(&cfg, opts.now, false).unwrap();
        let report = deploy_one(&prepared.targets[0], b"new", &sha, &opts);
        assert!(!report.ok);
        assert_eq!(report.error.as_ref().unwrap().code.as_str(), "target_chown");
        assert_eq!(fs::read(&target_path).unwrap(), b"old content");
        // No temp file is left behind.
        let leftovers: Vec<_> = fs::read_dir(&targets)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn missing_target_directory_fails_only_that_target() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let missing = targets.join("absent").join("keybox.xml");
        let good = targets.join("good.xml");
        cfg.deploy.targets = vec![
            custom("absent", &missing, "none"),
            custom("good", &good, "none"),
        ];
        stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        let report = apply(&cfg, &opts).unwrap();
        assert!(!report.targets[0].ok);
        assert_eq!(
            report.targets[0].error.as_ref().unwrap().code.as_str(),
            "target_dir_missing"
        );
        assert!(report.targets[1].ok);
        assert!(!missing.exists());
        assert!(good.exists());
    }

    // -- TEESim profile check ---------------------------------------------

    #[test]
    fn teesim_precheck_requires_profile_reference() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let target = Target {
            id: "teesim".to_string(),
            kind: TargetKind::Teesim,
            path: dir.path().join("keybox.xml"),
            uid: uid(),
            gid: gid(),
            mode: 0o600,
            reload: Reload::None,
        };

        fs::write(
            &config,
            br#"{"profiles":{"default":{"keybox":"other.xml"}}}"#,
        )
        .unwrap();
        let (failure, warning) = teesim_precheck(&target, &config);
        assert!(warning.is_none());
        assert_eq!(failure.unwrap().code.as_str(), "target_unreferenced");

        fs::write(
            &config,
            br#"{"profiles":[{"name":"a","keybox":"keybox.xml","flags":[]}]}"#,
        )
        .unwrap();
        let (failure, warning) = teesim_precheck(&target, &config);
        assert!(failure.is_none());
        assert!(warning.is_none());

        fs::remove_file(&config).unwrap();
        let (failure, warning) = teesim_precheck(&target, &config);
        assert!(failure.is_none());
        assert_eq!(warning.unwrap().code, "teesim_config_missing");

        let mut other = target.clone();
        other.path = dir.path().join("vendor.xml");
        let (failure, warning) = teesim_precheck(&other, &config);
        assert!(warning.is_none());
        assert_eq!(failure.unwrap().code.as_str(), "target_unreferenced");
    }

    #[test]
    fn teesim_unreferenced_target_fails_while_others_continue() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, cfg) = fail_setup(dir.path());
        let (xml, sha, _) = stage_keybox(&cfg, 300, 1);
        let config = dir.path().join("teesim").join("config.json");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(
            &config,
            br#"{"profiles":{"default":{"keybox":"vendor.xml"}}}"#,
        )
        .unwrap();
        let opts = opts_for(dir.path());

        let teesim = Target {
            id: "teesim".to_string(),
            kind: TargetKind::Teesim,
            path: targets.join("keybox.xml"),
            uid: uid(),
            gid: gid(),
            mode: 0o600,
            reload: Reload::None,
        };
        let custom_target = Target {
            id: "custom".to_string(),
            kind: TargetKind::Custom,
            path: targets.join("custom.xml"),
            uid: uid(),
            gid: gid(),
            mode: 0o600,
            reload: Reload::None,
        };
        let mut warnings = Vec::new();
        let reports = deploy_all(
            &[teesim, custom_target],
            xml.as_bytes(),
            &sha,
            &opts,
            &mut warnings,
        );
        assert_eq!(reports.len(), 2);
        assert!(!reports[0].ok);
        assert_eq!(
            reports[0].error.as_ref().unwrap().code.as_str(),
            "target_unreferenced"
        );
        assert!(reports[1].ok);
        assert!(!targets.join("keybox.xml").exists());
        assert!(targets.join("custom.xml").exists());
    }

    // -- reload ------------------------------------------------------------

    #[test]
    fn reload_modes_report_status() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, _) = fail_setup(dir.path());
        fs::create_dir_all(dir.path().join("omk")).unwrap();
        let touch = targets.join("touch.flag");
        let opts = opts_for(dir.path());
        let bytes = b"content";

        let restart = Target {
            id: "omk".to_string(),
            kind: TargetKind::Omk,
            path: targets.join("omk.xml"),
            uid: uid(),
            gid: gid(),
            mode: 0o600,
            reload: Reload::Restart,
        };
        let (status, error) = reload_target(&restart, &opts);
        assert_eq!(status, "ok");
        assert!(error.is_none());
        assert!(opts.omk_restart_flag.exists());

        let none = Target {
            reload: Reload::None,
            ..restart.clone()
        };
        assert_eq!(reload_target(&none, &opts).0, "skipped");

        let auto = Target {
            reload: Reload::Auto,
            ..restart.clone()
        };
        assert_eq!(reload_target(&auto, &opts).0, "watcher");

        let touched = Target {
            reload: Reload::Touch(touch.clone()),
            ..restart.clone()
        };
        let (status, error) = reload_target(&touched, &opts);
        assert_eq!(status, "ok");
        assert!(error.is_none());
        assert!(touch.exists());

        // Reload failure is degraded, not a target failure.
        let degraded = Target {
            reload: Reload::Restart,
            ..restart.clone()
        };
        let bad_opts = ApplyOptions {
            omk_restart_flag: dir.path().join("no-such-dir").join("flag"),
            ..opts.clone()
        };
        let report = deploy_one(
            &Target {
                path: targets.join("reload-target.xml"),
                ..degraded
            },
            bytes,
            &sha256_hex(bytes),
            &bad_opts,
        );
        assert!(report.ok);
        assert_eq!(report.reload_status.as_deref(), Some("degraded"));
        assert_eq!(
            report.error.as_ref().unwrap().code.as_str(),
            "reload_failed"
        );
    }

    // -- remove ------------------------------------------------------------

    #[test]
    fn remove_deletes_only_files_we_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        let (xml, sha, _) = stage_keybox(&cfg, 300, 1);
        let mut opts = opts_for(dir.path());
        opts.confirm_first = true;
        apply(&cfg, &opts).unwrap();

        // Same content but not ours to delete after an external edit.
        fs::write(&target_path, b"someone else").unwrap();
        let reports = remove(&cfg, &[]).unwrap();
        assert!(!reports[0].ok);
        assert_eq!(
            reports[0].error.as_ref().unwrap().code.as_str(),
            "target_modified"
        );
        assert!(target_path.exists());
        let state = state::load::<DeployState>(&cfg.deploy_state_path())
            .unwrap()
            .unwrap();
        assert_eq!(state.deployed_keybox_sha256.as_deref(), Some(sha.as_str()));

        // Restore the recorded content and remove it.
        fs::write(&target_path, xml.as_bytes()).unwrap();
        let reports = remove(&cfg, &["custom".to_string()]).unwrap();
        assert!(reports[0].ok);
        assert!(!target_path.exists());
        let state = state::load::<DeployState>(&cfg.deploy_state_path())
            .unwrap()
            .unwrap();
        assert!(state.deployed_keybox_sha256.is_none());
        assert!(state.deployed_seq.is_none());
        assert!(state.targets.iter().all(|t| t.deployed_sha256.is_none()));

        // Unknown id.
        assert_eq!(
            code_of(remove(&cfg, &["nope".to_string()])),
            "unknown_target_id"
        );
    }

    #[test]
    fn remove_refuses_undeployed_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        fs::write(&target_path, b"not ours").unwrap();
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        let reports = remove(&cfg, &[]).unwrap();
        assert!(!reports[0].ok);
        assert_eq!(
            reports[0].error.as_ref().unwrap().code.as_str(),
            "not_deployed"
        );
        assert!(target_path.exists());
    }

    #[test]
    fn remove_tolerates_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let (_, targets, mut cfg) = fail_setup(dir.path());
        let target_path = targets.join("keybox.xml");
        cfg.deploy.targets = vec![custom("custom", &target_path, "none")];
        let reports = remove(&cfg, &[]).unwrap();
        assert!(reports[0].ok);
        assert!(!target_path.exists());
    }
}
