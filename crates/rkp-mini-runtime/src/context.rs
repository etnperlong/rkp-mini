//! CLI globals and the command context.

use std::path::PathBuf;

use anyhow::Result;
use rkp_mini_core::locator::UrlPolicy;

use crate::config::{self, Config, Role};
use crate::tty;

/// Globals accepted anywhere on the command line.
#[derive(Debug, Default, Clone)]
pub struct Global {
    pub config: Option<PathBuf>,
    pub role: Option<Role>,
    pub json: bool,
    pub allow_insecure_http: bool,
    pub allow_private: bool,
}

pub struct Ctx {
    pub global: Global,
}

impl Ctx {
    pub fn url_policy(&self) -> UrlPolicy {
        UrlPolicy {
            allow_insecure_http: self.global.allow_insecure_http,
            allow_private: self.global.allow_private,
        }
    }

    pub fn config_path(&self, role: Option<Role>) -> Result<PathBuf> {
        config::resolve_path(self.global.config.as_deref(), role.or(self.global.role))
    }

    pub fn load(&self, role: Option<Role>) -> Result<Config> {
        let path = self.config_path(role)?;
        config::load(&path, role.or(self.global.role), self.url_policy())
    }

    /// Interactive prompts are allowed (TTY, not JSON, not automation).
    pub fn interactive(&self, auto: bool) -> bool {
        !auto && !self.global.json && tty::stdin_is_tty()
    }
}
