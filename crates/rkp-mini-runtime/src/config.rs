//! `rkp-sync` configuration (design §10.1, §11.1).
//!
//! One TOML file per role:
//! - sender: the shared `rkp-mini` config (`/data/misc/keystore/rkp-mini/config.toml`);
//!   `rkp-sync` reads top-level `output_dir` / `auto_issue_days` and owns `[sync]`.
//! - receiver: `/data/adb/rkp_sync/config.toml` with `[sync]` and `[deploy]`.
//!
//! The data root is the config file's directory. Secrets are referenced by
//! path only; TOML never carries a password or token value.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::locator::{self, Location, UrlPolicy};
use rkp_mini_core::{valid_stream_id, REMOTE_FILENAME};
use serde::Deserialize;

pub const SENDER_CONFIG: &str = "/data/misc/keystore/rkp-mini/config.toml";
pub const RECEIVER_CONFIG: &str = "/data/adb/rkp_sync/config.toml";
pub const SENDER_DEFAULT_OUTPUT: &str = "/data/misc/keystore/rkp-mini/output";

/// Current risk statement version (§13.2). Bump when `common::RISK_ITEMS` changes.
/// v2: adds the WebUI/debug-manager item.
pub const RISK_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Sender,
    Receiver,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Sender => "sender",
            Role::Receiver => "receiver",
        }
    }

    pub fn default_config(self) -> &'static str {
        match self {
            Role::Sender => SENDER_CONFIG,
            Role::Receiver => RECEIVER_CONFIG,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PasswordMode {
    SecretFile,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityPolicy {
    Scheduled,
    Always,
    Never,
}

impl IdentityPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityPolicy::Scheduled => "scheduled",
            IdentityPolicy::Always => "always",
            IdentityPolicy::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncSection {
    pub role: Option<Role>,
    pub enabled: bool,
    pub risk_ack_version: u32,
    pub risk_ack_at: i64,
    pub provider: String,
    pub stream_id: String,
    pub label: String,
    pub min_validity_days: Option<u32>,
    pub request_timeout_seconds: u64,
    pub proxy: String,

    pub gist_id: String,
    pub gist_filename: String,
    pub gist_url: String,
    pub gist_token_file: String,

    pub webdav_endpoint: String,
    pub webdav_path: String,
    pub webdav_user: String,
    pub webdav_password_file: String,

    pub url: String,

    pub password_mode: PasswordMode,
    pub password_file: String,
}

impl Default for SyncSection {
    fn default() -> Self {
        SyncSection {
            role: None,
            enabled: false,
            risk_ack_version: 0,
            risk_ack_at: 0,
            provider: String::new(),
            stream_id: String::new(),
            label: String::new(),
            min_validity_days: None,
            request_timeout_seconds: 30,
            proxy: String::new(),
            gist_id: String::new(),
            gist_filename: REMOTE_FILENAME.to_string(),
            gist_url: String::new(),
            gist_token_file: String::new(),
            webdav_endpoint: String::new(),
            webdav_path: String::new(),
            webdav_user: String::new(),
            webdav_password_file: String::new(),
            url: String::new(),
            password_mode: PasswordMode::SecretFile,
            password_file: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetConfig {
    pub id: String,
    pub path: Option<String>,
    pub owner: Option<String>,
    pub mode: Option<String>,
    pub reload: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeploySection {
    pub identity_change_policy: IdentityPolicy,
    pub rotation_lead_days: u32,
    pub auto_deploy: bool,
    pub targets: Vec<TargetConfig>,
}

impl Default for DeploySection {
    fn default() -> Self {
        DeploySection {
            identity_change_policy: IdentityPolicy::Scheduled,
            rotation_lead_days: 30,
            auto_deploy: true,
            targets: Vec::new(),
        }
    }
}

/// Raw file shape. Sender files also contain `rkp-mini` keys, which are
/// accepted and ignored here except for `output_dir` / `auto_issue_days`.
#[derive(Debug, Default, Deserialize)]
struct RawFile {
    output_dir: Option<String>,
    auto_issue_days: Option<u32>,
    #[serde(default)]
    sync: Option<SyncSection>,
    #[serde(default)]
    deploy: Option<DeploySection>,
}

/// Fully resolved configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub root: PathBuf,
    pub role: Role,
    pub sync: SyncSection,
    pub deploy: DeploySection,
    /// Sender: directory holding `keybox.xml` / `meta.json`.
    pub output_dir: PathBuf,
    /// Sender: `rkp-mini` issuance threshold (0 = off).
    pub auto_issue_days: u32,
    /// From CLI only: debugging relaxations for URL policy.
    pub url_policy: UrlPolicy,
}

fn cfg_err(msg: impl Into<String>) -> anyhow::Error {
    err(Stage::Config, "config_invalid", msg)
}

/// Resolve which config file to use.
///
/// `--config` wins; otherwise the role's default; with neither, whichever
/// default file exists (an ambiguous choice is an error).
pub fn resolve_path(explicit: Option<&Path>, role: Option<Role>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(r) = role {
        return Ok(PathBuf::from(r.default_config()));
    }
    let sender = has_sync_section(Path::new(SENDER_CONFIG));
    let receiver = Path::new(RECEIVER_CONFIG).exists();
    match (sender, receiver) {
        (true, false) => Ok(PathBuf::from(SENDER_CONFIG)),
        (false, true) => Ok(PathBuf::from(RECEIVER_CONFIG)),
        (true, true) => Err(err(
            Stage::Config,
            "role_ambiguous",
            "both sender and receiver configs exist; pass --role or --config",
        )),
        (false, false) => Err(err(
            Stage::Config,
            "not_initialized",
            "no rkp-sync configuration found; run `rkp-sync init --role sender|receiver`",
        )),
    }
}

fn has_sync_section(p: &Path) -> bool {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .is_some_and(|t| t.contains_key("sync"))
}

/// Load and validate. `expect` enforces the command's role.
pub fn load(path: &Path, expect: Option<Role>, url_policy: UrlPolicy) -> Result<Config> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            err(
                Stage::Config,
                "not_initialized",
                format!(
                    "{} not found; run `rkp-sync init --role {}`",
                    path.display(),
                    expect.map_or("sender|receiver", Role::as_str)
                ),
            )
        } else {
            cfg_err(format!("read {}: {e}", path.display()))
        }
    })?;
    let raw: RawFile =
        toml::from_str(&text).map_err(|e| cfg_err(format!("{}: {e}", path.display())))?;
    let sync = raw.sync.ok_or_else(|| {
        err(
            Stage::Config,
            "not_initialized",
            format!(
                "{} has no [sync] section; run `rkp-sync init`",
                path.display()
            ),
        )
    })?;
    let role = sync
        .role
        .ok_or_else(|| cfg_err("[sync].role must be \"sender\" or \"receiver\""))?;
    if let Some(want) = expect {
        if role != want {
            return Err(err(
                Stage::Config,
                "wrong_role",
                format!(
                    "this command needs a {} configuration, but {} is a {}",
                    want.as_str(),
                    path.display(),
                    role.as_str()
                ),
            ));
        }
    }
    if role == Role::Sender && raw.deploy.is_some() {
        return Err(cfg_err(
            "[deploy] is only valid in a receiver configuration",
        ));
    }
    let root = path
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or_else(|| cfg_err("config path must be absolute"))?
        .to_path_buf();
    let output_dir = PathBuf::from(
        raw.output_dir
            .unwrap_or_else(|| SENDER_DEFAULT_OUTPUT.to_string()),
    );
    let mut cfg = Config {
        path: path.to_path_buf(),
        root,
        role,
        sync,
        deploy: raw.deploy.unwrap_or_default(),
        output_dir,
        auto_issue_days: raw.auto_issue_days.unwrap_or(0),
        url_policy,
    };
    cfg.validate()?;
    Ok(cfg)
}

fn check_abs(p: &str, what: &str) -> Result<()> {
    if !p.is_empty() && !Path::new(p).is_absolute() {
        return Err(cfg_err(format!("{what} must be an absolute path")));
    }
    Ok(())
}

impl Config {
    fn validate(&mut self) -> Result<()> {
        let s = &self.sync;
        if !s.stream_id.is_empty() && !valid_stream_id(&s.stream_id) {
            return Err(cfg_err("[sync].stream_id has invalid format"));
        }
        if !s.label.is_empty() {
            locator::check_label(&s.label)?;
        }
        if !(1..=300).contains(&s.request_timeout_seconds) {
            return Err(cfg_err("[sync].request_timeout_seconds must be 1..=300"));
        }
        let proxy_ok =
            s.proxy.is_empty() || s.proxy.starts_with("http://") || s.proxy.starts_with("https://");
        if !proxy_ok {
            return Err(cfg_err("[sync].proxy must be empty or http(s)://"));
        }
        match (self.role, s.provider.as_str()) {
            (_, "") | (_, "gist") | (_, "webdav") | (Role::Receiver, "url") => {}
            (Role::Sender, "url") => {
                return Err(cfg_err(
                    "provider \"url\" is read-only; a sender must use gist or webdav",
                ))
            }
            (_, other) => return Err(cfg_err(format!("unknown provider `{other}`"))),
        }
        locator::check_filename(&s.gist_filename)?;
        for (p, what) in [
            (&s.gist_token_file, "gist_token_file"),
            (&s.webdav_password_file, "webdav_password_file"),
            (&s.password_file, "password_file"),
        ] {
            check_abs(p, what)?;
        }
        if self.role == Role::Sender && !self.output_dir.is_absolute() {
            return Err(cfg_err("output_dir must be absolute"));
        }
        if let Some(d) = s.min_validity_days {
            if d > 365 {
                return Err(cfg_err("[sync].min_validity_days must be 0..=365"));
            }
        }
        if self.deploy.rotation_lead_days > 365 {
            return Err(cfg_err("[deploy].rotation_lead_days must be 0..=365"));
        }
        Ok(())
    }

    /// Sync working directory (`<root>/sync`).
    pub fn sync_dir(&self) -> PathBuf {
        self.root.join("sync")
    }

    pub fn sync_state_path(&self) -> PathBuf {
        self.sync_dir().join("sync-state.json")
    }

    pub fn deploy_state_path(&self) -> PathBuf {
        self.root.join("deploy").join("deploy-state.json")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.sync_dir().join("sync.lock")
    }

    /// WebUI secret inbox (§19.2).
    pub fn inbox_dir(&self) -> PathBuf {
        self.sync_dir().join("inbox")
    }

    /// pid file written by the module's `sync-loop.sh`.
    pub fn loop_pid_path(&self) -> PathBuf {
        self.root.join("tmp").join("sync-loop.pid")
    }

    fn secrets_dir(&self) -> PathBuf {
        match self.role {
            Role::Sender => self.sync_dir(),
            Role::Receiver => self.root.join("secrets"),
        }
    }

    fn secret_path(&self, configured: &str, default_name: &str) -> PathBuf {
        if configured.is_empty() {
            self.secrets_dir().join(default_name)
        } else {
            PathBuf::from(configured)
        }
    }

    pub fn password_file(&self) -> PathBuf {
        self.secret_path(&self.sync.password_file, "age.pass")
    }

    pub fn gist_token_file(&self) -> PathBuf {
        self.secret_path(&self.sync.gist_token_file, "gist.token")
    }

    pub fn webdav_password_file(&self) -> PathBuf {
        self.secret_path(&self.sync.webdav_password_file, "webdav.pass")
    }

    pub fn min_validity_days(&self) -> u32 {
        self.sync.min_validity_days.unwrap_or(match self.role {
            Role::Sender => 14,
            Role::Receiver => 7,
        })
    }

    pub fn keybox_path(&self) -> PathBuf {
        self.output_dir.join("keybox.xml")
    }

    pub fn meta_path(&self) -> PathBuf {
        self.output_dir.join("meta.json")
    }

    /// Network operations require `enabled` and a current risk acknowledgement.
    pub fn require_enabled(&self) -> Result<()> {
        if self.sync.risk_ack_version < RISK_VERSION {
            return Err(err(
                Stage::Config,
                "risk_not_acknowledged",
                "remote keybox sync needs explicit risk acceptance; run `rkp-sync enable`",
            ));
        }
        if !self.sync.enabled {
            return Err(err(
                Stage::Config,
                "sync_disabled",
                "sync is disabled; run `rkp-sync enable`",
            ));
        }
        Ok(())
    }

    pub fn require_stream(&self) -> Result<&str> {
        if self.sync.stream_id.is_empty() {
            return Err(err(
                Stage::Config,
                "stream_missing",
                match self.role {
                    Role::Sender => {
                        "[sync].stream_id is empty; run `rkp-sync init --role sender`"
                    }
                    Role::Receiver => {
                        "no stream imported; run `rkp-sync import '<rkpsync://…>'`"
                    }
                },
            ));
        }
        Ok(&self.sync.stream_id)
    }

    /// Remote location derived from `[sync]`, validated with the URL policy.
    pub fn location(&self) -> Result<Location> {
        let s = &self.sync;
        let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
        let loc = match s.provider.as_str() {
            "gist" => Location::Gist {
                url: opt(&s.gist_url),
                gist_id: opt(&s.gist_id),
                filename: s.gist_filename.clone(),
            },
            "webdav" => Location::Webdav {
                endpoint: s.webdav_endpoint.clone(),
                path: s.webdav_path.clone(),
                user: opt(&s.webdav_user),
            },
            "url" => Location::Url { url: s.url.clone() },
            "" => {
                return Err(err(
                    Stage::Config,
                    "provider_missing",
                    "[sync].provider is empty",
                ))
            }
            other => return Err(cfg_err(format!("unknown provider `{other}`"))),
        };
        if self.role == Role::Sender {
            if let Location::Gist {
                url: None,
                gist_id: None,
                filename,
            } = loc
            {
                // A sender may not have created its gist yet.
                return Ok(Location::Gist {
                    url: None,
                    gist_id: None,
                    filename,
                });
            }
        }
        loc.validated(self.url_policy)
    }
}

// ---------------------------------------------------------------------------
// Editing (comment-preserving via toml_edit)
// ---------------------------------------------------------------------------

/// A typed value to write into `[sync]`.
pub enum Value<'a> {
    Str(&'a str),
    Int(i64),
    Bool(bool),
}

/// Set keys in a table (created if missing; `""` = top level) and write the
/// file atomically (0600).
pub fn edit_table(path: &Path, table: &str, values: &[(&str, Value)]) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(cfg_err(format!("read {}: {e}", path.display()))),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| cfg_err(format!("{}: {e}", path.display())))?;
    if !table.is_empty() && !doc.contains_key(table) {
        doc[table] = toml_edit::table();
    }
    let t = if table.is_empty() {
        doc.as_table_mut()
    } else {
        doc[table]
            .as_table_mut()
            .ok_or_else(|| cfg_err(format!("[{table}] is not a table")))?
    };
    for (k, v) in values {
        let item = match v {
            Value::Str(s) => toml_edit::value(*s),
            Value::Int(i) => toml_edit::value(*i),
            Value::Bool(b) => toml_edit::value(*b),
        };
        match t.get_mut(k) {
            // Keep any trailing comment on an existing line.
            Some(existing) if existing.is_value() => {
                let decor = existing.as_value().map(|v| v.decor().clone());
                *existing = item;
                if let (Some(d), Some(v)) = (decor, existing.as_value_mut()) {
                    *v.decor_mut() = d;
                }
            }
            _ => {
                t.insert(k, item);
            }
        }
    }
    write_config(path, doc.to_string().as_bytes())
}

pub fn write_config(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        rkp_mini_core::fsutil::ensure_private_dir(dir).map_err(|e| {
            err(
                Stage::State,
                "config_write",
                format!("mkdir {}: {e}", dir.display()),
            )
        })?;
    }
    rkp_mini_core::fsutil::atomic_write(path, bytes, 0o600, None).map_err(|e| {
        err(
            Stage::State,
            "config_write",
            format!("write {}: {e}", path.display()),
        )
    })
}

/// Append a `[[deploy.targets]]` entry (comment-preserving).
pub fn add_target(path: &Path, id: &str, fields: &[(&str, &str)]) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| cfg_err(format!("read {}: {e}", path.display())))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| cfg_err(format!("{}: {e}", path.display())))?;
    if !doc.contains_key("deploy") {
        doc["deploy"] = toml_edit::table();
    }
    let deploy = doc["deploy"]
        .as_table_mut()
        .ok_or_else(|| cfg_err("[deploy] is not a table"))?;
    if !deploy.contains_key("targets") {
        deploy.insert(
            "targets",
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
        );
    }
    let arr = deploy["targets"]
        .as_array_of_tables_mut()
        .ok_or_else(|| cfg_err("[deploy].targets must be [[deploy.targets]] tables"))?;
    if arr
        .iter()
        .any(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))
    {
        return Err(err(
            Stage::Config,
            "target_exists",
            format!("target `{id}` is already configured"),
        ));
    }
    let mut t = toml_edit::Table::new();
    t.insert("id", toml_edit::value(id));
    for (k, v) in fields {
        t.insert(k, toml_edit::value(*v));
    }
    arr.push(t);
    write_config(path, doc.to_string().as_bytes())
}

/// Remove a `[[deploy.targets]]` entry by id.
pub fn remove_target(path: &Path, id: &str) -> Result<bool> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| cfg_err(format!("read {}: {e}", path.display())))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| cfg_err(format!("{}: {e}", path.display())))?;
    let Some(arr) = doc
        .get_mut("deploy")
        .and_then(|d| d.get_mut("targets"))
        .and_then(|t| t.as_array_of_tables_mut())
    else {
        return Ok(false);
    };
    let before = arr.len();
    arr.retain(|t| t.get("id").and_then(|v| v.as_str()) != Some(id));
    let removed = arr.len() != before;
    if removed {
        write_config(path, doc.to_string().as_bytes())?;
    }
    Ok(removed)
}

pub fn sender_sync_template(stream_id: &str, provider: &str, root: &Path) -> String {
    let sync = root.join("sync");
    format!(
        r#"
# Remote keybox sync (rkp-sync). Disabled by default; enabling requires
# `rkp-sync enable` (explicit risk acceptance). Secrets live in 0600 files;
# never put a password or token value in this file.
[sync]
role = "sender"
enabled = false
risk_ack_version = 0
provider = "{provider}"              # gist | webdav
stream_id = "{stream_id}"
label = ""                        # shown to the receiver; no serials/IMEI/e-mail
min_validity_days = 14
request_timeout_seconds = 30
proxy = ""                        # e.g. "http://127.0.0.1:7890"; empty = direct

# Gist (secret gist; created on first push)
gist_id = ""
gist_filename = "rkp-sync.age"
gist_url = ""
gist_token_file = "{gist}"        # PAT with `gist` scope only, 0600

# WebDAV
webdav_endpoint = ""              # e.g. "https://dav.jianguoyun.com/dav/"
webdav_path = "/rkp/rkp-sync.age"
webdav_user = ""
webdav_password_file = "{webdav}" # app password, 0600

# age passphrase
password_mode = "secret-file"     # secret-file | prompt
password_file = "{age}"           # 0600
"#,
        gist = sync.join("gist.token").display(),
        webdav = sync.join("webdav.pass").display(),
        age = sync.join("age.pass").display(),
    )
}

pub fn receiver_template(root: &Path) -> String {
    let secrets = root.join("secrets");
    format!(
        r#"# rkp-sync receiver configuration.
# The remote location is written by `rkp-sync import`; deployment targets are
# local-only and are never taken from the remote side.
[sync]
role = "receiver"
enabled = false
risk_ack_version = 0
provider = ""                     # gist | webdav | url (set by import)
stream_id = ""
label = ""
min_validity_days = 7
request_timeout_seconds = 30
proxy = ""

# Gist (direct raw link; no token needed)
gist_url = ""
gist_id = ""
gist_filename = "rkp-sync.age"

# WebDAV
webdav_endpoint = ""
webdav_path = ""
webdav_user = ""
webdav_password_file = "{webdav}" # 0600

# Public HTTPS file
url = ""

# age passphrase
password_mode = "secret-file"     # secret-file | prompt
password_file = "{age}"           # 0600

[deploy]
identity_change_policy = "scheduled"  # scheduled | always | never
rotation_lead_days = 30
auto_deploy = true

# Targets are added with `rkp-sync deploy add-target omk|teesim|tricky_store`
# or by hand, e.g.:
# [[deploy.targets]]
# id = "omk"
# path = "/data/misc/keystore/omk/keybox.xml"
# owner = "1017:1017"
# mode = "0600"
# reload = "auto"
"#,
        webdav = secrets.join("webdav.pass").display(),
        age = secrets.join("age.pass").display(),
    )
}

/// New random stream id: `st_` + 26 Crockford base32 chars (128 bits).
pub fn new_stream_id() -> String {
    use rand_core::{OsRng, RngCore};
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bytes = [0u8; 17];
    OsRng.fill_bytes(&mut bytes);
    let mut out = String::from("st_");
    let mut acc: u32 = 0;
    let mut bits = 0;
    for b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 && out.len() < 3 + 26 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn stream_ids_are_valid_and_unique() {
        let a = new_stream_id();
        assert!(valid_stream_id(&a), "{a}");
        assert_eq!(a.len(), 29);
        assert_ne!(a, new_stream_id());
    }

    #[test]
    fn templates_parse_disabled_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let mini = "output_dir = \"/data/misc/keystore/rkp-mini/output\"\nauto_issue_days = 0\n";
        let body = format!(
            "{mini}{}",
            sender_sync_template(&new_stream_id(), "gist", dir.path())
        );
        let p = write(dir.path(), "config.toml", &body);
        let c = load(&p, Some(Role::Sender), UrlPolicy::default()).unwrap();
        assert!(!c.sync.enabled);
        assert_eq!(c.min_validity_days(), 14);
        assert!(c.require_enabled().is_err());

        let r = write(dir.path(), "recv.toml", &receiver_template(dir.path()));
        let c = load(&r, Some(Role::Receiver), UrlPolicy::default()).unwrap();
        assert_eq!(c.deploy.identity_change_policy, IdentityPolicy::Scheduled);
        assert!(c.deploy.targets.is_empty());
        assert_eq!(c.password_file(), dir.path().join("secrets/age.pass"));
        assert_eq!(c.min_validity_days(), 7);
        assert_eq!(
            load(&r, Some(Role::Sender), UrlPolicy::default())
                .map_err(|e| rkp_mini_core::error::to_sync_error(&e).code)
                .unwrap_err(),
            "wrong_role"
        );
    }

    #[test]
    fn enabled_without_ack_and_secret_values_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(
            dir.path(),
            "c.toml",
            "[sync]\nrole = \"receiver\"\nenabled = true\nrisk_ack_version = 0\n",
        );
        // Loads (so `enable`/`disable` still work) but every network/deploy
        // operation is refused until the risk statement is accepted.
        let c = load(&p, None, UrlPolicy::default()).unwrap();
        let e = c.require_enabled().unwrap_err();
        assert_eq!(
            rkp_mini_core::error::to_sync_error(&e).code,
            "risk_not_acknowledged"
        );
        // Unknown keys (e.g. an inline password) are rejected, not ignored.
        let p = write(
            dir.path(),
            "c2.toml",
            "[sync]\nrole = \"receiver\"\npassword = \"hunter2\"\n",
        );
        assert!(load(&p, None, UrlPolicy::default()).is_err());
    }

    #[test]
    fn edit_preserves_comments_and_adds_targets() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "c.toml", &receiver_template(dir.path()));
        edit_table(
            &p,
            "sync",
            &[
                ("enabled", Value::Bool(true)),
                ("risk_ack_version", Value::Int(RISK_VERSION as i64)),
            ],
        )
        .unwrap();
        add_target(&p, "omk", &[("path", "/data/misc/keystore/omk/keybox.xml")]).unwrap();
        assert!(add_target(&p, "omk", &[]).is_err());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# scheduled | always | never"));
        let c = load(&p, None, UrlPolicy::default()).unwrap();
        assert!(c.sync.enabled);
        assert_eq!(c.deploy.targets.len(), 1);
        assert!(remove_target(&p, "omk").unwrap());
        let c = load(&p, None, UrlPolicy::default()).unwrap();
        assert!(c.deploy.targets.is_empty());
    }
}
