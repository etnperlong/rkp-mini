//! Hand-written argument parsing (no clap; matches `rkp-mini`).
//!
//! There is deliberately no option that takes a password, passphrase or
//! token value: secrets are read from a TTY, stdin or a 0600 file only.

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::config::Role;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
rkp-sync — encrypted keybox sync and deployment (sync protocol v1)

Remote keybox deployment is opt-in and disabled by default. Using a keybox on
another device is keybox cloning, not legitimate RKP provisioning.

USAGE:
  rkp-sync [GLOBAL] <command> [options]

GLOBAL:
  --config PATH            config file (default by role)
  --role sender|receiver   pick the default config for that role
  --json                   one JSON envelope on stdout
  --allow-insecure-http    accept http:// URLs (local debugging only)
  --allow-private          accept loopback/private hosts (local debugging only)

COMMON:
  init --role R [--provider gist|webdav] [--stream-id ID]
  enable [--accept-risk]   show the risk statement and enable sync
  disable                  stop sync/automatic runs; keeps deployed keybox
  forget [--yes] [--staging]
                           delete local secrets/state (remote is not touched)
  secret set age|gist-token|webdav-password
                           read a secret from the TTY (no echo) or stdin
  status                   local state; never contacts the remote
  check-auto               exit 0 when unattended runs are allowed

SENDER:
  push [--force] [--allow-expiring] [--auto]
  sync-now                 alias of `push --auto`
  share [--label TEXT]     print the import link/QR (never contains the password)
  rotate-password          re-encrypt the current keybox with a new passphrase

RECEIVER:
  import <rkpsync://…|JSON> [--yes] [--test]
  fetch [--accept-first] [--no-cache] [--auto]
  deploy apply [--allow-identity-change] [--confirm-first] [--auto]
  deploy add-target omk|teesim|tricky_store [--path PATH] [--reload auto|restart|none]
  deploy remove-target ID
  deploy remove [--target ID]... --yes
  run [--auto]             fetch + deploy

  rkp-sync --version | --help
";

#[derive(Debug, Default, Clone)]
pub struct Global {
    pub config: Option<PathBuf>,
    pub role: Option<Role>,
    pub json: bool,
    pub allow_insecure_http: bool,
    pub allow_private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    Age,
    GistToken,
    WebdavPassword,
}

#[derive(Debug, Clone)]
pub enum Command {
    Help,
    Version,
    Init {
        provider: Option<String>,
        stream_id: Option<String>,
    },
    Enable {
        accept_risk: bool,
    },
    Disable,
    Forget {
        yes: bool,
        staging: bool,
    },
    SecretSet(SecretKind),
    Status,
    CheckAuto,
    Push {
        force: bool,
        allow_expiring: bool,
        auto: bool,
    },
    Share {
        label: Option<String>,
    },
    RotatePassword,
    Import {
        input: String,
        yes: bool,
        test: bool,
    },
    Fetch {
        accept_first: bool,
        no_cache: bool,
        auto: bool,
    },
    DeployApply {
        allow_identity_change: bool,
        confirm_first: bool,
        auto: bool,
    },
    DeployAddTarget {
        id: String,
        path: Option<String>,
        reload: Option<String>,
    },
    DeployRemoveTarget {
        id: String,
    },
    DeployRemove {
        targets: Vec<String>,
        yes: bool,
    },
    Run {
        auto: bool,
    },
}

impl Command {
    /// Role a command requires (None = either).
    pub fn role(&self) -> Option<Role> {
        match self {
            Command::Push { .. } | Command::Share { .. } | Command::RotatePassword => {
                Some(Role::Sender)
            }
            Command::Import { .. }
            | Command::Fetch { .. }
            | Command::DeployApply { .. }
            | Command::DeployAddTarget { .. }
            | Command::DeployRemoveTarget { .. }
            | Command::DeployRemove { .. }
            | Command::Run { .. } => Some(Role::Receiver),
            _ => None,
        }
    }
}

fn take_value(flag: &str, it: &mut impl Iterator<Item = String>) -> Result<String> {
    match it.next() {
        Some(v) if !v.starts_with("--") => Ok(v),
        _ => bail!("{flag} needs a value"),
    }
}

fn parse_role(v: &str) -> Result<Role> {
    match v {
        "sender" => Ok(Role::Sender),
        "receiver" => Ok(Role::Receiver),
        _ => bail!("--role must be sender or receiver"),
    }
}

/// Reject anything that looks like an attempt to pass a secret on argv.
fn reject_secret_flag(a: &str) -> Result<()> {
    let l = a.to_ascii_lowercase();
    if l.starts_with("--") && ["pass", "token", "secret"].iter().any(|m| l.contains(m)) {
        bail!("{a}: secrets are never accepted on the command line; use `rkp-sync secret set`");
    }
    Ok(())
}

pub fn parse(args: &[String]) -> Result<(Global, Command)> {
    let mut g = Global::default();
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter().cloned();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => g.config = Some(PathBuf::from(take_value("--config", &mut it)?)),
            "--role" => g.role = Some(parse_role(&take_value("--role", &mut it)?)?),
            "--json" => g.json = true,
            "--allow-insecure-http" => g.allow_insecure_http = true,
            "--allow-private" => g.allow_private = true,
            _ => rest.push(a),
        }
    }
    let mut it = rest.into_iter();
    let Some(cmd) = it.next() else {
        return Ok((g, Command::Help));
    };
    let args: Vec<String> = it.collect();
    for a in &args {
        reject_secret_flag(a)?;
    }
    let mut flags = Flags::new(args);
    let command = match cmd.as_str() {
        "-h" | "--help" | "help" => Command::Help,
        "-V" | "--version" | "version" => Command::Version,
        "init" => Command::Init {
            provider: flags.value("--provider")?,
            stream_id: flags.value("--stream-id")?,
        },
        "enable" => Command::Enable {
            accept_risk: flags.flag("--accept-risk"),
        },
        "disable" => Command::Disable,
        "forget" => Command::Forget {
            yes: flags.flag("--yes"),
            staging: flags.flag("--staging"),
        },
        "secret" => {
            match flags.positional().as_deref() {
                Some("set") => {}
                _ => bail!("usage: rkp-sync secret set age|gist-token|webdav-password"),
            }
            Command::SecretSet(match flags.positional().as_deref() {
                Some("age") => SecretKind::Age,
                Some("gist-token") => SecretKind::GistToken,
                Some("webdav-password") => SecretKind::WebdavPassword,
                _ => bail!("usage: rkp-sync secret set age|gist-token|webdav-password"),
            })
        }
        "status" => Command::Status,
        "check-auto" => Command::CheckAuto,
        "push" => Command::Push {
            force: flags.flag("--force"),
            allow_expiring: flags.flag("--allow-expiring"),
            auto: flags.flag("--auto"),
        },
        "sync-now" => Command::Push {
            force: false,
            allow_expiring: false,
            auto: true,
        },
        "share" => Command::Share {
            label: flags.value("--label")?,
        },
        "rotate-password" => Command::RotatePassword,
        "import" => {
            let yes = flags.flag("--yes");
            let test = flags.flag("--test");
            let input = flags
                .positional()
                .ok_or_else(|| anyhow::anyhow!("usage: rkp-sync import '<rkpsync://…>'"))?;
            Command::Import { input, yes, test }
        }
        "fetch" => Command::Fetch {
            accept_first: flags.flag("--accept-first"),
            no_cache: flags.flag("--no-cache"),
            auto: flags.flag("--auto"),
        },
        "deploy" => match flags.positional().as_deref() {
            Some("apply") => Command::DeployApply {
                allow_identity_change: flags.flag("--allow-identity-change"),
                confirm_first: flags.flag("--confirm-first"),
                auto: flags.flag("--auto"),
            },
            Some("add-target") => {
                let path = flags.value("--path")?;
                let reload = flags.value("--reload")?;
                let id = flags
                    .positional()
                    .ok_or_else(|| anyhow::anyhow!("usage: rkp-sync deploy add-target <id>"))?;
                Command::DeployAddTarget { id, path, reload }
            }
            Some("remove-target") => Command::DeployRemoveTarget {
                id: flags
                    .positional()
                    .ok_or_else(|| anyhow::anyhow!("usage: rkp-sync deploy remove-target <id>"))?,
            },
            Some("remove") => {
                let yes = flags.flag("--yes");
                let mut targets = Vec::new();
                while let Some(t) = flags.value("--target")? {
                    targets.push(t);
                }
                Command::DeployRemove { targets, yes }
            }
            _ => bail!("usage: rkp-sync deploy apply|add-target|remove-target|remove"),
        },
        "run" => Command::Run {
            auto: flags.flag("--auto"),
        },
        other => bail!("unknown command `{other}`"),
    };
    flags.finish()?;
    Ok((g, command))
}

/// Remaining command arguments, consumed by name.
struct Flags {
    args: Vec<String>,
}

impl Flags {
    fn new(args: Vec<String>) -> Self {
        Flags { args }
    }

    fn flag(&mut self, name: &str) -> bool {
        match self.args.iter().position(|a| a == name) {
            Some(i) => {
                self.args.remove(i);
                true
            }
            None => false,
        }
    }

    fn value(&mut self, name: &str) -> Result<Option<String>> {
        let Some(i) = self.args.iter().position(|a| a == name) else {
            return Ok(None);
        };
        if i + 1 >= self.args.len() || self.args[i + 1].starts_with("--") {
            bail!("{name} needs a value");
        }
        let v = self.args.remove(i + 1);
        self.args.remove(i);
        Ok(Some(v))
    }

    fn positional(&mut self) -> Option<String> {
        let i = self.args.iter().position(|a| !a.starts_with("--"))?;
        Some(self.args.remove(i))
    }

    fn finish(self) -> Result<()> {
        if let Some(a) = self.args.first() {
            bail!("unexpected argument `{a}`");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<(Global, Command)> {
        parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_globals_anywhere() {
        let (g, c) = p(&["push", "--json", "--force", "--config", "/x.toml"]).unwrap();
        assert!(g.json);
        assert_eq!(g.config, Some(PathBuf::from("/x.toml")));
        assert!(matches!(
            c,
            Command::Push {
                force: true,
                auto: false,
                ..
            }
        ));
        let (_, c) = p(&[
            "deploy", "remove", "--target", "omk", "--target", "teesim", "--yes",
        ])
        .unwrap();
        match c {
            Command::DeployRemove { targets, yes } => {
                assert_eq!(targets, vec!["omk", "teesim"]);
                assert!(yes);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_secret_flags_and_leftovers() {
        assert!(p(&["push", "--password", "x"]).is_err());
        assert!(p(&["import", "rkpsync://v1/import?p=e30", "--token=abc"]).is_err());
        assert!(p(&["push", "extra"]).is_err());
        assert!(p(&["frobnicate"]).is_err());
    }
}
