//! Local deployment targets: parsing and validation only.
//!
//! Paths, owners, modes and reload hooks come **only** from local
//! configuration; the remote side can never select a target. Nothing here
//! reads or writes a target file or runs a reload action — that is the
//! deployer job.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};

use crate::config::{Config, TargetConfig};

/// OhMyKeymint active keybox (fixed path; owner is the keystore uid).
pub const OMK_PATH: &str = "/data/misc/keystore/omk/keybox.xml";
/// TEESimulator data directory; the keybox must live directly inside it.
pub const TEESIM_DIR: &str = "/data/adb/teesim";
/// TEESimulator default keybox.
pub const TEESIM_PATH: &str = "/data/adb/teesim/keybox.xml";
/// Legacy TrickyStore keybox path.
pub const TRICKY_STORE_PATH: &str = "/data/adb/tricky_store/keybox.xml";
/// TEESimulator profile file (read only, never modified).
pub const DEFAULT_TEESIM_CONFIG: &str = "/data/adb/teesim/config.json";
/// OhMyKeymint "restart keymint" trigger file.
pub const DEFAULT_OMK_RESTART: &str = "/data/adb/omk/restart.keymint";

// ---------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Omk,
    Teesim,
    TrickyStore,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reload {
    /// Rely on the tool's own file watcher.
    Auto,
    /// OhMyKeymint only: `touch /data/adb/omk/restart.keymint`.
    Restart,
    /// No reload.
    None,
    /// Custom only: touch this path.
    Touch(PathBuf),
}

impl Reload {
    /// Stable string for state files and `--json` output.
    pub fn as_string(&self) -> String {
        match self {
            Reload::Auto => "auto".to_string(),
            Reload::Restart => "restart".to_string(),
            Reload::None => "none".to_string(),
            Reload::Touch(p) => format!("touch:{}", p.display()),
        }
    }
}

/// A fully resolved deployment target.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub kind: TargetKind,
    pub path: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub reload: Reload,
}

impl Target {
    pub fn owner_string(&self) -> String {
        format!("{}:{}", self.uid, self.gid)
    }

    pub fn mode_string(&self) -> String {
        format!("{:04o}", self.mode)
    }
}

fn config_err(code: &'static str, msg: impl Into<String>) -> anyhow::Error {
    err(Stage::Config, code, msg)
}

/// Absolute path with no empty, `.` or `..` segment.
fn check_path(raw: &str, what: &str) -> Result<PathBuf> {
    let invalid = || {
        config_err(
            "invalid_target_path",
            format!("{what} `{raw}` must be an absolute path without empty, `.` or `..` segments"),
        )
    };
    let mut parts = raw.split('/');
    if parts.next() != Some("") {
        return Err(invalid());
    }
    let mut depth = 0usize;
    for seg in parts {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(invalid());
        }
        depth += 1;
    }
    if depth == 0 {
        return Err(invalid());
    }
    Ok(PathBuf::from(raw))
}

fn parse_owner(raw: &str) -> Result<(u32, u32)> {
    let invalid = || {
        config_err(
            "invalid_target_owner",
            format!("owner `{raw}` must be `uid:gid` with numeric ids"),
        )
    };
    let (u, g) = raw.split_once(':').ok_or_else(invalid)?;
    if u.is_empty() || g.is_empty() || g.contains(':') {
        return Err(invalid());
    }
    let uid = u.parse::<u32>().map_err(|_| invalid())?;
    let gid = g.parse::<u32>().map_err(|_| invalid())?;
    Ok((uid, gid))
}

fn parse_mode(raw: &str) -> Result<u32> {
    let invalid = || {
        config_err(
            "invalid_target_mode",
            format!("mode `{raw}` must be an octal mode inside 0o777 without group/other write"),
        )
    };
    if raw.is_empty() {
        return Err(invalid());
    }
    let mode = u32::from_str_radix(raw, 8).map_err(|_| invalid())?;
    if mode & !0o777 != 0 || mode & 0o022 != 0 {
        return Err(invalid());
    }
    Ok(mode)
}

fn builtin_path(t: &TargetConfig, want: &str) -> Result<PathBuf> {
    match t.path.as_deref() {
        None => Ok(PathBuf::from(want)),
        Some(p) if p == want => Ok(PathBuf::from(want)),
        Some(p) => Err(config_err(
            "builtin_path_override",
            format!("target `{}` path must be `{want}`, not `{p}`", t.id),
        )),
    }
}

fn builtin_owner(t: &TargetConfig, uid: u32, gid: u32) -> Result<(u32, u32)> {
    match t.owner.as_deref() {
        None => Ok((uid, gid)),
        Some(raw) => {
            let parsed = parse_owner(raw)?;
            if parsed != (uid, gid) {
                return Err(config_err(
                    "builtin_override",
                    format!("target `{}` owner must be {uid}:{gid}", t.id),
                ));
            }
            Ok(parsed)
        }
    }
}

fn builtin_mode(t: &TargetConfig, mode: u32) -> Result<u32> {
    match t.mode.as_deref() {
        None => Ok(mode),
        Some(raw) => {
            let parsed = parse_mode(raw)?;
            if parsed != mode {
                return Err(config_err(
                    "builtin_override",
                    format!("target `{}` mode must be {:04o}", t.id, mode),
                ));
            }
            Ok(parsed)
        }
    }
}

fn builtin_reload(t: &TargetConfig, allowed: &[(&str, Reload)]) -> Result<Reload> {
    let raw = t.reload.as_deref().unwrap_or("auto");
    allowed
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, r)| r.clone())
        .ok_or_else(|| {
            let names: Vec<&str> = allowed.iter().map(|(name, _)| *name).collect();
            config_err(
                "invalid_target_reload",
                format!(
                    "target `{}` reload `{raw}` is not supported (allowed: {})",
                    t.id,
                    names.join(", ")
                ),
            )
        })
}

fn resolve_one(t: &TargetConfig) -> Result<Target> {
    if t.id.is_empty() || t.id.contains('/') {
        return Err(config_err(
            "invalid_target_id",
            format!("target id `{}` is empty or contains `/`", t.id),
        ));
    }
    match t.id.as_str() {
        "omk" => {
            let (uid, gid) = builtin_owner(t, 1017, 1017)?;
            Ok(Target {
                id: t.id.clone(),
                kind: TargetKind::Omk,
                path: builtin_path(t, OMK_PATH)?,
                uid,
                gid,
                mode: builtin_mode(t, 0o600)?,
                reload: builtin_reload(t, &[("auto", Reload::Auto), ("restart", Reload::Restart)])?,
            })
        }
        "teesim" => {
            let path = match t.path.as_deref() {
                None => PathBuf::from(TEESIM_PATH),
                Some(raw) => {
                    let p = check_path(raw, "teesim target path")?;
                    if p.parent() != Some(Path::new(TEESIM_DIR)) || p.file_name().is_none() {
                        return Err(config_err(
                            "builtin_path_override",
                            format!(
                                "teesim keybox must be directly inside {TEESIM_DIR}, not `{raw}`"
                            ),
                        ));
                    }
                    p
                }
            };
            let (uid, gid) = builtin_owner(t, 0, 0)?;
            Ok(Target {
                id: t.id.clone(),
                kind: TargetKind::Teesim,
                path,
                uid,
                gid,
                mode: builtin_mode(t, 0o600)?,
                reload: builtin_reload(t, &[("auto", Reload::Auto)])?,
            })
        }
        "tricky_store" => {
            let (uid, gid) = builtin_owner(t, 0, 0)?;
            Ok(Target {
                id: t.id.clone(),
                kind: TargetKind::TrickyStore,
                path: builtin_path(t, TRICKY_STORE_PATH)?,
                uid,
                gid,
                mode: builtin_mode(t, 0o600)?,
                reload: builtin_reload(t, &[("auto", Reload::Auto)])?,
            })
        }
        // Any other id is an explicit local opt-in target.
        id => {
            let raw = t.path.as_deref().ok_or_else(|| {
                config_err(
                    "custom_target_incomplete",
                    format!("custom target `{id}` requires an absolute `path`"),
                )
            })?;
            let path = check_path(raw, "custom target path")?;
            let owner = t.owner.as_deref().ok_or_else(|| {
                config_err(
                    "custom_target_incomplete",
                    format!("custom target `{id}` requires `owner` as `uid:gid`"),
                )
            })?;
            let (uid, gid) = parse_owner(owner)?;
            let mode = t.mode.as_deref().ok_or_else(|| {
                config_err(
                    "custom_target_incomplete",
                    format!("custom target `{id}` requires an octal `mode`"),
                )
            })?;
            let reload = match t.reload.as_deref() {
                None | Some("auto") => Reload::Auto,
                Some("none") => Reload::None,
                Some(spec) => match spec.strip_prefix("touch:") {
                    Some(p) => Reload::Touch(check_path(p, "custom reload touch path")?),
                    None => {
                        return Err(config_err(
                            "invalid_target_reload",
                            format!(
                                "custom target `{id}` reload `{spec}` must be `auto`, `none` or `touch:<abs path>`"
                            ),
                        ))
                    }
                },
            };
            Ok(Target {
                id: id.to_string(),
                kind: TargetKind::Custom,
                path,
                uid,
                gid,
                mode: parse_mode(mode)?,
                reload,
            })
        }
    }
}

/// Resolve and validate every configured target.
///
/// Rejects an empty list (`no_targets`), duplicate ids/paths, invalid paths,
/// owners, modes and reload specs, and attempts to move a builtin target.
pub fn resolve_targets(cfg: &Config) -> Result<Vec<Target>> {
    if cfg.deploy.targets.is_empty() {
        return Err(config_err(
            "no_targets",
            "[deploy].targets is empty; add at least one target (builtin id or custom opt-in)",
        ));
    }
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    let mut out = Vec::with_capacity(cfg.deploy.targets.len());
    for t in &cfg.deploy.targets {
        let target = resolve_one(t)?;
        if !ids.insert(target.id.clone()) {
            return Err(config_err(
                "duplicate_target_id",
                format!("duplicate target id `{}`", target.id),
            ));
        }
        if !paths.insert(target.path.clone()) {
            return Err(config_err(
                "duplicate_target_path",
                format!(
                    "path {} is used by more than one target",
                    target.path.display()
                ),
            ));
        }
        out.push(target);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeploySection, Role, SyncSection, TargetConfig};
    use rkp_mini_core::error::to_sync_error;
    use rkp_mini_core::locator::UrlPolicy;

    fn test_cfg(root: &Path) -> Config {
        Config {
            path: root.join("config.toml"),
            root: root.to_path_buf(),
            role: Role::Receiver,
            sync: SyncSection::default(),
            deploy: DeploySection::default(),
            output_dir: root.join("out"),
            auto_issue_days: 0,
            url_policy: UrlPolicy::default(),
        }
    }

    fn uid() -> u32 {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    fn gid() -> u32 {
        // SAFETY: getgid has no preconditions and cannot fail.
        unsafe { libc::getgid() }
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

    fn code_of<T: std::fmt::Debug>(result: Result<T>) -> String {
        to_sync_error(&result.unwrap_err()).code
    }

    #[test]
    fn builtin_targets_resolve_with_fixed_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg(dir.path());
        cfg.deploy.targets = vec![
            TargetConfig {
                id: "omk".to_string(),
                path: None,
                owner: None,
                mode: None,
                reload: Some("restart".to_string()),
            },
            TargetConfig {
                id: "teesim".to_string(),
                path: Some("/data/adb/teesim/vendor.xml".to_string()),
                owner: None,
                mode: None,
                reload: None,
            },
            TargetConfig {
                id: "tricky_store".to_string(),
                path: None,
                owner: None,
                mode: None,
                reload: None,
            },
        ];
        let targets = resolve_targets(&cfg).unwrap();
        assert_eq!(targets.len(), 3);
        assert_eq!(targets[0].path, PathBuf::from(OMK_PATH));
        assert_eq!((targets[0].uid, targets[0].gid), (1017, 1017));
        assert_eq!(targets[0].reload, Reload::Restart);
        assert_eq!(
            targets[1].path,
            PathBuf::from("/data/adb/teesim/vendor.xml")
        );
        assert_eq!((targets[1].uid, targets[1].gid), (0, 0));
        assert_eq!(targets[2].path, PathBuf::from(TRICKY_STORE_PATH));
        assert_eq!(targets[2].mode, 0o600);
    }

    #[test]
    fn resolve_targets_rejects_invalid_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg(dir.path());

        cfg.deploy.targets = Vec::new();
        assert_eq!(code_of(resolve_targets(&cfg)), "no_targets");

        let path_a = dir.path().join("a.xml");
        cfg.deploy.targets = vec![
            custom("one", &path_a, "none"),
            custom("one", &dir.path().join("b.xml"), "none"),
        ];
        assert_eq!(code_of(resolve_targets(&cfg)), "duplicate_target_id");

        cfg.deploy.targets = vec![
            custom("one", &path_a, "none"),
            custom("two", &path_a, "none"),
        ];
        assert_eq!(code_of(resolve_targets(&cfg)), "duplicate_target_path");

        let mut bad_mode = custom("one", &path_a, "none");
        bad_mode.mode = Some("0666".to_string());
        cfg.deploy.targets = vec![bad_mode];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_mode");

        let mut too_wide = custom("one", &path_a, "none");
        too_wide.mode = Some("1600".to_string());
        cfg.deploy.targets = vec![too_wide];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_mode");

        let mut relative = custom("one", &path_a, "none");
        relative.path = Some("keybox.xml".to_string());
        cfg.deploy.targets = vec![relative];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_path");

        let mut dotdot = custom("one", &path_a, "none");
        dotdot.path = Some("/data/adb/../keybox.xml".to_string());
        cfg.deploy.targets = vec![dotdot];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_path");

        let mut empty_seg = custom("one", &path_a, "none");
        empty_seg.path = Some("/data//keybox.xml".to_string());
        cfg.deploy.targets = vec![empty_seg];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_path");

        let mut bad_owner = custom("one", &path_a, "none");
        bad_owner.owner = Some("root:0".to_string());
        cfg.deploy.targets = vec![bad_owner];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_owner");

        let mut bad_reload = custom("one", &path_a, "none");
        bad_reload.reload = Some("restart".to_string());
        cfg.deploy.targets = vec![bad_reload];
        assert_eq!(code_of(resolve_targets(&cfg)), "invalid_target_reload");

        let mut missing_mode = custom("one", &path_a, "none");
        missing_mode.mode = None;
        cfg.deploy.targets = vec![missing_mode];
        assert_eq!(code_of(resolve_targets(&cfg)), "custom_target_incomplete");

        cfg.deploy.targets = vec![TargetConfig {
            id: "omk".to_string(),
            path: Some("/tmp/keybox.xml".to_string()),
            owner: None,
            mode: None,
            reload: None,
        }];
        assert_eq!(code_of(resolve_targets(&cfg)), "builtin_path_override");

        cfg.deploy.targets = vec![TargetConfig {
            id: "teesim".to_string(),
            path: Some("/data/adb/teesim/nested/keybox.xml".to_string()),
            owner: None,
            mode: None,
            reload: None,
        }];
        assert_eq!(code_of(resolve_targets(&cfg)), "builtin_path_override");
    }
}
