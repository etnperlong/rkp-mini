//! Hand-written argument parsing (no clap, keep binary small).

use std::path::PathBuf;

use anyhow::{bail, Result};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
rkp-mini-issuer — Qualcomm KeyMint CSR_V2 (0x3122) RKP keybox issuer

USAGE:
  rkp-mini-issuer status [--json] [--update-description] [--quiet] [--config PATH]
  rkp-mini-issuer issue  [--acknowledge] [--json] [--config PATH]
                  [--output DIR] [--server URL] [--fingerprint FP]
                  [--if-expiring-days N]   (skip unless the current leaf has <= N days left)
  rkp-mini-issuer remove [--yes] [--json] [--config PATH]
                  (delete the local keybox + artifacts; --yes required)
  rkp-mini-issuer config init|show [--config PATH]
  rkp-mini-issuer config set auto_issue_days N [--json] [--config PATH]
                  (N = 0..=365; 0 = off; used by the module's auto-loop.sh)
  rkp-mini-issuer update-description [--config PATH]
                  (rewrite module.prop description from the current keybox)
  rkp-mini-issuer --version
  rkp-mini-issuer --help
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

#[derive(Debug, Default)]
pub struct RemoveArgs {
    pub yes: bool,
    pub json: bool,
    pub config: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct ConfigSetArgs {
    pub key: String,
    pub value: String,
    pub json: bool,
    pub config: Option<PathBuf>,
}

#[derive(Debug)]
pub enum Command {
    Status(StatusArgs),
    Issue(IssueArgs),
    Remove(RemoveArgs),
    ConfigInit { config: Option<PathBuf> },
    ConfigShow { config: Option<PathBuf> },
    ConfigSet(ConfigSetArgs),
    UpdateDescription { config: Option<PathBuf> },
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
        "remove" => {
            let mut a = RemoveArgs::default();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--yes" => a.yes = true,
                    "--json" => a.json = true,
                    "--config" => a.config = Some(PathBuf::from(take_value("--config", &mut it)?)),
                    other => bail!("unknown remove flag: {other}"),
                }
            }
            Ok(Command::Remove(a))
        }
        "config" => {
            let sub = it.next().unwrap_or_default();
            if sub == "set" {
                let mut a = ConfigSetArgs::default();
                let mut positional = Vec::new();
                while let Some(arg) = it.next() {
                    match arg.as_str() {
                        "--json" => a.json = true,
                        "--config" => {
                            a.config = Some(PathBuf::from(take_value("--config", &mut it)?))
                        }
                        other => {
                            if other.starts_with("--") {
                                bail!("unknown config set flag: {other}");
                            }
                            positional.push(other.to_string());
                        }
                    }
                }
                let mut pos = positional.into_iter();
                match (pos.next(), pos.next(), pos.next()) {
                    (Some(k), Some(v), None) => {
                        a.key = k;
                        a.value = v;
                        return Ok(Command::ConfigSet(a));
                    }
                    _ => bail!("config set needs exactly KEY VALUE"),
                }
            }
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
                "" => bail!("config requires a subcommand: init|show|set"),
                other => bail!("unknown config subcommand: {other}"),
            }
        }
        "update-description" => {
            let mut config = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--config" => config = Some(PathBuf::from(take_value("--config", &mut it)?)),
                    other => bail!("unknown update-description flag: {other}"),
                }
            }
            Ok(Command::UpdateDescription { config })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn remove(v: &[&str]) -> RemoveArgs {
        match parse(&args(v)).unwrap() {
            Command::Remove(a) => a,
            other => panic!("expected remove, got {other:?}"),
        }
    }

    #[test]
    fn remove_parses_with_and_without_yes() {
        let a = remove(&["remove"]);
        assert!(!a.yes && !a.json && a.config.is_none());

        // Flags may appear in any order.
        let a = remove(&["remove", "--config", "/x", "--json", "--yes"]);
        assert!(a.yes && a.json);
        assert_eq!(a.config, Some(PathBuf::from("/x")));

        let a = remove(&["remove", "--yes", "--json"]);
        assert!(a.yes && a.json);
        assert!(a.config.is_none());
    }

    #[test]
    fn remove_rejects_unknown_flags() {
        assert!(parse(&args(&["remove", "--nope"])).is_err());
        assert!(parse(&args(&["remove", "--yes", "--config"])).is_err());
    }

    #[test]
    fn update_description_takes_config_only() {
        match parse(&args(&["update-description"])).unwrap() {
            Command::UpdateDescription { config } => assert!(config.is_none()),
            other => panic!("expected update-description, got {other:?}"),
        }
        match parse(&args(&["update-description", "--config", "/x"])).unwrap() {
            Command::UpdateDescription { config } => {
                assert_eq!(config, Some(PathBuf::from("/x")))
            }
            other => panic!("expected update-description, got {other:?}"),
        }
        // Plain command: no JSON envelope and no truncating flags.
        assert!(parse(&args(&["update-description", "--json"])).is_err());
        assert!(parse(&args(&["update-description", "--quiet"])).is_err());
        assert!(parse(&args(&["update-description", "--config"])).is_err());
    }
}
