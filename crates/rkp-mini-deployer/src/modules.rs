//! Read-only detection of the root modules that own the builtin targets.
//!
//! A builtin target is only useful when its module is installed: the module
//! creates the keybox directory and runs the watcher that reloads the file.
//! Nothing here writes, and nothing here decides whether `apply` may run.

use std::fs;
use std::path::Path;

use rkp_mini_core::fsutil::read_limited;
use serde::Serialize;

/// Where Magisk and KernelSU keep installed modules.
pub const MODULES_DIR: &str = "/data/adb/modules";
/// Upper bound for a `module.prop`.
const MAX_MODULE_PROP: usize = 16 * 1024;
/// Longest `name`/`version` value passed on to the UI.
const MAX_PROP_CHARS: usize = 64;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleInfo {
    pub id: &'static str,
    /// `<modules>/<id>/module.prop` exists.
    pub installed: bool,
    /// Installed and neither disabled nor queued for removal.
    pub enabled: bool,
    pub name: Option<String>,
    pub version: Option<String>,
}

/// Printable, length-capped value of `key` in a `module.prop`.
fn prop(text: &str, key: &str) -> Option<String> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))?;
    let clean: String = value
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_PROP_CHARS)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// Inspect `<modules_dir>/<id>`. Any read failure counts as "not installed":
/// the result only gates a UI choice, so it fails closed.
pub fn detect(modules_dir: &Path, id: &'static str) -> ModuleInfo {
    let dir = modules_dir.join(id);
    let text = fs::metadata(dir.join("module.prop"))
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| read_limited(&dir.join("module.prop"), MAX_MODULE_PROP).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let Some(text) = text else {
        return ModuleInfo {
            id,
            installed: false,
            enabled: false,
            name: None,
            version: None,
        };
    };
    let marked = |flag: &str| fs::symlink_metadata(dir.join(flag)).is_ok();
    ModuleInfo {
        id,
        installed: true,
        enabled: !marked("disable") && !marked("remove"),
        name: prop(&text, "name"),
        version: prop(&text, "version"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(modules: &Path, id: &str, prop: &str) {
        let dir = modules.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("module.prop"), prop).unwrap();
    }

    #[test]
    fn reports_installed_module_with_sanitized_props() {
        let dir = tempfile::tempdir().unwrap();
        install(
            dir.path(),
            "tricky_store",
            "id=tricky_store\nname= TEESimulator\u{7}-RS \nversion=v6.0.1\nversionCode=307\n",
        );
        let info = detect(dir.path(), "tricky_store");
        assert!(info.installed && info.enabled);
        assert_eq!(info.name.as_deref(), Some("TEESimulator-RS"));
        assert_eq!(info.version.as_deref(), Some("v6.0.1"));

        install(dir.path(), "long", &format!("name={}\n", "x".repeat(500)));
        let info = detect(dir.path(), "long");
        assert_eq!(info.name.unwrap().chars().count(), MAX_PROP_CHARS);
        assert_eq!(info.version, None);
    }

    #[test]
    fn missing_or_propless_module_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!detect(dir.path(), "teesim").installed);

        // A leftover data-less directory is not an installed module.
        fs::create_dir_all(dir.path().join("teesim")).unwrap();
        let info = detect(dir.path(), "teesim");
        assert!(!info.installed && !info.enabled);

        // Neither is a directory posing as module.prop.
        fs::create_dir_all(dir.path().join("omk").join("module.prop")).unwrap();
        assert!(!detect(dir.path(), "omk").installed);
    }

    #[test]
    fn disabled_or_removed_module_is_installed_but_not_enabled() {
        let dir = tempfile::tempdir().unwrap();
        for flag in ["disable", "remove"] {
            install(dir.path(), flag, "name=x\n");
            fs::write(dir.path().join(flag).join(flag), "").unwrap();
            let info = detect(dir.path(), flag);
            assert!(info.installed, "{flag}");
            assert!(!info.enabled, "{flag}");
        }
    }
}
