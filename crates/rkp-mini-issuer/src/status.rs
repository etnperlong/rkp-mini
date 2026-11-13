//! Prerequisite detection, keybox status, module description + status files.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rust_i18n::t;

use crate::cli::StatusArgs;
use crate::config::{self, Config};
use crate::error::{err, Stage};
use crate::rkp::keybox::{self, KeyboxInfo};
use crate::util;

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub android_arm64: bool,
    pub root: bool,
    pub qualcomm: bool,
    pub library_present: bool,
    pub device_present: bool,
    pub fingerprint: String,
    pub blockers: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocClass {
    Qualcomm,
    Other,
    Unknown,
    Conflicting,
}

/// Props and sysfs nodes inspected for SoC vendor classification.
const SOC_PROPS: &[&str] = &[
    "ro.soc.manufacturer",
    "ro.vendor.soc.manufacturer",
    "ro.soc.model",
    "ro.vendor.qti.soc_model",
    "ro.hardware",
    "ro.boot.hardware",
    "ro.board.platform",
];
const SOC_FILES: &[&str] = &["/sys/devices/soc0/family", "/sys/devices/soc0/machine"];

fn classify_soc() -> SocClass {
    let mut values: Vec<String> = SOC_PROPS.iter().map(|k| util::getprop(k)).collect();
    for f in SOC_FILES {
        if let Some(v) = util::read_file_trim(f) {
            values.push(v);
        }
    }
    let values: Vec<String> = values
        .into_iter()
        .map(|v| v.to_lowercase())
        .filter(|v| !v.is_empty())
        .collect();

    let is_qcom = values.iter().any(|v| {
        v.contains("qualcomm")
            || v.contains("qcom")
            || v.starts_with("sm")
            || v.starts_with("msm")
            || v.starts_with("sdm")
            || v.starts_with("qcs")
            || v.starts_with("sa")
    });
    let is_other = values
        .iter()
        .any(|v| v.contains("mediatek") || v.starts_with("mt"));

    match (is_qcom, is_other) {
        (true, true) => SocClass::Conflicting,
        (true, false) => SocClass::Qualcomm,
        (false, true) => SocClass::Other,
        (false, false) => SocClass::Unknown,
    }
}

fn is_android_arm64() -> bool {
    cfg!(all(target_os = "android", target_arch = "aarch64"))
}

fn is_root() -> bool {
    // SAFETY: geteuid is always safe.
    unsafe { libc::geteuid() == 0 }
}

/// Run all prerequisite checks (spec §9.1).
pub fn detect(cfg: &Config) -> DeviceStatus {
    let android_arm64 = is_android_arm64();
    let root = is_root();
    let soc = classify_soc();
    let qualcomm = soc == SocClass::Qualcomm;
    let library_present = Path::new(keybox_lib()).is_file();
    let device_present = Path::new(crate::rkp::keymint::SMCINVOKE).exists();
    let fingerprint = config::effective_fingerprint(cfg);

    let mut blockers: Vec<&'static str> = Vec::new();
    if !android_arm64 {
        blockers.push("android_arm64_required");
    }
    if !root {
        blockers.push("root_required");
    }
    match soc {
        SocClass::Qualcomm => {}
        SocClass::Other => blockers.push("qualcomm_required"),
        SocClass::Unknown => blockers.push("soc_unknown"),
        SocClass::Conflicting => blockers.push("soc_conflicting"),
    }
    if !library_present {
        blockers.push("keymint_library_missing");
    }
    if !device_present {
        blockers.push("smcinvoke_missing");
    }
    if fingerprint.is_empty() {
        blockers.push("fingerprint_missing");
    }

    DeviceStatus {
        android_arm64,
        root,
        qualcomm,
        library_present,
        device_present,
        fingerprint,
        blockers,
    }
}

fn keybox_lib() -> &'static str {
    crate::rkp::keymint::VENDOR_LIB
}

/// Canonical locale codes the WebUI persists in `<dataDir>/ui-language`.
const LOCALES: [&str; 8] = ["zh-CN", "zh-TW", "en", "ja", "ko", "de", "fr", "it"];
/// Native default when the persisted locale is missing or invalid.
const DEFAULT_LOCALE: &str = "zh-CN";

/// Translated hint for a blocker code; unknown codes use the generic hint.
fn hint(locale: &str, code: &str) -> String {
    let key = match code {
        "android_arm64_required" => "hint.android_arm64_required",
        "root_required" => "hint.root_required",
        "qualcomm_required" => "hint.qualcomm_required",
        "soc_unknown" => "hint.soc_unknown",
        "soc_conflicting" => "hint.soc_conflicting",
        "keymint_library_missing" => "hint.keymint_library_missing",
        "smcinvoke_missing" => "hint.smcinvoke_missing",
        "fingerprint_missing" => "hint.fingerprint_missing",
        _ => "hint.other",
    };
    t!(key, locale = locale).into_owned()
}

/// Short human hint for a blocker code. Diagnostics stay zh-CN by design.
pub(crate) fn blocker_hint(code: &str) -> String {
    hint(DEFAULT_LOCALE, code)
}

/// Day count with the locale's plural form; French treats 0 as singular.
fn day_text(locale: &str, days: i64) -> String {
    let one = if locale == "fr" { days <= 1 } else { days == 1 };
    let key = if one { "day.one" } else { "day.other" };
    t!(key, locale = locale, n = days).into_owned()
}

/// "Renew soon" threshold. Native RKP leaves last ≈12–14 days; matches the
/// recommended auto-issue threshold.
const EXPIRING_DAYS: i64 = 3;

fn lang_file() -> PathBuf {
    Path::new(config::DATA_DIR).join("ui-language")
}

/// Locale last chosen in the WebUI; a missing or invalid file keeps zh-CN.
fn load_locale(path: &Path) -> &'static str {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| LOCALES.iter().copied().find(|l| raw.trim() == *l))
        .unwrap_or(DEFAULT_LOCALE)
}

/// Locale-explicit renderer; `now` and the locale are injected so tests are
/// deterministic and never touch the production data dir.
fn render(locale: &str, status: &DeviceStatus, keybox: &KeyboxInfo, now: i64) -> String {
    if keybox.exists {
        match keybox.not_after {
            Some(na) if na <= now => t!(
                "description.expired",
                locale = locale,
                date = util::format_date(na)
            )
            .into_owned(),
            Some(na) if keybox.valid => {
                let left = util::days_left(na, now);
                let key = if left <= EXPIRING_DAYS {
                    "description.expiring"
                } else {
                    "description.valid"
                };
                t!(
                    key,
                    locale = locale,
                    date = util::format_date(na),
                    days = day_text(locale, left)
                )
                .into_owned()
            }
            _ => {
                // The raw diagnostic text is inserted verbatim, never re-parsed
                // as a template.
                let reason = keybox.error.clone().unwrap_or_else(|| {
                    t!("description.parse_failed", locale = locale).into_owned()
                });
                t!("description.invalid", locale = locale, reason = reason).into_owned()
            }
        }
    } else if let Some(b) = status.blockers.first() {
        t!(
            "description.blocked",
            locale = locale,
            hint = hint(locale, b)
        )
        .into_owned()
    } else {
        t!("description.unissued", locale = locale).into_owned()
    }
}

/// Compose the single-line module description (spec §7.7) in the WebUI locale.
pub fn description_for(status: &DeviceStatus, keybox: &KeyboxInfo) -> String {
    let raw = render(load_locale(&lang_file()), status, keybox, util::now_unix());
    util::single_line(&raw, 120)
}

/// Locate the module root from the running binary: `.../bin/rkp-mini-issuer` -> root.
fn module_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // exe = <root>/bin/rkp-mini-issuer
    exe.parent()?.parent().map(|p| p.to_path_buf())
}

/// Replace the `description=` line in module.prop (atomic, preserves perms).
pub fn update_module_description(text: &str) -> Result<()> {
    let root = module_root()
        .ok_or_else(|| anyhow::anyhow!("cannot locate module root from current_exe"))?;
    let prop = root.join("module.prop");
    let content = std::fs::read_to_string(&prop)?;
    let line = util::single_line(text, 120);

    let mut replaced = false;
    let mut out = String::with_capacity(content.len() + line.len());
    for l in content.lines() {
        if l.starts_with("description=") {
            out.push_str("description=");
            out.push_str(&line);
            replaced = true;
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    if !replaced {
        out.push_str("description=");
        out.push_str(&line);
        out.push('\n');
    }

    // Preserve existing file mode.
    let mode = std::fs::metadata(&prop)
        .map(|m| std::os::unix::fs::MetadataExt::mode(&m) & 0o7777)
        .unwrap_or(0o644);
    util::atomic_write(&prop, out.as_bytes(), mode)?;
    Ok(())
}

fn auto_loop_pid_path() -> PathBuf {
    Path::new(config::DATA_DIR)
        .join("tmp")
        .join("auto-loop.pid")
}

/// Whether the issuer module's `auto-loop.sh` is alive (pid file, no locking).
pub fn auto_loop_running() -> bool {
    loop_running_at(&auto_loop_pid_path())
}

fn loop_running_at(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<i32>() else {
        return false;
    };
    if pid <= 1 {
        return false;
    }
    // SAFETY: signal 0 only checks for existence/permission.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Build the §8.4 status object.
pub fn status_json(
    status: &DeviceStatus,
    keybox: &KeyboxInfo,
    keybox_path: &Path,
    description: &str,
    auto_issue: (u32, bool),
) -> serde_json::Value {
    let now = util::now_unix();
    let days_left = keybox.not_after.map(|na| util::days_left(na, now));
    serde_json::json!({
        "ok": true,
        "generated_at": now,
        "device": {
            "android_arm64": status.android_arm64,
            "root": status.root,
            "qualcomm": status.qualcomm,
            "library_present": status.library_present,
            "device_present": status.device_present,
            "fingerprint": status.fingerprint,
        },
        "blockers": status.blockers,
        "keybox": {
            "path": keybox_path.display().to_string(),
            "exists": keybox.exists,
            "valid": keybox.valid,
            "certificates": keybox.certificates,
            "leaf_subject": keybox.leaf_subject,
            "not_after": keybox.not_after,
            "not_after_utc": keybox.not_after.map(util::format_datetime),
            "days_left": days_left,
            "key_match": keybox.key_match,
            "error": keybox.error,
        },
        "description": description,
        "auto_issue": { "days": auto_issue.0, "loop_running": auto_issue.1 },
    })
}

fn human_status(status: &DeviceStatus, keybox: &KeyboxInfo, keybox_path: &Path, description: &str) {
    println!("{description}");
    println!(
        "   device : arm64={} root={} qualcomm={} lib={} smcinvoke={}",
        status.android_arm64,
        status.root,
        status.qualcomm,
        status.library_present,
        status.device_present
    );
    println!("   fingerprint : {}", status.fingerprint);
    if status.blockers.is_empty() {
        println!("   blockers : none");
    } else {
        println!("   blockers : {}", status.blockers.join(", "));
    }
    print!(
        "   keybox : {} exists={}",
        keybox_path.display(),
        keybox.exists
    );
    if keybox.exists {
        print!(
            " valid={} certs={} key_match={}",
            keybox.valid, keybox.certificates, keybox.key_match
        );
        if let Some(na) = keybox.not_after {
            print!(
                " notAfter {} ({}天)",
                util::format_datetime(na),
                util::days_left(na, util::now_unix())
            );
        }
        if let Some(e) = &keybox.error {
            print!(" error={e}");
        }
    }
    println!();
}

/// Write `status.json` and `status.txt` under the data dir (best effort).
fn write_status_files(obj: &serde_json::Value, description: &str) {
    let dir = Path::new(config::DATA_DIR);
    if util::ensure_dir_0700(dir).is_err() {
        return;
    }
    let json = serde_json::to_string_pretty(obj).unwrap_or_default();
    let _ = util::atomic_write(&dir.join("status.json"), json.as_bytes(), 0o600);
    let txt = format!("{description}\n{json}\n");
    let _ = util::atomic_write(&dir.join("status.txt"), txt.as_bytes(), 0o600);
}

/// `status` command.
pub fn run(args: &StatusArgs) -> Result<i32> {
    let cfg_path = config::resolve_path(&args.config);
    let cfg = config::load(&cfg_path)?;

    let status = detect(&cfg);
    let keybox_path = Path::new(&cfg.output_dir).join("keybox.xml");
    let kb = keybox::inspect(&keybox_path);
    let description = description_for(&status, &kb);
    let obj = status_json(
        &status,
        &kb,
        &keybox_path,
        &description,
        (cfg.auto_issue_days, auto_loop_running()),
    );

    write_status_files(&obj, &description);

    if args.update_description {
        if let Err(e) = update_module_description(&description) {
            if !args.quiet {
                eprintln!("warning: could not update module.prop: {e}");
            }
        }
    }

    if args.json {
        let env = serde_json::json!({ "ok": true, "data": obj });
        println!("{env}");
    } else if !args.quiet {
        human_status(&status, &kb, &keybox_path, &description);
    }
    Ok(0)
}

/// A module.prop write failure is a write-stage error (exit 7) for callers.
fn description_write(res: Result<()>) -> Result<()> {
    res.map_err(|e| err(Stage::Write, "description_write", e.to_string()))
}

/// `update-description` command: refresh module.prop only, no status files.
pub fn run_update_description(config_path: &Option<PathBuf>) -> Result<i32> {
    let cfg = config::load(&config::resolve_path(config_path))?;
    let status = detect(&cfg);
    let kb = keybox::inspect(&Path::new(&cfg.output_dir).join("keybox.xml"));
    let description = description_for(&status, &kb);
    description_write(update_module_description(&description))?;
    println!("{description}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_status() -> DeviceStatus {
        DeviceStatus {
            android_arm64: true,
            root: true,
            qualcomm: true,
            library_present: true,
            device_present: true,
            fingerprint: "fp".into(),
            blockers: vec![],
        }
    }

    const NOW: i64 = 1_700_000_000;

    fn kb_at(na: i64, valid: bool) -> KeyboxInfo {
        KeyboxInfo {
            exists: true,
            valid,
            certificates: 4,
            not_after: Some(na),
            key_match: valid,
            ..Default::default()
        }
    }

    #[test]
    fn every_locale_renders_every_state() {
        let na = NOW + 22 * 86_400;
        for locale in LOCALES {
            let cases = [
                (kb_at(na, true), "valid"),
                (kb_at(NOW + 2 * 86_400, true), "expiring"),
                (kb_at(NOW - 86_400, true), "expired"),
                (kb_at(NOW + 10 * 86_400, false), "invalid"),
            ];
            for (kb, state) in cases {
                let d = render(locale, &base_status(), &kb, NOW);
                assert_eq!(d.lines().count(), 1, "{locale} {state}: {d}");
                assert!(!d.contains("%{"), "{locale} {state}: {d}");
                assert!(!d.contains("description."), "{locale} {state}: {d}");
                if let Some(na) = kb.not_after.filter(|_| state != "invalid") {
                    let date = util::format_date(na);
                    assert!(d.contains(&date), "{locale} {state}: {d}");
                }
            }
            let mut blocked = base_status();
            blocked.blockers = vec!["fingerprint_missing"];
            let b = render(locale, &blocked, &KeyboxInfo::default(), NOW);
            assert!(!b.contains("fingerprint_missing"), "{locale}: {b}");
            let unissued = render(locale, &base_status(), &KeyboxInfo::default(), NOW);
            assert_ne!(b, unissued, "{locale}");
            assert!(!unissued.is_empty(), "{locale}");
        }
    }

    #[test]
    fn plural_forms_follow_locale_rules() {
        fn subst(key: &str, locale: &str, n: i64) -> String {
            t!(key, locale = locale, n = n).into_owned()
        }
        for locale in ["en", "de", "it"] {
            assert_eq!(day_text(locale, 1), subst("day.one", locale, 1), "{locale}");
            assert_ne!(
                subst("day.one", locale, 1),
                subst("day.other", locale, 1),
                "{locale}"
            );
            for n in [0, 2, 5] {
                assert_eq!(
                    day_text(locale, n),
                    subst("day.other", locale, n),
                    "{locale} {n}"
                );
            }
        }
        assert_eq!(day_text("fr", 0), subst("day.one", "fr", 0));
        assert_eq!(day_text("fr", 2), subst("day.other", "fr", 2));
        // Invariant locales reuse the same text for both categories.
        for locale in ["zh-CN", "zh-TW", "ja", "ko"] {
            for n in [0, 1, 2] {
                assert_eq!(
                    subst("day.one", locale, n),
                    subst("day.other", locale, n),
                    "{locale} {n}"
                );
                assert_eq!(
                    day_text(locale, n),
                    subst("day.other", locale, n),
                    "{locale} {n}"
                );
            }
        }
    }

    #[test]
    fn catalog_keys_and_placeholders_match_across_locales() {
        fn placeholders(s: &str) -> Vec<String> {
            let mut out: Vec<String> = s
                .match_indices("%{")
                .filter_map(|(i, _)| s[i + 2..].split_once('}').map(|(key, _)| key.to_string()))
                .collect();
            out.sort();
            out
        }
        fn load(locale: &str) -> serde_json::Map<String, serde_json::Value> {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("locales")
                .join(format!("{locale}.json"));
            let raw = std::fs::read_to_string(path).unwrap();
            serde_json::from_str::<serde_json::Value>(&raw)
                .unwrap()
                .as_object()
                .unwrap()
                .clone()
        }
        let base = load(DEFAULT_LOCALE);
        let mut keys: Vec<&str> = base.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "day.one",
                "day.other",
                "description.blocked",
                "description.expired",
                "description.expiring",
                "description.invalid",
                "description.parse_failed",
                "description.unissued",
                "description.valid",
                "hint.android_arm64_required",
                "hint.fingerprint_missing",
                "hint.keymint_library_missing",
                "hint.other",
                "hint.qualcomm_required",
                "hint.root_required",
                "hint.smcinvoke_missing",
                "hint.soc_conflicting",
                "hint.soc_unknown",
            ]
        );
        for locale in LOCALES {
            let cat = load(locale);
            assert_eq!(cat.len(), base.len(), "{locale}");
            for (key, value) in &base {
                let text = cat
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| panic!("{locale} missing {key}"));
                assert!(!text.is_empty(), "{locale} {key}");
                assert_eq!(
                    placeholders(text),
                    placeholders(value.as_str().unwrap()),
                    "{locale} {key}"
                );
            }
        }
    }

    #[test]
    fn description_keeps_raw_error_reason() {
        // Placeholder-like text in the diagnostic must survive untouched.
        let reason = "bad %{reason} %{hint} 100%";
        let kb = KeyboxInfo {
            error: Some(reason.into()),
            ..kb_at(NOW + 10 * 86_400, false)
        };
        for locale in LOCALES {
            let d = render(locale, &base_status(), &kb, NOW);
            assert!(d.ends_with(reason), "{locale}: {d}");
            assert_eq!(d.matches("%{reason}").count(), 1, "{locale}: {d}");
            assert_eq!(d.matches("%{hint}").count(), 1, "{locale}: {d}");
        }
        let no_reason = KeyboxInfo {
            error: None,
            ..kb_at(NOW + 10 * 86_400, false)
        };
        let failed = t!("description.parse_failed", locale = "en").into_owned();
        let expected = t!("description.invalid", locale = "en", reason = failed).into_owned();
        assert_eq!(render("en", &base_status(), &no_reason, NOW), expected);
    }

    #[test]
    fn persisted_locale_loads_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-language");
        assert_eq!(load_locale(&path), DEFAULT_LOCALE);
        std::fs::write(&path, "ko\n").unwrap();
        assert_eq!(load_locale(&path), "ko");
        std::fs::write(&path, " fr \n").unwrap();
        assert_eq!(load_locale(&path), "fr");
        for bad in ["pt-BR\n", "zh\n", "\n", "en-US"] {
            std::fs::write(&path, bad).unwrap();
            assert_eq!(load_locale(&path), DEFAULT_LOCALE, "{bad:?}");
        }
    }

    #[test]
    fn description_write_failure_uses_write_stage() {
        let e = description_write(Err(anyhow::anyhow!("boom"))).unwrap_err();
        let app = crate::error::to_app_error(&e);
        assert_eq!(app.stage, Stage::Write);
        assert_eq!(app.code, "description_write");
        assert_eq!(app.exit_code(), 7);
    }

    #[test]
    fn status_json_carries_auto_issue() {
        let obj = status_json(
            &base_status(),
            &KeyboxInfo::default(),
            Path::new("/nonexistent/keybox.xml"),
            "d",
            (30, false),
        );
        assert_eq!(obj["auto_issue"]["days"], 30);
        assert_eq!(obj["auto_issue"]["loop_running"], false);
        assert!(obj["ok"].as_bool().unwrap());
    }

    #[test]
    fn loop_running_false_without_usable_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!loop_running_at(&dir.path().join("auto-loop.pid")));
        let p = dir.path().join("garbage.pid");
        std::fs::write(&p, b"not-a-pid\n").unwrap();
        assert!(!loop_running_at(&p));
        std::fs::write(&p, b"0\n").unwrap();
        assert!(!loop_running_at(&p));
        // Above pid_max on Linux, so never a live process.
        std::fs::write(&p, b"2147483647\n").unwrap();
        assert!(!loop_running_at(&p));
    }
}
