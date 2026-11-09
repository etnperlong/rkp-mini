//! Prerequisite detection, keybox status, module description + status files.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::StatusArgs;
use crate::config::{self, Config};
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

/// Short human hint for a blocker code (used in the single-line description).
fn blocker_hint(code: &str) -> &'static str {
    match code {
        "android_arm64_required" => "需 arm64",
        "root_required" => "需 root",
        "qualcomm_required" => "非高通",
        "soc_unknown" => "SoC 未知",
        "soc_conflicting" => "SoC 冲突",
        "keymint_library_missing" => "缺 keymaster 库",
        "smcinvoke_missing" => "smcinvoke 缺失",
        "fingerprint_missing" => "缺指纹",
        _ => "前置未满足",
    }
}

/// Compose the single-line module description (spec §7.7).
pub fn description_for(status: &DeviceStatus, keybox: &KeyboxInfo) -> String {
    let now = util::now_unix();
    let raw = if keybox.exists {
        match keybox.not_after {
            Some(na) if na <= now => format!("❌ RKP 已过期 {}", util::format_date(na)),
            Some(na) if keybox.valid => {
                let days = util::days_left(na, now);
                if days <= 7 {
                    format!("✅ RKP {}（{}天，尽快续签）", util::format_date(na), days)
                } else {
                    format!("✅ RKP {}（{}天）", util::format_date(na), days)
                }
            }
            _ => {
                let reason = keybox
                    .error
                    .clone()
                    .unwrap_or_else(|| "解析失败".to_string());
                format!("❌ keybox 无效：{}", reason)
            }
        }
    } else if let Some(b) = status.blockers.first() {
        format!("❌ 未签发（{}）", blocker_hint(b))
    } else {
        "❌ 未签发；点 Action".to_string()
    };
    util::single_line(&raw, 120)
}

/// Locate the module root from the running binary: `.../bin/rkp-mini` -> root.
fn module_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // exe = <root>/bin/rkp-mini
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

/// Build the §8.4 status object.
pub fn status_json(
    status: &DeviceStatus,
    keybox: &KeyboxInfo,
    keybox_path: &Path,
    description: &str,
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
    let obj = status_json(&status, &kb, &keybox_path, &description);

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

    #[test]
    fn description_no_keybox_no_blocker() {
        let kb = KeyboxInfo::default();
        assert_eq!(description_for(&base_status(), &kb), "❌ 未签发；点 Action");
    }

    #[test]
    fn description_no_keybox_with_blocker() {
        let mut s = base_status();
        s.blockers = vec!["smcinvoke_missing"];
        let kb = KeyboxInfo::default();
        assert_eq!(description_for(&s, &kb), "❌ 未签发（smcinvoke 缺失）");
    }

    #[test]
    fn description_valid_keybox() {
        let na = util::now_unix() + 22 * 86_400;
        let kb = KeyboxInfo {
            exists: true,
            valid: true,
            certificates: 4,
            not_after: Some(na),
            key_match: true,
            ..Default::default()
        };
        let d = description_for(&base_status(), &kb);
        assert!(d.starts_with("✅ RKP"));
        assert!(d.contains("22天"));
    }

    #[test]
    fn description_expiring_soon() {
        let na = util::now_unix() + 5 * 86_400;
        let kb = KeyboxInfo {
            exists: true,
            valid: true,
            not_after: Some(na),
            key_match: true,
            ..Default::default()
        };
        assert!(description_for(&base_status(), &kb).contains("尽快续签"));
    }

    #[test]
    fn description_expired() {
        let na = util::now_unix() - 86_400;
        let kb = KeyboxInfo {
            exists: true,
            valid: true,
            not_after: Some(na),
            key_match: true,
            ..Default::default()
        };
        assert!(description_for(&base_status(), &kb).starts_with("❌ RKP 已过期"));
    }

    #[test]
    fn description_invalid_keybox() {
        let na = util::now_unix() + 10 * 86_400;
        let kb = KeyboxInfo {
            exists: true,
            valid: false,
            not_after: Some(na),
            key_match: false,
            error: Some("private key does not match leaf".into()),
            ..Default::default()
        };
        assert!(description_for(&base_status(), &kb).starts_with("❌ keybox 无效："));
    }
}
