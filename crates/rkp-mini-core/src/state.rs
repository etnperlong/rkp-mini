//! Local state files (design §7).
//!
//! - [`SyncState`]: what the sync layer last verified (receiver) or pushed
//!   (sender). It deliberately has **no** target/path/owner/reload fields.
//! - [`DeployState`]: what the deploy layer actually wrote, per target.
//! - [`Staged`]: `staging/latest.json`, the hand-off from `fetch` to `deploy`.
//!
//! All files are written atomically with mode 0600 inside 0700 directories.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{err, Stage};
use crate::fsutil;

pub const STATE_VERSION: u32 = 1;
const MAX_STATE_FILE: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastError {
    pub code: String,
    pub message: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncState {
    pub v: u32,
    pub provider: String,
    /// Public location (URL / endpoint+path). Never contains credentials.
    pub location: String,
    pub stream_id: String,
    /// Highest verified (receiver) or uploaded (sender) seq; 0 = none yet.
    pub verified_seq: u64,
    pub plaintext_sha256: Option<String>,
    pub keybox_sha256: Option<String>,
    pub device_id: Option<String>,
    pub keybox_not_after: Option<i64>,
    /// ETag / Gist `updated_at` / content hash from the last fetch.
    pub last_remote_token: Option<String>,
    pub last_checked_at: Option<i64>,
    pub verified_at: Option<i64>,
    pub last_error: Option<LastError>,
}

impl SyncState {
    pub fn new(provider: &str, location: &str, stream_id: &str) -> SyncState {
        SyncState {
            v: STATE_VERSION,
            provider: provider.to_string(),
            location: location.to_string(),
            stream_id: stream_id.to_string(),
            verified_seq: 0,
            plaintext_sha256: None,
            keybox_sha256: None,
            device_id: None,
            keybox_not_after: None,
            last_remote_token: None,
            last_checked_at: None,
            verified_at: None,
            last_error: None,
        }
    }

    /// Whether any version has been verified/pushed on this stream.
    pub fn has_verified(&self) -> bool {
        self.verified_seq > 0 && self.plaintext_sha256.is_some()
    }

    /// Previous plaintext digest as raw bytes (for `prev` comparisons).
    pub fn plaintext_digest(&self) -> Option<[u8; 32]> {
        let h = self.plaintext_sha256.as_deref()?;
        hex::decode(h).ok()?.try_into().ok()
    }

    pub fn set_error(&mut self, code: &str, message: &str, at: i64) {
        self.last_error = Some(LastError {
            code: code.to_string(),
            message: message.to_string(),
            at,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetState {
    pub id: String,
    pub path: String,
    pub owner: String,
    pub mode: String,
    pub deployed_sha256: Option<String>,
    pub deployed_at: Option<i64>,
    pub reload: String,
    pub reload_status: Option<String>,
    pub last_error: Option<LastError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub seq: u64,
    pub keybox_sha256: String,
    pub leaf_not_after: i64,
    /// Why it was held back (`identity_change`, `first_deploy`, ...).
    pub reason: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeployState {
    pub v: u32,
    pub stream_id: String,
    pub last_seen_keybox_sha256: Option<String>,
    pub last_seen_seq: Option<u64>,
    pub deployed_keybox_sha256: Option<String>,
    pub deployed_seq: Option<u64>,
    pub deployed_at: Option<i64>,
    /// Leaf `not_after` of the deployed keybox (drives the `scheduled` policy).
    pub deployed_leaf_not_after: Option<i64>,
    pub targets: Vec<TargetState>,
    pub pending: Option<Pending>,
}

impl DeployState {
    pub fn new(stream_id: &str) -> DeployState {
        DeployState {
            v: STATE_VERSION,
            stream_id: stream_id.to_string(),
            last_seen_keybox_sha256: None,
            last_seen_seq: None,
            deployed_keybox_sha256: None,
            deployed_seq: None,
            deployed_at: None,
            deployed_leaf_not_after: None,
            targets: Vec::new(),
            pending: None,
        }
    }
}

/// `staging/latest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Staged {
    pub v: u32,
    pub stream_id: String,
    pub seq: u64,
    pub keybox_sha256: String,
    pub plaintext_sha256: String,
    pub device_id: String,
    pub leaf_not_before: i64,
    pub leaf_not_after: i64,
    pub staged_at: i64,
}

impl Staged {
    /// Path of the staged keybox inside `staging_dir`.
    pub fn keybox_path(&self, staging_dir: &Path) -> PathBuf {
        staging_dir.join(format!("{}.xml", self.keybox_sha256))
    }
}

/// Load a JSON state file; `Ok(None)` when absent.
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let bytes = match fsutil::read_limited(path, MAX_STATE_FILE) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(err(
                Stage::State,
                "state_unreadable",
                format!("read {}: {e}", path.display()),
            ))
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|e| {
        err(
            Stage::State,
            "state_corrupt",
            format!("parse {}: {e}", path.display()),
        )
    })
}

/// Save a JSON state file atomically (0600; parent forced to 0700).
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let io = |e: std::io::Error| {
        err(
            Stage::State,
            "state_write",
            format!("write {}: {e}", path.display()),
        )
    };
    if let Some(dir) = path.parent() {
        fsutil::ensure_private_dir(dir).map_err(io)?;
    }
    let mut body = serde_json::to_vec_pretty(value)
        .map_err(|e| err(Stage::State, "state_write", format!("encode state: {e}")))?;
    body.push(b'\n');
    fsutil::atomic_write(path, &body, 0o600, None).map_err(io)
}

/// Write a verified keybox into `staging_dir` and point `latest.json` at it.
/// Older staged keyboxes are removed so at most one plaintext copy remains.
pub fn write_staging(staging_dir: &Path, staged: &Staged, keybox_xml: &[u8]) -> Result<PathBuf> {
    let io = |what: &str, e: std::io::Error| {
        err(
            Stage::State,
            "staging_write",
            format!("{what} {}: {e}", staging_dir.display()),
        )
    };
    fsutil::ensure_private_dir(staging_dir).map_err(|e| io("mkdir", e))?;
    let kb_path = staged.keybox_path(staging_dir);
    fsutil::atomic_write(&kb_path, keybox_xml, 0o600, None).map_err(|e| io("write", e))?;
    save(&staging_dir.join("latest.json"), staged)?;
    prune_staging(staging_dir, Some(&kb_path)).map_err(|e| io("prune", e))?;
    Ok(kb_path)
}

/// Remove staged keyboxes except `keep`. With `keep = None`, clears staging.
pub fn prune_staging(staging_dir: &Path, keep: Option<&Path>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(staging_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let p = entry?.path();
        let is_keybox = p.extension().is_some_and(|x| x == "xml");
        if (is_keybox && keep != Some(p.as_path()))
            || (keep.is_none() && p.ends_with("latest.json"))
        {
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn sync_state_has_no_deploy_fields() {
        let s = SyncState::new("gist", "https://example.com/x.age", "st_01JTESTSTREAM");
        let v = serde_json::to_value(&s).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        for banned in [
            "target",
            "target_path",
            "path",
            "owner",
            "mode",
            "reload",
            "targets",
        ] {
            assert!(
                !keys.contains(&banned),
                "sync-state must not contain `{banned}`"
            );
        }
        // And unknown (e.g. deploy) fields are rejected on load.
        let mut v = v;
        v["target_path"] = serde_json::json!("/data/x");
        assert!(serde_json::from_value::<SyncState>(v).is_err());
    }

    #[test]
    fn save_load_roundtrip_with_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sync").join("sync-state.json");
        assert!(load::<SyncState>(&p).unwrap().is_none());
        let mut s = SyncState::new("url", "https://example.com/x.age", "st_01JTESTSTREAM");
        s.verified_seq = 3;
        s.plaintext_sha256 = Some(hex::encode([9u8; 32]));
        save(&p, &s).unwrap();
        assert_eq!(load::<SyncState>(&p).unwrap().unwrap(), s);
        assert_eq!(std::fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            std::fs::metadata(p.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        assert_eq!(s.plaintext_digest(), Some([9u8; 32]));
    }

    #[test]
    fn corrupt_state_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.json");
        std::fs::write(&p, b"{not json").unwrap();
        let e = crate::error::to_sync_error(&load::<SyncState>(&p).unwrap_err());
        assert_eq!(e.code, "state_corrupt");
    }

    #[test]
    fn staging_keeps_only_latest() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let mk = |sha: &str, seq| Staged {
            v: 1,
            stream_id: "st_01JTESTSTREAM".into(),
            seq,
            keybox_sha256: sha.into(),
            plaintext_sha256: "p".into(),
            device_id: "d".into(),
            leaf_not_before: 0,
            leaf_not_after: 1,
            staged_at: 2,
        };
        let a = write_staging(&staging, &mk("aaaa", 1), b"A").unwrap();
        let b = write_staging(&staging, &mk("bbbb", 2), b"B").unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"B");
        assert_eq!(std::fs::metadata(&b).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&staging).unwrap().mode() & 0o777, 0o700);
        let latest: Staged = load(&staging.join("latest.json")).unwrap().unwrap();
        assert_eq!(latest.seq, 2);
        prune_staging(&staging, None).unwrap();
        assert_eq!(std::fs::read_dir(&staging).unwrap().count(), 0);
    }
}
