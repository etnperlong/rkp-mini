//! Structured error model: stage + code drive JSON envelope and process exit code.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Config,
    Prereq,
    Challenge,
    Ta,
    Csr,
    Server,
    Chain,
    Write,
    Generic,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Config => "config",
            Stage::Prereq => "prereq",
            Stage::Challenge => "challenge",
            Stage::Ta => "ta",
            Stage::Csr => "csr",
            Stage::Server => "server",
            Stage::Chain => "chain",
            Stage::Write => "write",
            Stage::Generic => "generic",
        }
    }

    /// Process exit code per spec §5.4.
    pub fn exit_code(self) -> i32 {
        match self {
            Stage::Generic => 1,
            Stage::Config => 2,
            Stage::Prereq => 3,
            Stage::Challenge | Stage::Server => 4,
            Stage::Ta | Stage::Csr => 5,
            Stage::Chain => 6,
            Stage::Write => 7,
        }
    }
}

/// Error carrying stage, machine code, and optional TA diagnostics.
#[derive(Debug, Clone)]
pub struct AppError {
    pub stage: Stage,
    pub code: String,
    pub message: String,
    pub ta_status: Option<i32>,
    pub head_hex: Option<String>,
}

impl AppError {
    pub fn new(stage: Stage, code: &str, message: impl Into<String>) -> Self {
        AppError {
            stage,
            code: code.to_string(),
            message: message.into(),
            ta_status: None,
            head_hex: None,
        }
    }

    pub fn with_ta_status(mut self, status: i32) -> Self {
        self.ta_status = Some(status);
        self
    }

    pub fn with_head_hex(mut self, head_hex: impl Into<String>) -> Self {
        self.head_hex = Some(head_hex.into());
        self
    }

    pub fn exit_code(&self) -> i32 {
        self.stage.exit_code()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.stage.as_str(), self.message)?;
        if let Some(s) = self.ta_status {
            write!(f, " (ta_status={s})")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

/// Build an [`AppError`] as an `anyhow::Error`, preserving downcast.
pub fn err(stage: Stage, code: &str, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(AppError::new(stage, code, message))
}

/// Resolve any error into a reportable [`AppError`] (downcast or generic wrap).
pub fn to_app_error(e: &anyhow::Error) -> AppError {
    if let Some(app) = e.downcast_ref::<AppError>() {
        return app.clone();
    }
    AppError::new(Stage::Generic, "error", format!("{e:#}"))
}
