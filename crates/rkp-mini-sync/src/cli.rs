//! Hand-written argument parsing (no clap; matches the other commands).
//!
//! There is deliberately no option that takes a password, passphrase or
//! token value: secrets are read from a TTY, stdin, a 0600 file, or a 0600
//! WebUI inbox file (`--from-file`, design §19.2).

use std::path::PathBuf;

use anyhow::{bail, Result};
use rkp_mini_runtime::cli::{sha_arg, Flags};
use rkp_mini_runtime::config::Role;
use rkp_mini_runtime::context::Global;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
rkp-mini-sync — encrypted keybox sync (sync protocol v1)

Remote keybox deployment is opt-in and disabled by default. Using a keybox on
another device is keybox cloning, not legitimate RKP provisioning. Local
deployment is a separate command: rkp-mini-deployer.

USAGE:
  rkp-mini-sync [GLOBAL] <command> [options]

GLOBAL:
  --config PATH            config file (default by role)
  --role sender|receiver   pick the default config for that role
  --json                   one JSON envelope on stdout
  --allow-insecure-http    accept http:// URLs (local debugging only)
  --allow-private          accept loopback/private hosts (local debugging only)

COMMON:
  init --role R [--provider gist|webdav] [--stream-id ID]
  risk                     print the risk statement and its version
  enable [--accept-risk VERSION]
                           accept the risk statement (TTY phrase or exact version)
  disable                  stop sync/automatic runs; keeps deployed keybox
  forget [--yes] [--staging]
                           delete local secrets/state (remote is not touched)
  secret set age|gist-token|webdav-password [--from-file PATH [--hex]]
                           read a secret from the TTY (no echo), stdin, or a
                           0600 inbox file that is deleted afterwards
  config show              effective configuration (secret paths only)
  config set KEY VALUE     change one allow-listed, non-secret setting
  status                   local state; never contacts the remote
  check-auto [--require-verified]
                           exit 0 when unattended runs are allowed;
                           --require-verified also needs a completed first import

SENDER:
  push [--force] [--allow-expiring] [--auto]
  share [--label TEXT]     print the import link/QR (never contains the password)
  rotate-password [--from-file PATH [--hex]]
                           re-encrypt the current keybox with a new passphrase

RECEIVER:
  import <rkpsync://…|JSON> [--check | --yes [--test]]
  fetch [--accept-first PLAINTEXT_SHA256] [--no-cache] [--auto]
                           verify a remote version and stage it for deployment

Deployment is not part of this command: run
  rkp-mini-deployer plan
  rkp-mini-deployer apply [--confirm-first KEYBOX_SHA256]
                          [--allow-identity-change KEYBOX_SHA256]

Confirmation values (sha256 hex, full or >= 16-char prefix) bind the approval
to the exact version that was reviewed; a different staged version is refused.

  rkp-mini-sync --version | --help
";

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
    Risk,
    Enable {
        accept_risk: Option<u32>,
    },
    Disable,
    Forget {
        yes: bool,
        staging: bool,
    },
    SecretSet {
        kind: SecretKind,
        from_file: Option<PathBuf>,
        hex: bool,
    },
    ConfigShow,
    ConfigSet {
        key: String,
        value: String,
    },
    Status,
    CheckAuto {
        require_verified: bool,
    },
    Push {
        force: bool,
        allow_expiring: bool,
        auto: bool,
    },
    Share {
        label: Option<String>,
    },
    RotatePassword {
        from_file: Option<PathBuf>,
        hex: bool,
    },
    Import {
        input: String,
        yes: bool,
        test: bool,
        check: bool,
    },
    Fetch {
        accept_first: Option<String>,
        no_cache: bool,
        auto: bool,
    },
}

impl Command {
    /// Role a command requires (None = either).
    pub fn role(&self) -> Option<Role> {
        match self {
            Command::Push { .. } | Command::Share { .. } | Command::RotatePassword { .. } => {
                Some(Role::Sender)
            }
            Command::Import { .. } | Command::Fetch { .. } | Command::CheckAuto { .. } => {
                Some(Role::Receiver)
            }
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
        bail!(
            "{a}: secrets are never accepted on the command line; use `rkp-mini-sync secret set`"
        );
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
        "risk" => Command::Risk,
        "enable" => Command::Enable {
            accept_risk: flags
                .value("--accept-risk")?
                .map(|v| {
                    v.parse::<u32>().map_err(|_| {
                        anyhow::anyhow!(
                            "--accept-risk needs the statement version (see `rkp-mini-sync risk`)"
                        )
                    })
                })
                .transpose()?,
        },
        "disable" => Command::Disable,
        "forget" => Command::Forget {
            yes: flags.flag("--yes"),
            staging: flags.flag("--staging"),
        },
        "secret" => {
            const USAGE: &str = "usage: rkp-mini-sync secret set age|gist-token|webdav-password [--from-file PATH [--hex]]";
            let from_file = flags.value("--from-file")?.map(PathBuf::from);
            let hex = flags.flag("--hex");
            if hex && from_file.is_none() {
                bail!("--hex needs --from-file");
            }
            match flags.positional().as_deref() {
                Some("set") => {}
                _ => bail!(USAGE),
            }
            let kind = match flags.positional().as_deref() {
                Some("age") => SecretKind::Age,
                Some("gist-token") => SecretKind::GistToken,
                Some("webdav-password") => SecretKind::WebdavPassword,
                _ => bail!(USAGE),
            };
            Command::SecretSet {
                kind,
                from_file,
                hex,
            }
        }
        "config" => match flags.positional().as_deref() {
            Some("show") => Command::ConfigShow,
            Some("set") => {
                let (Some(key), Some(value)) = (flags.positional(), flags.positional_any()) else {
                    bail!("usage: rkp-mini-sync config set KEY VALUE");
                };
                Command::ConfigSet { key, value }
            }
            _ => bail!("usage: rkp-mini-sync config show|set"),
        },
        "status" => Command::Status,
        "check-auto" => Command::CheckAuto {
            require_verified: flags.flag("--require-verified"),
        },
        "push" => Command::Push {
            force: flags.flag("--force"),
            allow_expiring: flags.flag("--allow-expiring"),
            auto: flags.flag("--auto"),
        },
        "share" => Command::Share {
            label: flags.value("--label")?,
        },
        "rotate-password" => {
            let from_file = flags.value("--from-file")?.map(PathBuf::from);
            let hex = flags.flag("--hex");
            if hex && from_file.is_none() {
                bail!("--hex needs --from-file");
            }
            Command::RotatePassword { from_file, hex }
        }
        "import" => {
            let yes = flags.flag("--yes");
            let test = flags.flag("--test");
            let check = flags.flag("--check");
            if check && (yes || test) {
                bail!("--check only previews; it cannot be combined with --yes/--test");
            }
            let input = flags
                .positional()
                .ok_or_else(|| anyhow::anyhow!("usage: rkp-mini-sync import '<rkpsync://…>'"))?;
            Command::Import {
                input,
                yes,
                test,
                check,
            }
        }
        "fetch" => Command::Fetch {
            accept_first: flags.value("--accept-first")?.map(sha_arg).transpose()?,
            no_cache: flags.flag("--no-cache"),
            auto: flags.flag("--auto"),
        },
        "deploy" | "run" | "sync-now" => bail!(
            "`{cmd}` is not a command of rkp-mini-sync; local deployment moved to `rkp-mini-deployer`"
        ),
        other => bail!("unknown command `{other}`"),
    };
    flags.finish()?;
    Ok((g, command))
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
        assert!(matches!(
            p(&["check-auto", "--require-verified"]).unwrap().1,
            Command::CheckAuto {
                require_verified: true
            }
        ));
        assert!(matches!(
            p(&["check-auto"]).unwrap().1,
            Command::CheckAuto {
                require_verified: false
            }
        ));
    }

    #[test]
    fn deployment_commands_are_gone() {
        for args in [
            vec!["deploy", "plan"],
            vec!["deploy", "apply"],
            vec!["deploy", "remove", "--yes"],
            vec!["run"],
            vec!["run", "--auto"],
            vec!["sync-now"],
        ] {
            assert!(p(&args).is_err(), "{args:?} must be rejected");
        }
    }

    #[test]
    fn confirmation_values_are_validated() {
        let sha = "ABCDEF0123456789abcdef";
        match p(&["fetch", "--accept-first", sha]).unwrap().1 {
            Command::Fetch { accept_first, .. } => {
                assert_eq!(accept_first.as_deref(), Some("abcdef0123456789abcdef"))
            }
            other => panic!("{other:?}"),
        }
        assert!(
            p(&["fetch", "--accept-first", "abcdef"]).is_err(),
            "prefix too short"
        );
        assert!(
            p(&["fetch", "--accept-first"]).is_err(),
            "bare flag no longer accepted"
        );
        assert!(p(&["enable", "--accept-risk"]).is_err(), "version required");
        assert!(matches!(
            p(&["enable", "--accept-risk", "2"]).unwrap().1,
            Command::Enable {
                accept_risk: Some(2)
            }
        ));
        assert!(
            p(&["secret", "set", "age", "--hex"]).is_err(),
            "--hex needs --from-file"
        );
        match p(&["config", "set", "proxy", ""]).unwrap().1 {
            Command::ConfigSet { key, value } => {
                assert_eq!((key.as_str(), value.as_str()), ("proxy", ""))
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
