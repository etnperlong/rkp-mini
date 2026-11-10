//! `rkp-mini-runtime`: configuration, command context and local helpers shared
//! by the sync CLI and the deployer.
//!
//! - [`config`]: the TOML file, role selection, templates, comment-preserving
//!   edits and validation (no network, no `age`).
//! - [`targets`]: local deployment target parsing/validation.
//! - [`context`]: `Global` CLI globals and the command `Ctx`.
//! - [`output`]: one result envelope per command plus the exit-code mapping.
//! - [`lock`]: the exclusive non-blocking data lock.
//! - [`cli`]: option parsing shared by the two parsers.
//! - [`tty`]: interactive prompts and confirmations.
//! - [`util`]: clock, UTC date and module-loop detection.
//!
//! Nothing here performs network I/O, links `age`, or touches vendor ABI code.

pub mod cli;
pub mod config;
pub mod context;
pub mod lock;
pub mod output;
pub mod targets;
pub mod tty;
pub mod util;

pub use context::{Ctx, Global};
pub use output::Output;
