//! RKP issuance: protocol encoding, TA transport, server I/O, chain + keybox.

pub mod chain;
pub mod keybox;
pub mod keymint;
pub mod protocol;
pub mod server;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use p256::SecretKey;
use rand_core::OsRng;
use serde_json::Value;

use crate::cli::IssueArgs;
use crate::config::{self, Config};
use crate::error::{err, to_app_error, Stage};
use crate::status;
use crate::util;

use chain::CertInfo;

/// Accumulates issue log lines, flushed to `logs/issue-<unix>.log` at the end.
struct Log {
    lines: Vec<String>,
    start: i64,
    retention: u32,
}

impl Log {
    fn new(cfg: &Config) -> Self {
        Log {
            lines: Vec::new(),
            start: util::now_unix(),
            retention: cfg.log_retention,
        }
    }

    fn line(&mut self, msg: &str) {
        self.lines.push(format!(
            "{} {}",
            util::format_datetime(util::now_unix()),
            msg
        ));
    }

    fn finish(&self, result: &str) {
        let dir = Path::new(config::DATA_DIR).join("logs");
        if util::ensure_dir_0700(&dir).is_err() {
            return;
        }
        let mut body = String::new();
        body.push_str(&format!("# rkp-mini-issuer issue log ({result})\n"));
        for l in &self.lines {
            body.push_str(l);
            body.push('\n');
        }
        let path = dir.join(format!("issue-{}.log", self.start));
        let _ = util::atomic_write(&path, body.as_bytes(), 0o600);
        self.prune(&dir);
    }

    fn prune(&self, dir: &Path) {
        let mut logs: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with("issue-") && n.ends_with(".log"))
                        .unwrap_or(false)
                })
                .collect(),
            Err(_) => return,
        };
        logs.sort();
        let keep = self.retention as usize;
        if logs.len() > keep {
            for p in &logs[..logs.len() - keep] {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// Success data for human/JSON output.
struct Outcome {
    keybox_path: PathBuf,
    chain_len: usize,
    leaf_subject: String,
    leaf_not_after: i64,
    device_id: String,
    csr_len: usize,
    frame_layout: String,
    ta_status: i32,
}

impl Outcome {
    fn json(&self) -> Value {
        serde_json::json!({
            "keybox": self.keybox_path.display().to_string(),
            "chain_certificates": self.chain_len,
            "leaf_subject": self.leaf_subject,
            "leaf_not_after": self.leaf_not_after,
            "leaf_not_after_utc": util::format_rfc3339(self.leaf_not_after),
            "device_id": self.device_id,
            "csr_len": self.csr_len,
            "frame_layout": self.frame_layout,
            "ta_status": self.ta_status,
        })
    }

    fn print_human(&self) {
        let days = util::days_left(self.leaf_not_after, util::now_unix());
        println!("✅ RKP keybox issued");
        println!("   keybox : {}", self.keybox_path.display());
        println!(
            "   chain  : {} certs, leaf notAfter {} ({}天)",
            self.chain_len,
            util::format_datetime(self.leaf_not_after),
            days
        );
        println!("   device : {}", self.device_id);
        println!(
            "   csr    : {} bytes, TA frame {}",
            self.csr_len, self.frame_layout
        );
    }
}

/// `issue` command entry point.
pub fn issue(args: &IssueArgs) -> Result<i32> {
    if !args.acknowledge {
        eprintln!(
            "rkp-mini-issuer issue signs a fresh RKP keybox via the on-device KeyMint TA (0x3122)."
        );
        eprintln!("A new P-256 key is generated every run; an old keybox is never reused.");
        eprintln!("Re-run with --acknowledge.");
        return Ok(2);
    }

    let cfg = build_config(args)?;
    if let Some(threshold) = args.if_expiring_days {
        if let Some(days_left) = not_expiring(&cfg, threshold) {
            let keybox = Path::new(&cfg.output_dir).join("keybox.xml");
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "data": {
                        "skipped": true,
                        "reason": "not_expiring",
                        "days_left": days_left,
                        "threshold_days": threshold,
                        "keybox": keybox.display().to_string(),
                    }})
                );
            } else {
                println!(
                    "keybox still valid for {days_left} days (> {threshold}); not issuing: {}",
                    keybox.display()
                );
            }
            return Ok(0);
        }
    }
    let mut log = Log::new(&cfg);
    log.line("issue start");

    match issue_inner(&cfg, &mut log) {
        Ok(outcome) => {
            log.line("issue ok");
            log.finish("ok");
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "data": outcome.json() })
                );
            } else {
                outcome.print_human();
            }
            Ok(0)
        }
        Err(e) => {
            let app = to_app_error(&e);
            log.line(&format!(
                "error [{}] {}: {}",
                app.stage.as_str(),
                app.code,
                app.message
            ));
            log.finish("error");
            let desc =
                util::single_line(&format!("❌ [{}] {}", app.stage.as_str(), app.message), 120);
            let _ = status::update_module_description(&desc);
            Err(e)
        }
    }
}

fn build_config(args: &IssueArgs) -> Result<Config> {
    let path = config::resolve_path(&args.config);
    let mut cfg = config::load(&path)?;
    if let Some(s) = &args.server {
        cfg.server_url = config::effective_server_url(s)?;
    }
    if let Some(o) = &args.output {
        let s = o.to_string_lossy().to_string();
        if !Path::new(&s).is_absolute() {
            return Err(err(
                Stage::Config,
                "config_invalid",
                "--output must be absolute",
            ));
        }
        cfg.output_dir = s;
    }
    if let Some(f) = &args.fingerprint {
        cfg.fingerprint = f.clone();
    }
    Ok(cfg)
}

/// `Some(days_left)` when the current keybox is valid with more than
/// `threshold` whole days left; `None` means issuance should proceed.
fn not_expiring(cfg: &Config, threshold: u32) -> Option<i64> {
    let info = keybox::inspect(&Path::new(&cfg.output_dir).join("keybox.xml"));
    let not_after = info.not_after.filter(|_| info.valid)?;
    let days_left = util::days_left(not_after, util::now_unix());
    (days_left > i64::from(threshold)).then_some(days_left)
}

fn issue_inner(cfg: &Config, log: &mut Log) -> Result<Outcome> {
    // 1. Prerequisites — abort on the first blocker.
    let st = status::detect(cfg);
    if let Some(&b) = st.blockers.first() {
        return Err(err(
            Stage::Prereq,
            b,
            format!("prerequisite not met: {} ({})", b, status::blocker_hint(b)),
        ));
    }
    let fingerprint = config::effective_fingerprint(cfg);
    log.line(&format!("fingerprint len={}", fingerprint.len()));

    // 2. Fresh P-256 key.
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();
    let cose = protocol::cose_key(&public);
    let device_id = keybox::device_id(&public);

    // 3. Challenge.
    let challenge = server::fetch_challenge(cfg, &fingerprint)?;
    log.line(&format!("challenge_len={}", challenge.len()));

    // 4. KeyMint 0x3122 CSR via isolated worker.
    let request = protocol::build_request(&challenge, &cose)?;
    let tmp = Path::new(config::DATA_DIR).join("tmp");
    let reply = keymint::send(&request, Duration::from_secs(cfg.ta_timeout_seconds), &tmp)?;
    let frame = protocol::decode_frame(&reply.buffer)?;
    log.line(&format!(
        "ta_status={} frame={}",
        frame.status,
        frame.layout.name()
    ));

    // 5. Extract + validate CSR, append fingerprint.
    let csr = protocol::unwrap_csr(&frame.payload)?;
    protocol::validate_csr(&csr, &challenge, &cose)?;
    let csr_final = if cfg.include_fingerprint {
        protocol::append_fingerprint(&csr, &fingerprint)?
    } else {
        csr
    };
    log.line(&format!("csr_len={}", csr_final.len()));

    // 6. Sign + validate chain.
    let der = server::sign_certificates(cfg, &csr_final, &challenge)?;
    let now = util::now_unix();
    let ordered = chain::prepare(&der, &public, now)?;
    log.line(&format!(
        "chain_certs={} leaf_not_after={}",
        ordered.len(),
        ordered[0].not_after
    ));

    // 7. Write outputs atomically.
    let der_concat: Vec<u8> = ordered.iter().flat_map(|c| c.der.iter().copied()).collect();
    let meta = build_meta(
        cfg,
        &fingerprint,
        &challenge,
        &csr_final,
        frame.status,
        frame.layout.name(),
        &ordered,
        &der_concat,
        &device_id,
        now,
    );
    write_outputs(cfg, &secret, &ordered, &der_concat, &device_id, &meta)?;

    // 8. Refresh description from the freshly written keybox.
    let keybox_path = Path::new(&cfg.output_dir).join("keybox.xml");
    let kb = keybox::inspect(&keybox_path);
    let desc = status::description_for(&st, &kb);
    let _ = status::update_module_description(&desc);

    Ok(Outcome {
        keybox_path,
        chain_len: ordered.len(),
        leaf_subject: ordered[0].subject_cn.clone(),
        leaf_not_after: ordered[0].not_after,
        device_id,
        csr_len: csr_final.len(),
        frame_layout: frame.layout.name().to_string(),
        ta_status: frame.status,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_meta(
    cfg: &Config,
    fingerprint: &str,
    challenge: &[u8],
    csr: &[u8],
    ta_status: i32,
    frame_layout: &str,
    ordered: &[CertInfo],
    der_concat: &[u8],
    device_id: &str,
    now: i64,
) -> Value {
    let leaf = &ordered[0];
    serde_json::json!({
        "version": 1,
        "issued_at": now,
        "issued_at_utc": util::format_rfc3339(now),
        "fingerprint": fingerprint,
        "server": cfg.server_url,
        "challenge_hex": hex::encode(challenge),
        "csr_len": csr.len(),
        "csr_sha256": util::sha256_hex(csr),
        "ta_status": ta_status,
        "frame_layout": frame_layout,
        "chain_certificates": ordered.len(),
        "leaf_subject": leaf.subject_cn,
        "leaf_not_before": leaf.not_before,
        "leaf_not_after": leaf.not_after,
        "leaf_not_after_utc": util::format_rfc3339(leaf.not_after),
        "device_id": device_id,
        "chain_sha256": util::sha256_hex(der_concat),
    })
}

fn write_io(path: &Path, bytes: &[u8]) -> Result<()> {
    util::atomic_write(path, bytes, 0o600)
        .map_err(|e| err(Stage::Write, "io", format!("write {}: {e}", path.display())))
}

fn write_outputs(
    cfg: &Config,
    secret: &SecretKey,
    ordered: &[CertInfo],
    der_concat: &[u8],
    device_id: &str,
    meta: &Value,
) -> Result<()> {
    let dir = Path::new(&cfg.output_dir);
    util::ensure_dir_0700(dir).map_err(|e| {
        err(
            Stage::Write,
            "io",
            format!(
                "mkdir {} failed: {e}; check SELinux (ls -ldZ {}; dmesg | grep avc) \
                 or set output_dir to a writable path",
                dir.display(),
                dir.display()
            ),
        )
    })?;

    // Order: der -> pem -> private-key -> meta -> keybox (keybox last).
    write_io(&dir.join("cert-chain.der"), der_concat)?;
    write_io(
        &dir.join("cert-chain.pem"),
        keybox::chain_pem(ordered).as_bytes(),
    )?;
    write_io(
        &dir.join("private-key.pem"),
        keybox::private_key_pem(secret)?.as_bytes(),
    )?;
    let meta_json = serde_json::to_string_pretty(meta)
        .map_err(|e| err(Stage::Write, "io", format!("encode meta.json: {e}")))?;
    write_io(&dir.join("meta.json"), meta_json.as_bytes())?;

    // Back up the previous keybox before overwriting.
    let kb_path = dir.join("keybox.xml");
    if kb_path.exists() {
        let prev = dir.join("keybox.xml.prev");
        if std::fs::copy(&kb_path, &prev).is_ok() {
            let _ = std::fs::set_permissions(
                &prev,
                std::os::unix::fs::PermissionsExt::from_mode(0o600),
            );
        }
    }
    let xml = keybox::build(secret, ordered, device_id)?;
    write_io(&kb_path, xml.as_bytes())?;
    Ok(())
}
