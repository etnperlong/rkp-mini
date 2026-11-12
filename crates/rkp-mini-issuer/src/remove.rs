//! `remove`: delete the locally issued keybox and its sibling artifacts.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::RemoveArgs;
use crate::config::{self, Config};
use crate::error::{err, Stage};
use crate::rkp::keybox;
use crate::status;
use crate::util;

/// Files written by `issue` (`src/rkp/mod.rs`), keybox first.
const ARTIFACTS: &[&str] = &[
    "keybox.xml",
    "keybox.xml.prev",
    "private-key.pem",
    "cert-chain.pem",
    "cert-chain.der",
    "meta.json",
];

/// The `issue.lock` directory this run created; removed on drop, also on error.
struct IssueLock(PathBuf);

impl Drop for IssueLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

/// `remove` command entry point.
pub fn run(args: &RemoveArgs) -> Result<i32> {
    if !args.yes {
        return Err(err(
            Stage::Config,
            "confirmation_required",
            "remove deletes the local keybox and its artifacts; re-run with --yes",
        ));
    }
    let cfg = config::load(&config::resolve_path(&args.config))?;
    remove_artifacts(&cfg, Path::new(config::DATA_DIR), args.json)
}

fn remove_artifacts(cfg: &Config, data_dir: &Path, json: bool) -> Result<i32> {
    let _lock = take_issue_lock(data_dir)?;
    let removed = delete_artifacts(Path::new(&cfg.output_dir))?;

    // Same refresh as `issue` after writing; the keybox is gone now.
    let st = status::detect(cfg);
    let kb = keybox::inspect(&Path::new(&cfg.output_dir).join("keybox.xml"));
    let _ = status::update_module_description(&status::description_for(&st, &kb));

    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "data": {
                "removed": removed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "output_dir": cfg.output_dir.as_str(),
            }})
        );
    } else if removed.is_empty() {
        println!("nothing to remove in {}", cfg.output_dir);
    } else {
        for p in &removed {
            println!("removed {}", p.display());
        }
    }
    Ok(0)
}

/// Create `<data_dir>/tmp/issue.lock`; fail if the WebUI or auto-loop holds it.
fn take_issue_lock(data_dir: &Path) -> Result<IssueLock> {
    let tmp = data_dir.join("tmp");
    util::ensure_dir_0700(&tmp)
        .map_err(|e| err(Stage::Write, "io", format!("mkdir {}: {e}", tmp.display())))?;
    let lock = tmp.join("issue.lock");
    match fs::create_dir(&lock) {
        Ok(()) => Ok(IssueLock(lock)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(err(
            Stage::Write,
            "issue_running",
            format!(
                "issuance in progress ({} exists); nothing removed",
                lock.display()
            ),
        )),
        Err(e) => Err(err(
            Stage::Write,
            "io",
            format!("mkdir {}: {e}", lock.display()),
        )),
    }
}

/// Delete whichever artifacts exist, in order; a missing file is not an error.
fn delete_artifacts(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for name in ARTIFACTS {
        let path = dir.join(name);
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(err(
                    Stage::Write,
                    "io",
                    format!("remove {}: {e}", path.display()),
                ))
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_app_error;

    fn cfg_with_output(dir: &Path) -> Config {
        Config {
            output_dir: dir.display().to_string(),
            ..Config::default()
        }
    }

    #[test]
    fn deletes_exactly_the_artifacts() {
        let out = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        for f in ARTIFACTS {
            std::fs::write(out.path().join(f), b"x").unwrap();
        }
        std::fs::write(out.path().join("status.json"), b"{}").unwrap();
        std::fs::create_dir(out.path().join("logs")).unwrap();
        std::fs::write(out.path().join("logs/issue-1.log"), b"log").unwrap();

        let cfg = cfg_with_output(out.path());
        assert_eq!(remove_artifacts(&cfg, data.path(), true).unwrap(), 0);

        for f in ARTIFACTS {
            assert!(!out.path().join(f).exists(), "{f} still present");
        }
        assert!(out.path().join("status.json").exists());
        assert!(out.path().join("logs/issue-1.log").exists());
        assert!(out.path().exists(), "output dir itself must stay");
        assert!(
            !data.path().join("tmp/issue.lock").exists(),
            "own lock left behind"
        );
    }

    #[test]
    fn refuses_when_lock_is_held() {
        let out = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(out.path().join("keybox.xml"), b"x").unwrap();
        let lock = data.path().join("tmp/issue.lock");
        std::fs::create_dir_all(&lock).unwrap();

        let cfg = cfg_with_output(out.path());
        let e = remove_artifacts(&cfg, data.path(), false).unwrap_err();
        let app = to_app_error(&e);
        assert_eq!(app.code, "issue_running");
        assert_eq!(app.stage, Stage::Write);
        assert_eq!(app.exit_code(), 7);
        assert!(out.path().join("keybox.xml").exists(), "deleted anyway");
        assert!(lock.exists(), "pre-existing lock must stay");
    }

    #[test]
    fn missing_yes_is_a_usage_error() {
        let out = tempfile::tempdir().unwrap();
        std::fs::write(out.path().join("keybox.xml"), b"x").unwrap();

        let e = run(&RemoveArgs::default()).unwrap_err();
        let app = to_app_error(&e);
        assert_eq!(app.code, "confirmation_required");
        assert_eq!(app.stage, Stage::Config);
        assert_eq!(app.exit_code(), 2);
        assert!(out.path().join("keybox.xml").exists());
    }

    #[test]
    fn missing_artifacts_are_fine() {
        let dir = tempfile::tempdir().unwrap();
        assert!(delete_artifacts(dir.path()).unwrap().is_empty());
    }
}
