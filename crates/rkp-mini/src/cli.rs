//! Hand-written argument parsing (no clap, keep binary small).

use std::path::PathBuf;

use anyhow::{bail, Result};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
rkp-mini — Qualcomm KeyMint CSR_V2 (0x3122) RKP keybox issuer

USAGE:
  rkp-mini status [--json] [--update-description] [--quiet] [--config PATH]
  rkp-mini issue  [--acknowledge] [--json] [--config PATH]
                  [--output DIR] [--server URL] [--fingerprint FP]
                  [--if-expiring-days N]   (skip unless the current leaf has <= N days left)
  rkp-mini config init|show [--config PATH]
  rkp-mini --version
  rkp-mini --help
";

#[derive(Debug, Default)]
pub struct StatusArgs {
    pub json: bool,
    pub update_description: bool,
    pub quiet: bool,
    pub config: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct IssueArgs {
    pub acknowledge: bool,
    pub json: bool,
    pub config: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub server: Option<String>,
    pub fingerprint: Option<String>,
    /// Only issue when the current keybox is missing/invalid or within N days of expiry.
    pub if_expiring_days: Option<u32>,
}

#[derive(Debug)]
pub enum Command {
    Status(StatusArgs),
    Issue(IssueArgs),
    ConfigInit { config: Option<PathBuf> },
    ConfigShow { config: Option<PathBuf> },
    TaWorker { dir: PathBuf },
    Version,
    Help,
}

/// Pull the value for a flag like `--config` from the iterator.
fn take_value(flag: &str, it: &mut impl Iterator<Item = String>) -> Result<String> {
    it.next()
        .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))
}

pub fn parse(args: &[String]) -> Result<Command> {
    let mut it = args.iter().cloned();
    let cmd = match it.next() {
        Some(c) => c,
        None => return Ok(Command::Help),
    };

    match cmd.as_str() {
        "--help" | "-h" | "help" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Version),
        "status" => {
            let mut a = StatusArgs::default();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--json" => a.json = true,
                    "--update-description" => a.update_description = true,
                    "--quiet" => a.quiet = true,
                    "--config" => a.config = Some(PathBuf::from(take_value("--config", &mut it)?)),
                    other => bail!("unknown status flag: {other}"),
                }
            }
            Ok(Command::Status(a))
        }
        "issue" => {
            let mut a = IssueArgs::default();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--acknowledge" => a.acknowledge = true,
                    "--json" => a.json = true,
                    "--config" => a.config = Some(PathBuf::from(take_value("--config", &mut it)?)),
                    "--output" => a.output = Some(PathBuf::from(take_value("--output", &mut it)?)),
                    "--server" => a.server = Some(take_value("--server", &mut it)?),
                    "--fingerprint" => a.fingerprint = Some(take_value("--fingerprint", &mut it)?),
                    "--if-expiring-days" => {
                        let v = take_value("--if-expiring-days", &mut it)?;
                        a.if_expiring_days = Some(v.parse().map_err(|_| {
                            anyhow::anyhow!("--if-expiring-days needs a non-negative integer")
                        })?);
                    }
                    other => bail!("unknown issue flag: {other}"),
                }
            }
            Ok(Command::Issue(a))
        }
        "config" => {
            let sub = it.next().unwrap_or_default();
            let mut config = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--config" => config = Some(PathBuf::from(take_value("--config", &mut it)?)),
                    other => bail!("unknown config flag: {other}"),
                }
            }
            match sub.as_str() {
                "init" => Ok(Command::ConfigInit { config }),
                "show" => Ok(Command::ConfigShow { config }),
                "" => bail!("config requires a subcommand: init|show"),
                other => bail!("unknown config subcommand: {other}"),
            }
        }
        "__ta-worker" => {
            let dir = it
                .next()
                .ok_or_else(|| anyhow::anyhow!("__ta-worker requires a directory"))?;
            Ok(Command::TaWorker {
                dir: PathBuf::from(dir),
            })
        }
        other => bail!("unknown command: {other}"),
    }
}
