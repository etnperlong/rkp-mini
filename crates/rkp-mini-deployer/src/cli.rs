//! Hand-written argument parsing for the local deployer (no clap).

use anyhow::{bail, Result};
use rkp_mini_runtime::cli::{sha_arg, Flags};
use rkp_mini_runtime::config::Role;
use rkp_mini_runtime::context::Global;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
rkp-mini-deployer — write a verified staged keybox to local targets

Never contacts the network and never reads the age passphrase: it only reads
what `rkp-mini-sync fetch` left in staging/ and the local target config.
Using a keybox from another device is keybox cloning, not legitimate RKP
provisioning.

USAGE:
  rkp-mini-deployer [GLOBAL] <command> [options]

GLOBAL:
  --config PATH     receiver config (default /data/adb/rkp_sync/config.toml)
  --role receiver   the only accepted role
  --json            one JSON envelope on stdout

COMMANDS:
  plan                     preview the identity gate; writes nothing
  apply [--confirm-first KEYBOX_SHA256] [--allow-identity-change KEYBOX_SHA256]
        [--auto] [--if-staged]
                           write the staged keybox to every local target
  targets                  list the built-in targets and whether the module
                           that reads each keybox is installed
  add-target omk|teesim|tricky_store [--path PATH] [--reload auto|restart|none]
                           edit the local target list
  remove-target ID         drop a target from the config (file untouched)
  remove [--target ID]... --yes
                           delete deployed keybox files
  update-description       refresh the module card introduction in the
                           language last chosen in the WebUI; needs no config

--auto honours [deploy].auto_deploy; --if-staged makes a missing staging/latest.json
a successful no-op. Neither flag approves anything: an identity change outside
the rotation policy still needs --allow-identity-change <keybox_sha256>.

  rkp-mini-deployer --version | --help
";

#[derive(Debug, Clone)]
pub enum Command {
    Help,
    Version,
    Plan,
    Apply {
        allow_identity_change: Option<String>,
        confirm_first: Option<String>,
        auto: bool,
        if_staged: bool,
    },
    Targets,
    AddTarget {
        id: String,
        path: Option<String>,
        reload: Option<String>,
    },
    RemoveTarget {
        id: String,
    },
    Remove {
        targets: Vec<String>,
        yes: bool,
    },
    UpdateDescription,
    ActionMessage {
        key: String,
        args: Vec<String>,
    },
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

pub fn parse(args: &[String]) -> Result<(Global, Command)> {
    let mut g = Global::default();
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter().cloned();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => {
                g.config = Some(std::path::PathBuf::from(take_value("--config", &mut it)?))
            }
            "--role" => g.role = Some(parse_role(&take_value("--role", &mut it)?)?),
            "--json" => g.json = true,
            _ => rest.push(a),
        }
    }
    let mut it = rest.into_iter();
    let Some(cmd) = it.next() else {
        return Ok((g, Command::Help));
    };
    // Internal, for the module script: allowlisted key + literal args only.
    if cmd == "__action-message" {
        let key = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("__action-message requires a message key"))?;
        let args: Vec<String> = it.collect();
        crate::action::validate(&key, &args)?;
        return Ok((g, Command::ActionMessage { key, args }));
    }
    let mut flags = Flags::new(it.collect());
    let command = match cmd.as_str() {
        "-h" | "--help" | "help" => Command::Help,
        "-V" | "--version" | "version" => Command::Version,
        "plan" => Command::Plan,
        "apply" => Command::Apply {
            allow_identity_change: flags
                .value("--allow-identity-change")?
                .map(sha_arg)
                .transpose()?,
            confirm_first: flags.value("--confirm-first")?.map(sha_arg).transpose()?,
            auto: flags.flag("--auto"),
            if_staged: flags.flag("--if-staged"),
        },
        "targets" => Command::Targets,
        "add-target" => {
            let path = flags.value("--path")?;
            let reload = flags.value("--reload")?;
            let id = flags
                .positional()
                .ok_or_else(|| anyhow::anyhow!("usage: rkp-mini-deployer add-target <id>"))?;
            Command::AddTarget { id, path, reload }
        }
        "remove-target" => Command::RemoveTarget {
            id: flags
                .positional()
                .ok_or_else(|| anyhow::anyhow!("usage: rkp-mini-deployer remove-target <id>"))?,
        },
        "remove" => {
            let yes = flags.flag("--yes");
            let mut targets = Vec::new();
            while let Some(t) = flags.value("--target")? {
                targets.push(t);
            }
            Command::Remove { targets, yes }
        }
        "update-description" => Command::UpdateDescription,
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
    fn parses_commands_and_globals() {
        let (g, c) = p(&[
            "apply",
            "--json",
            "--confirm-first",
            "ABCDEF0123456789",
            "--if-staged",
        ])
        .unwrap();
        assert!(g.json);
        match c {
            Command::Apply {
                confirm_first,
                auto,
                if_staged,
                ..
            } => {
                assert_eq!(confirm_first.as_deref(), Some("abcdef0123456789"));
                assert!(!auto);
                assert!(if_staged);
            }
            other => panic!("{other:?}"),
        }
        match p(&["remove", "--target", "omk", "--target", "teesim", "--yes"])
            .unwrap()
            .1
        {
            Command::Remove { targets, yes } => {
                assert_eq!(targets, vec!["omk", "teesim"]);
                assert!(yes);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(p(&["plan"]).unwrap().1, Command::Plan));
        assert!(matches!(p(&["targets"]).unwrap().1, Command::Targets));
        assert!(matches!(
            p(&["add-target", "teesim", "--reload", "auto"]).unwrap().1,
            Command::AddTarget { .. }
        ));
        assert!(matches!(
            p(&["remove-target", "teesim"]).unwrap().1,
            Command::RemoveTarget { .. }
        ));
        let (g, c) = p(&["update-description", "--json"]).unwrap();
        assert!(g.json);
        assert!(matches!(c, Command::UpdateDescription));
    }

    #[test]
    fn action_message_is_allowlisted_and_verbatim() {
        let (g, c) = p(&["__action-message", "start", "/data/adb/x.log"]).unwrap();
        assert!(!g.json);
        match c {
            Command::ActionMessage { key, args } => {
                assert_eq!(key, "start");
                assert_eq!(args, vec!["/data/adb/x.log"]);
            }
            other => panic!("{other:?}"),
        }
        let (g, c) = p(&["__action-message", "action.busy", "--json"]).unwrap();
        assert!(g.json);
        assert!(matches!(c, Command::ActionMessage { .. }));
        assert!(p(&["__action-message", "nope"]).is_err());
        assert!(p(&["__action-message", "start"]).is_err());
        assert!(p(&["__action-message", "busy", "extra"]).is_err());
        assert!(p(&["__action-message"]).is_err());
    }

    #[test]
    fn update_description_takes_no_arguments() {
        assert!(p(&["update-description", "en"]).is_err());
        assert!(p(&["update-description", "--description", "x"]).is_err());
    }

    #[test]
    fn rejects_removed_and_unknown_flags() {
        // The old `deploy plan` spelling and network debugging flags are gone.
        assert!(p(&["deploy", "plan"]).is_err());
        assert!(p(&["run"]).is_err());
        assert!(p(&["plan", "--allow-private"]).is_err());
        assert!(p(&["plan", "--allow-insecure-http"]).is_err());
        assert!(p(&["apply", "--confirm-first", "abcdef"]).is_err());
        assert!(p(&["add-target"]).is_err());
        assert!(p(&["targets", "omk"]).is_err());
    }
}
