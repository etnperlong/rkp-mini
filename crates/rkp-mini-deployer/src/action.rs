//! Internal `__action-message KEY [VALUE]`: the human text the module Action
//! shell prints (busy/banner/footer/round lines).
//!
//! The script owns control flow, statuses and raw output; it passes only
//! literal paths and exit codes. Keys and argument counts are a fixed
//! allowlist, so a caller cannot reach an arbitrary catalog key, eval shell
//! text, read config/env, or touch the network. Text comes from the compiled
//! catalogs in the locale the WebUI last saved (zh-CN when unset).

use anyhow::{bail, Result};
use rust_i18n::t;

use crate::lang;

/// Allowlisted message key -> literal argument names, in command-line order.
fn params(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "busy" | "done" | "round_start" | "round_end" | "loop_exit" | "round_skipped" => &[],
        "start" => &["log"],
        "auto_exit" | "preflight_exit" | "fetch_exit" | "deploy_exit" => &["code"],
        _ => return None,
    })
}

/// Accept both `busy` and `action.busy` spellings at the CLI.
fn normalize(key: &str) -> &str {
    key.strip_prefix("action.").unwrap_or(key)
}

/// Reject unknown keys and wrong argument counts (usage error, exit 2).
pub fn validate(key: &str, args: &[String]) -> Result<()> {
    let key = normalize(key);
    let Some(names) = params(key) else {
        bail!("unknown action message: {key}");
    };
    if args.len() != names.len() {
        bail!(
            "action message {key} takes {} argument(s), got {}",
            names.len(),
            args.len()
        );
    }
    if let Some(flag) = args.iter().find(|a| a.starts_with("--")) {
        bail!("unknown action message flag: {flag}");
    }
    Ok(())
}

fn arg(args: &[String], i: usize) -> &str {
    args.get(i).map(String::as_str).unwrap_or_default()
}

/// Render one message in the locale saved by the WebUI.
pub fn render(key: &str, args: &[String]) -> Result<String> {
    validate(key, args)?;
    Ok(render_in(
        lang::load(std::path::Path::new(lang::DATA_ROOT)),
        normalize(key),
        args,
    ))
}

/// Locale-explicit renderer; validated callers only, so `args` arity holds.
fn render_in(locale: &str, key: &str, args: &[String]) -> String {
    match key {
        "busy" => t!("action.busy", locale = locale).into_owned(),
        "start" => t!("action.start", locale = locale, log = arg(args, 0)).into_owned(),
        "done" => t!("action.done", locale = locale).into_owned(),
        "round_start" => t!("action.round_start", locale = locale).into_owned(),
        "auto_exit" => t!("action.auto_exit", locale = locale, code = arg(args, 0)).into_owned(),
        "preflight_exit" => t!(
            "action.preflight_exit",
            locale = locale,
            code = arg(args, 0)
        )
        .into_owned(),
        "fetch_exit" => t!("action.fetch_exit", locale = locale, code = arg(args, 0)).into_owned(),
        "deploy_exit" => {
            t!("action.deploy_exit", locale = locale, code = arg(args, 0)).into_owned()
        }
        "round_end" => t!("action.round_end", locale = locale).into_owned(),
        "loop_exit" => t!("action.loop_exit", locale = locale).into_owned(),
        _ => t!("action.round_skipped", locale = locale).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn catalog(locale: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("locales")
                .join(format!("{locale}.json")),
        )
        .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    const CASES: [(&str, Option<&str>); 11] = [
        ("busy", None),
        ("start", Some("/data/adb/rkp_sync/logs/sync.log")),
        ("done", None),
        ("round_start", None),
        ("auto_exit", Some("7")),
        ("preflight_exit", Some("3")),
        ("fetch_exit", Some("0")),
        ("deploy_exit", Some("9")),
        ("round_end", None),
        ("loop_exit", None),
        ("round_skipped", None),
    ];

    fn expected(cat: &serde_json::Value, key: &str, value: Option<&str>) -> String {
        let text = cat.get(format!("action.{key}")).unwrap().as_str().unwrap();
        match value {
            Some(v) => text.replace("%{log}", v).replace("%{code}", v),
            None => text.to_string(),
        }
    }

    #[test]
    fn all_messages_render_in_all_eight_locales() {
        for locale in lang::LOCALES {
            let cat = catalog(locale);
            for (key, value) in CASES {
                let extra = value.map(|v| args(&[v])).unwrap_or_default();
                let text = render_in(locale, key, &extra);
                assert_eq!(text, expected(&cat, key, value), "{locale} {key}");
                assert!(!text.contains("%{"), "{locale} {key} kept a placeholder");
            }
        }
    }

    #[test]
    fn arity_is_fixed_per_key() {
        assert!(validate("busy", &args(&["x"])).is_err());
        assert!(validate("start", &[]).is_err());
        assert!(validate("start", &args(&["a", "b"])).is_err());
        assert!(validate("auto_exit", &[]).is_err());
        assert!(validate("auto_exit", &args(&["1", "2"])).is_err());
    }

    #[test]
    fn unknown_keys_and_flags_are_usage_errors() {
        for key in ["module_intro", "nope", "--json", "-h", ""] {
            assert!(validate(key, &[]).is_err(), "{key}");
        }
        assert!(validate("start", &args(&["--json"])).is_err());
        assert!(validate("auto_exit", &args(&["--1"])).is_err());
    }

    #[test]
    fn action_prefix_is_accepted_at_the_cli() {
        assert_eq!(normalize("busy"), "busy");
        assert_eq!(normalize("action.fetch_exit"), "fetch_exit");
        assert_eq!(
            render_in("en", normalize("action.busy"), &[]),
            catalog("en").get("action.busy").unwrap().as_str().unwrap()
        );
        assert!(validate("action.busy", &[]).is_ok());
        assert!(validate("action.fetch_exit", &args(&["0"])).is_ok());
    }

    #[test]
    fn catalogs_cover_the_action_keys_with_the_same_placeholders() {
        let keys: BTreeSet<&str> = CASES.iter().map(|(k, _)| *k).collect();
        let placeholders = |v: &serde_json::Value| -> BTreeSet<String> {
            let text = v.as_str().unwrap();
            text.split("%{")
                .skip(1)
                .map(|part| part.split('}').next().unwrap().to_string())
                .collect()
        };
        let reference = catalog("en");
        for locale in lang::LOCALES {
            let cat = catalog(locale);
            for key in &keys {
                let value = cat.get(format!("action.{key}")).unwrap();
                assert!(value.is_string(), "{locale} {key}");
                assert_eq!(
                    placeholders(value),
                    placeholders(reference.get(format!("action.{key}")).unwrap()),
                    "{locale} {key} placeholders"
                );
            }
            for key in cat.as_object().unwrap().keys() {
                if key != "_version" && key != "module_intro" {
                    assert!(
                        keys.contains(key.trim_start_matches("action.")),
                        "{locale} unexpected {key}"
                    );
                }
            }
        }
    }
}
