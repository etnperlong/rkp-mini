//! `rkp-sync-core`: everything the sync protocol needs that is independent of
//! the network stack and of Android.
//!
//! - [`envelope`]: age v1 passphrase encryption with ASCII armor.
//! - [`manifest`]: canonical CBOR plaintext (manifest + keybox bytes).
//! - [`keybox`]: strict keybox XML validation (no DTD, chain/key/DeviceID checks).
//! - [`locator`]: `rkpsync://v1/import` URL scheme parsing and URL policy.
//! - [`state`]: `sync-state.json` / `deploy-state.json` / staging metadata.
//! - [`transport`]: provider-neutral fetch/put trait plus a local directory transport.
//! - [`protocol`]: push base selection and receiver replay/rollback rules.
//! - [`secret`]: secret-file loading (permission checks, newline handling).
//! - [`fsutil`]: atomic, permission-preserving file writes.
//!
//! The crate is synchronous and performs no network I/O itself.

pub mod envelope;
pub mod error;
pub mod fsutil;
pub mod keybox;
pub mod locator;
pub mod manifest;
pub mod protocol;
pub mod secret;
pub mod state;
pub mod transport;

#[cfg(test)]
pub(crate) mod test_support;

use sha2::{Digest, Sha256};

/// Sync protocol version carried in the manifest, URL scheme and state files.
pub const PROTOCOL_VERSION: u64 = 1;

/// Fixed remote file name for Gist streams (and the WebDAV default).
pub const REMOTE_FILENAME: &str = "rkp-sync.age";

/// Size limits from the design (§6.3). Everything above is rejected outright.
pub mod limits {
    /// Whole armored age file.
    pub const MAX_ENVELOPE: usize = 256 * 1024;
    /// Decrypted CBOR plaintext.
    pub const MAX_PLAINTEXT: usize = 192 * 1024;
    /// `keybox_xml` bytes.
    pub const MAX_KEYBOX_XML: usize = 128 * 1024;
    /// `meta_json` bytes.
    pub const MAX_META_JSON: usize = 16 * 1024;
    /// URL-scheme JSON after base64url decoding.
    pub const MAX_IMPORT_JSON: usize = 4 * 1024;
    /// Whole `rkpsync://` URI.
    pub const MAX_IMPORT_URI: usize = 2 * 1024;
    /// Any single HTTP response body.
    pub const MAX_RESPONSE: usize = 256 * 1024;
}

/// SHA-256 digest.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Lowercase hex SHA-256 digest.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha256(bytes))
}

/// Validate a stream id: `st_` followed by 8..=60 chars of `[A-Za-z0-9_-]`.
pub fn valid_stream_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("st_") else {
        return false;
    };
    (8..=60).contains(&rest.len())
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Seconds per day.
pub const DAY: i64 = 86_400;

/// Earliest plausible wall clock (2020-01-01T00:00:00Z); below = `clock_invalid`.
pub const MIN_PLAUSIBLE_NOW: i64 = 1_577_836_800;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_rules() {
        assert!(valid_stream_id("st_01JABCDEFGH"));
        assert!(valid_stream_id("st_abc-def_123"));
        assert!(!valid_stream_id("st_abcdefg"));
        assert!(!valid_stream_id("xx_01JABCDEFGH"));
        assert!(!valid_stream_id("st_01JABC/DEFGH"));
        assert!(!valid_stream_id(&format!("st_{}", "a".repeat(61))));
    }
}
