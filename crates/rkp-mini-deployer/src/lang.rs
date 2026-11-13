//! Native module-introduction localization.
//!
//! The WebUI persists the locale it resolved under `<data root>/ui-language`;
//! this crate reads only that code and owns its own compiled-in catalogs.
//! Nothing here reads the Vue catalogs or changes the global rust-i18n locale.

use std::path::Path;

/// Receiver data root: where the WebUI persists `ui-language`.
pub const DATA_ROOT: &str = "/data/adb/rkp_sync";
/// Canonical codes shared with the WebUI. Anything else means Chinese.
pub const LOCALES: [&str; 8] = ["zh-CN", "zh-TW", "en", "ja", "ko", "de", "fr", "it"];
const DEFAULT_LOCALE: &str = "zh-CN";

/// Locale persisted by the WebUI under `root`; missing or invalid → zh-CN.
pub fn load(root: &Path) -> &'static str {
    std::fs::read_to_string(root.join("ui-language"))
        .ok()
        .and_then(|raw| {
            let code = raw.trim();
            LOCALES.iter().copied().find(|l| *l == code)
        })
        .unwrap_or(DEFAULT_LOCALE)
}

/// The static module introduction in `locale` (explicit; never global state).
pub fn intro(locale: &str) -> String {
    rust_i18n::t!("module_intro", locale = locale).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn catalog_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("locales")
    }

    fn catalog(locale: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(catalog_dir().join(format!("{locale}.json"))).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn all_eight_catalogs_have_the_same_keys() {
        assert_eq!(LOCALES.len(), 8);
        let expected = BTreeSet::from([
            "module_intro".to_string(),
            "action.busy".to_string(),
            "action.start".to_string(),
            "action.done".to_string(),
            "action.round_start".to_string(),
            "action.auto_exit".to_string(),
            "action.preflight_exit".to_string(),
            "action.fetch_exit".to_string(),
            "action.deploy_exit".to_string(),
            "action.round_end".to_string(),
            "action.loop_exit".to_string(),
            "action.round_skipped".to_string(),
        ]);
        for locale in LOCALES {
            let keys: BTreeSet<String> = catalog(locale)
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| k.as_str() != "_version")
                .cloned()
                .collect();
            assert_eq!(keys, expected, "{locale} catalog keys");
        }
    }

    #[test]
    fn intro_comes_from_the_catalog_and_keeps_the_cloning_warning() {
        let warning = [
            ("zh-CN", "克隆"),
            ("zh-TW", "複製"),
            ("en", "cloning"),
            ("ja", "クローン"),
            ("ko", "복제"),
            ("de", "Klonen"),
            ("fr", "clonage"),
            ("it", "clonazione"),
        ];
        for (locale, word) in warning {
            let cat = catalog(locale);
            let expected = cat["module_intro"].as_str().unwrap();
            let text = intro(locale);
            assert_eq!(text, expected, "{locale}");
            assert!(
                text.contains(word),
                "{locale} lost the cloning warning: {text}"
            );
        }
    }

    #[test]
    fn locale_file_is_whitelisted_and_defaults_to_chinese() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), "zh-CN");
        std::fs::write(dir.path().join("ui-language"), "de\n").unwrap();
        assert_eq!(load(dir.path()), "de");
        std::fs::write(dir.path().join("ui-language"), "pt-BR\n").unwrap();
        assert_eq!(load(dir.path()), "zh-CN");
        std::fs::write(dir.path().join("ui-language"), "english").unwrap();
        assert_eq!(load(dir.path()), "zh-CN");
        std::fs::remove_file(dir.path().join("ui-language")).unwrap();
        assert_eq!(load(dir.path()), "zh-CN");
    }
}
