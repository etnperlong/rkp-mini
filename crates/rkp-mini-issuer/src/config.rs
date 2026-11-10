//! TOML configuration: defaults, load+validate, `config init|show`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Deserialize;

use crate::error::{err, Stage};
use crate::util;

pub const DEFAULT_CONFIG_PATH: &str = "/data/misc/keystore/rkp-mini/config.toml";
pub const DEFAULT_SERVER_URL: &str = "https://remoteprovisioning.googleapis.com/v1";
pub const DEFAULT_OUTPUT_DIR: &str = "/data/misc/keystore/rkp-mini/output";
pub const DATA_DIR: &str = "/data/misc/keystore/rkp-mini";

pub const TEMPLATE: &str = r#"# RKP Mini configuration
server_url = "https://remoteprovisioning.googleapis.com/v1"
fingerprint = ""                  # empty = read ro.build.fingerprint
include_fingerprint = true        # append {"fingerprint": ...} to the CSR
output_dir = "/data/misc/keystore/rkp-mini/output"
request_timeout_seconds = 30
ta_timeout_seconds = 30
proxy = ""                        # e.g. "http://127.0.0.1:7890"; empty = direct
log_retention = 10                # keep the most recent N issue logs

# Re-issue when the current leaf has <= N days left. Used only by the sync
# loop (`rkp-mini-issuer issue --if-expiring-days N` before `rkp-mini-sync push`) while
# remote sync is enabled. 0 = off.
auto_issue_days = 0
"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server_url: String,
    pub fingerprint: String,
    pub include_fingerprint: bool,
    pub output_dir: String,
    pub request_timeout_seconds: u64,
    pub ta_timeout_seconds: u64,
    pub proxy: String,
    pub log_retention: u32,
    pub auto_issue_days: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server_url: DEFAULT_SERVER_URL.to_string(),
            fingerprint: String::new(),
            include_fingerprint: true,
            output_dir: DEFAULT_OUTPUT_DIR.to_string(),
            request_timeout_seconds: 30,
            ta_timeout_seconds: 30,
            proxy: String::new(),
            log_retention: 10,
            auto_issue_days: 0,
        }
    }
}

/// Fields the user wrote that we do not recognise (forward compat).
fn unknown_keys(raw: &toml::Table) -> Vec<String> {
    const KNOWN: &[&str] = &[
        "server_url",
        "fingerprint",
        "include_fingerprint",
        "output_dir",
        "request_timeout_seconds",
        "ta_timeout_seconds",
        "proxy",
        "log_retention",
        "auto_issue_days",
        // Owned by `rkp-mini-sync` (remote keybox sync); not read by rkp-mini-issuer.
        "sync",
    ];
    raw.keys()
        .filter(|k| !KNOWN.contains(&k.as_str()))
        .cloned()
        .collect()
}

/// Load config (defaults when the file is missing) and validate.
pub fn load(path: &Path) -> Result<Config> {
    let (cfg, warnings) = load_with_warnings(path)?;
    for w in warnings {
        eprintln!("warning: config: {w}");
    }
    Ok(cfg)
}

/// Load + validate, returning non-fatal warnings (unknown keys) separately.
pub fn load_with_warnings(path: &Path) -> Result<(Config, Vec<String>)> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Config::default(), Vec::new()));
        }
        Err(e) => {
            return Err(err(
                Stage::Config,
                "config_invalid",
                format!("read {path:?}: {e}"),
            ))
        }
    };

    let raw: toml::Table = text.parse().map_err(|e| {
        err(
            Stage::Config,
            "config_invalid",
            format!("TOML parse error: {e}"),
        )
    })?;
    let warnings: Vec<String> = unknown_keys(&raw)
        .into_iter()
        .map(|k| format!("unknown field `{k}` ignored"))
        .collect();

    let mut cfg: Config = raw.try_into().map_err(|e| {
        err(
            Stage::Config,
            "config_invalid",
            format!("TOML field error: {e}"),
        )
    })?;
    validate(&mut cfg)?;
    Ok((cfg, warnings))
}

/// Normalise and range-check config fields in place.
pub fn validate(cfg: &mut Config) -> Result<()> {
    cfg.server_url = effective_server_url(&cfg.server_url)?;

    if !Path::new(&cfg.output_dir).is_absolute() {
        return Err(err(
            Stage::Config,
            "config_invalid",
            format!("output_dir must be absolute: {}", cfg.output_dir),
        ));
    }
    if !(1..=300).contains(&cfg.request_timeout_seconds) {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "request_timeout_seconds must be 1..=300",
        ));
    }
    if !(1..=300).contains(&cfg.ta_timeout_seconds) {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "ta_timeout_seconds must be 1..=300",
        ));
    }
    if cfg.log_retention > 1000 {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "log_retention must be 0..=1000",
        ));
    }
    let proxy_ok = cfg.proxy.is_empty()
        || cfg.proxy.starts_with("http://")
        || cfg.proxy.starts_with("https://");
    if !proxy_ok {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "proxy must be empty or http(s)://; SOCKS requires the socks-proxy feature",
        ));
    }
    Ok(())
}

/// Normalise a server base URL per spec §6.4.
pub fn effective_server_url(base: &str) -> Result<String> {
    let base = base.trim();
    if base.contains('?') || base.contains('#') {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "server_url must not contain query/fragment",
        ));
    }
    let allowed = base.starts_with("https://")
        || base.starts_with("http://127.0.0.1")
        || base.starts_with("http://localhost");
    if !allowed {
        return Err(err(
            Stage::Config,
            "config_invalid",
            format!("server_url must be https:// (or http://127.0.0.1 / http://localhost): {base}"),
        ));
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap();
    let (prefix, rest) = base.split_at(scheme_end);
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if host.is_empty() {
        return Err(err(
            Stage::Config,
            "config_invalid",
            "server_url has no host",
        ));
    }
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "/v1" } else { path };
    Ok(format!("{prefix}{host}{path}"))
}

/// Effective fingerprint: config value if set, else `ro.build.fingerprint`.
pub fn effective_fingerprint(cfg: &Config) -> String {
    if !cfg.fingerprint.is_empty() {
        cfg.fingerprint.clone()
    } else {
        util::getprop("ro.build.fingerprint")
    }
}

/// Resolve the config path: CLI override or the fixed default.
pub fn resolve_path(cli: &Option<PathBuf>) -> PathBuf {
    cli.clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
}

/// `config init`: write the template only if the file is absent.
pub fn init(path: &Path) -> Result<()> {
    if path.exists() {
        println!("config already exists: {}", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| err(Stage::Write, "io", format!("mkdir {parent:?}: {e}")))?;
    }
    util::atomic_write(path, TEMPLATE.as_bytes(), 0o600)
        .map_err(|e| err(Stage::Write, "io", format!("write {path:?}: {e}")))?;
    println!("wrote config: {}", path.display());
    Ok(())
}

/// `config show`: print the effective (merged) config; no secrets to hide.
pub fn show(path: &Path) -> Result<()> {
    let (cfg, warnings) = load_with_warnings(path)?;
    for w in warnings {
        println!("# warning: {w}");
    }
    println!("# effective configuration (source: {})", path.display());
    println!("server_url = {:?}", cfg.server_url);
    println!("fingerprint = {:?}", cfg.fingerprint);
    println!("include_fingerprint = {}", cfg.include_fingerprint);
    println!("output_dir = {:?}", cfg.output_dir);
    println!("request_timeout_seconds = {}", cfg.request_timeout_seconds);
    println!("ta_timeout_seconds = {}", cfg.ta_timeout_seconds);
    println!("proxy = {:?}", cfg.proxy);
    println!("log_retention = {}", cfg.log_retention);
    println!("auto_issue_days = {}", cfg.auto_issue_days);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_cfg(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn defaults_when_missing() {
        let cfg = load(Path::new("/nonexistent/rkp-mini.toml")).unwrap();
        assert_eq!(cfg.server_url, DEFAULT_SERVER_URL);
        assert!(cfg.include_fingerprint);
        assert_eq!(cfg.log_retention, 10);
    }

    #[test]
    fn toml_overrides_defaults() {
        let f = tmp_cfg(
            "server_url = \"https://example.com/v2\"\ninclude_fingerprint = false\nrequest_timeout_seconds = 5\n",
        );
        let cfg = load(f.path()).unwrap();
        assert_eq!(cfg.server_url, "https://example.com/v2");
        assert!(!cfg.include_fingerprint);
        assert_eq!(cfg.request_timeout_seconds, 5);
        // untouched field keeps default
        assert_eq!(cfg.ta_timeout_seconds, 30);
    }

    #[test]
    fn url_normalization() {
        assert_eq!(
            effective_server_url("https://host").unwrap(),
            "https://host/v1"
        );
        assert_eq!(
            effective_server_url("https://host/").unwrap(),
            "https://host/v1"
        );
        assert_eq!(
            effective_server_url("https://host/v1/").unwrap(),
            "https://host/v1"
        );
        assert_eq!(
            effective_server_url("http://127.0.0.1:9/v1").unwrap(),
            "http://127.0.0.1:9/v1"
        );
    }

    #[test]
    fn url_rejects_non_https() {
        assert!(effective_server_url("http://example.com/v1").is_err());
        assert!(effective_server_url("ftp://x").is_err());
        assert!(effective_server_url("https://host/v1?x=1").is_err());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(load(tmp_cfg("request_timeout_seconds = 0\n").path()).is_err());
        assert!(load(tmp_cfg("request_timeout_seconds = 9999\n").path()).is_err());
        assert!(load(tmp_cfg("output_dir = \"relative/dir\"\n").path()).is_err());
        assert!(load(tmp_cfg("proxy = \"socks5://127.0.0.1:1080\"\n").path()).is_err());
    }

    #[test]
    fn unknown_keys_warn_not_fail() {
        let (cfg, warnings) =
            load_with_warnings(tmp_cfg("mystery = 1\nserver_url = \"https://h\"\n").path())
                .unwrap();
        assert_eq!(cfg.server_url, "https://h/v1");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("mystery"));
        // The rkp-sync section is not reported as unknown.
        let (_, warnings) =
            load_with_warnings(tmp_cfg("[sync]\nrole = \"sender\"\nenabled = false\n").path())
                .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
