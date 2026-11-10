//! Structured error model shared by `rkp-mini-sync` and `rkp-mini-deployer`.
//!
//! Stages map to the JSON `error.stage` field (appendix B) and to process exit
//! codes. Messages MUST NOT contain secrets (passwords, tokens, key material);
//! callers are responsible for keeping diagnostics free of plaintext.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Configuration / usage / missing risk acknowledgement.
    Config,
    /// Remote fetch/upload (network, HTTP status, SSRF guard).
    Remote,
    /// age decryption failure (wrong password, tampering, truncation).
    Decrypt,
    /// CBOR / keybox / seq / stream validation.
    Validate,
    /// Local sync/deploy state read/write or replay rejection.
    State,
    /// Target deployment (write, chown, reload).
    Deploy,
    /// Anything else.
    Generic,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Config => "config",
            Stage::Remote => "remote",
            Stage::Decrypt => "decrypt",
            Stage::Validate => "validate",
            Stage::State => "state",
            Stage::Deploy => "deploy",
            Stage::Generic => "generic",
        }
    }

    /// Process exit code (appendix B).
    pub fn exit_code(self) -> i32 {
        match self {
            Stage::Config => 2,
            Stage::Remote | Stage::Decrypt | Stage::Validate | Stage::State => 8,
            Stage::Deploy => 9,
            Stage::Generic => 1,
        }
    }
}

/// Error carrying a stage and a machine-readable code.
#[derive(Debug, Clone)]
pub struct SyncError {
    pub stage: Stage,
    pub code: String,
    pub message: String,
}

impl SyncError {
    pub fn new(stage: Stage, code: &str, message: impl Into<String>) -> Self {
        SyncError {
            stage,
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.stage.exit_code()
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}:{}] {}",
            self.stage.as_str(),
            self.code,
            self.message
        )
    }
}

impl std::error::Error for SyncError {}

/// Build a [`SyncError`] as an `anyhow::Error`, preserving downcast.
pub fn err(stage: Stage, code: &str, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(SyncError::new(stage, code, message))
}

/// Resolve any error into a reportable [`SyncError`] (downcast or generic wrap).
pub fn to_sync_error(e: &anyhow::Error) -> SyncError {
    if let Some(se) = e.downcast_ref::<SyncError>() {
        return se.clone();
    }
    SyncError::new(Stage::Generic, "error", format!("{e:#}"))
}
