//! Internal `__action-message KEY [VALUE]`: the human text the module shell
//! scripts print outside JSON envelopes (busy/banner/footer/round lines).
//!
//! The scripts own control flow, statuses and raw output; they pass only
//! literal paths, counts and exit codes taken from the command they just ran.
//! Keys and argument counts are a fixed allowlist, so a caller cannot reach an
//! arbitrary catalog key, eval shell text, read config/env, or touch the
//! network. Text comes from the compiled catalogs in the WebUI locale.

use anyhow::{bail, Result};
use rust_i18n::t;

use crate::status::current_locale;

/// Allowlisted message key -> literal argument names, in command-line order.
fn params(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "busy" | "done" | "issue_locked" | "issue_disabled" | "round_end" | "loop_exit"
        | "round_skipped" => &[],
        "start" => &["log"],
        "round_start" => &["days"],
        "issue_exit" | "push_exit" | "push_skipped" => &["code"],
        _ => return None,
    })
}

/// Accept both `busy` and `action.busy` spellings.
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
    Ok(())
}

fn arg(args: &[String], i: usize) -> &str {
    args.get(i).map(String::as_str).unwrap_or_default()
}

/// Render one message in the locale saved by the WebUI (zh-CN when unset).
pub fn render(key: &str, args: &[String]) -> Result<String> {
    validate(key, args)?;
    Ok(render_in(current_locale(), normalize(key), args))
}

/// Locale-explicit renderer; validated callers only, so `args` arity holds.
fn render_in(locale: &str, key: &str, args: &[String]) -> String {
    match key {
        "busy" => t!("action.busy", locale = locale).into_owned(),
        "start" => t!("action.start", locale = locale, log = arg(args, 0)).into_owned(),
        "done" => t!("action.done", locale = locale).into_owned(),
        "round_start" => {
            t!("action.round_start", locale = locale, days = arg(args, 0)).into_owned()
        }
        "issue_exit" => t!("action.issue_exit", locale = locale, code = arg(args, 0)).into_owned(),
        "issue_locked" => t!("action.issue_locked", locale = locale).into_owned(),
        "issue_disabled" => t!("action.issue_disabled", locale = locale).into_owned(),
        "push_exit" => t!("action.push_exit", locale = locale, code = arg(args, 0)).into_owned(),
        "push_skipped" => {
            t!("action.push_skipped", locale = locale, code = arg(args, 0)).into_owned()
        }
        "round_end" => t!("action.round_end", locale = locale).into_owned(),
        "loop_exit" => t!("action.loop_exit", locale = locale).into_owned(),
        _ => t!("action.round_skipped", locale = locale).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::LOCALES;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Every allowlisted key renders in all locales with named params filled.
    #[test]
    fn every_locale_renders_every_message() {
        let cases: [(&str, Vec<String>); 12] = [
            ("busy", args(&[])),
            (
                "start",
                args(&["/data/misc/keystore/rkp-mini/logs/auto.log"]),
            ),
            ("done", args(&[])),
            ("round_start", args(&["0"])),
            ("issue_exit", args(&["0"])),
            ("issue_locked", args(&[])),
            ("issue_disabled", args(&[])),
            ("push_exit", args(&["7"])),
            ("push_skipped", args(&["3"])),
            ("round_end", args(&[])),
            ("loop_exit", args(&[])),
            ("round_skipped", args(&[])),
        ];
        for locale in LOCALES {
            for (key, a) in &cases {
                assert!(validate(key, a).is_ok(), "{locale} {key}");
                let text = render_in(locale, key, a);
                assert!(!text.is_empty(), "{locale} {key}");
                assert!(!text.contains("%{"), "{locale} {key}: {text}");
                assert!(!text.contains("action."), "{locale} {key}: {text}");
            }
        }
    }

    /// `action.`-prefixed keys work too; unknown keys and bad arity are usage errors.
    #[test]
    fn keys_and_arity_are_validated() {
        assert!(validate("action.busy", &args(&[])).is_ok());
        assert!(validate("busy", &args(&["x"])).is_err());
        assert!(validate("start", &args(&[])).is_err());
        assert!(validate("start", &args(&["a", "b"])).is_err());
        assert!(validate("issue_exit", &args(&[])).is_err());
        assert!(validate("round_start", &args(&["1"])).is_ok());
        for bad in ["", "nope", "action.", "hint.other", "description.valid"] {
            assert!(validate(bad, &args(&[])).is_err(), "{bad:?}");
        }
        assert!(render("nope", &args(&[])).is_err());
    }

    /// Script-supplied literals are inserted verbatim, never re-parsed.
    #[test]
    fn literal_arguments_are_opaque() {
        let text = render_in("en", "start", &args(&["/x %{log} 'a b' `id` $(x)"]));
        assert!(
            text.contains("/x %{log} 'a b' `id` $(x)"),
            "opaque log path: {text}"
        );
        assert_eq!(text.matches("%{log}").count(), 1, "{text}");
        let text = render_in("zh-CN", "issue_exit", &args(&["%{code}"]));
        assert_eq!(text.matches("%{code}").count(), 1, "{text}");
        assert!(text.contains("%{code}"), "{text}");
    }
}
