//! Provider-neutral remote access.
//!
//! A [`Transport`] moves opaque envelope bytes; it never sees plaintext.
//! Implementations: [`DirTransport`] (local directory, used for tests and
//! sneakernet), and the HTTP providers in the `rkp-sync` binary.

use std::path::PathBuf;

use anyhow::Result;

use crate::error::{err, Stage};
use crate::{fsutil, limits, sha256_hex};

/// Result of reading the remote envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    /// Nothing stored at the location yet.
    Missing,
    /// Conditional request matched `if_none_match`; body unchanged.
    NotModified,
    Body {
        bytes: Vec<u8>,
        /// Change token (ETag, Gist `updated_at`, content hash) if available.
        token: Option<String>,
    },
}

pub trait Transport {
    /// `gist` / `webdav` / `url` / `dir`.
    fn provider(&self) -> &'static str;

    /// Public, credential-free location string.
    fn location(&self) -> String;

    /// Read the envelope. `if_none_match` is a token from a previous fetch.
    fn fetch(&self, if_none_match: Option<&str>) -> Result<Fetch>;

    /// Replace the envelope. `expected` is the token observed by the preceding
    /// fetch; providers with compare-and-swap reject the write if it changed.
    fn put(&self, body: &[u8], expected: Option<&str>) -> Result<()>;

    /// Read back after `put` for verification. Providers whose normal read path
    /// may be cached (e.g. Gist raw CDN) override this with an uncached read.
    fn fetch_fresh(&self) -> Result<Fetch> {
        self.fetch(None)
    }
}

/// Envelope stored as a single file on the local filesystem.
#[derive(Debug, Clone)]
pub struct DirTransport {
    pub path: PathBuf,
}

impl Transport for DirTransport {
    fn provider(&self) -> &'static str {
        "dir"
    }

    fn location(&self) -> String {
        self.path.display().to_string()
    }

    fn fetch(&self, if_none_match: Option<&str>) -> Result<Fetch> {
        let bytes = match fsutil::read_limited(&self.path, limits::MAX_RESPONSE) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Fetch::Missing),
            Err(e) => {
                return Err(err(
                    Stage::Remote,
                    "remote_read",
                    format!("read {}: {e}", self.path.display()),
                ))
            }
        };
        let token = sha256_hex(&bytes);
        if if_none_match == Some(token.as_str()) {
            return Ok(Fetch::NotModified);
        }
        Ok(Fetch::Body {
            bytes,
            token: Some(token),
        })
    }

    fn put(&self, body: &[u8], expected: Option<&str>) -> Result<()> {
        if let Some(exp) = expected {
            if let Fetch::Body { token, .. } = self.fetch(None)? {
                if token.as_deref() != Some(exp) {
                    return Err(err(
                        Stage::Remote,
                        "remote_conflict",
                        "remote changed since it was read",
                    ));
                }
            }
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                err(
                    Stage::Remote,
                    "remote_write",
                    format!("mkdir {}: {e}", dir.display()),
                )
            })?;
        }
        fsutil::atomic_write(&self.path, body, 0o600, None).map_err(|e| {
            err(
                Stage::Remote,
                "remote_write",
                format!("write {}: {e}", self.path.display()),
            )
        })
    }
}
