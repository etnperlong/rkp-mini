//! `rkp-mini-deployer`: write a verified staged keybox to the local targets.
//!
//! Local only: no network client, no age/key material, no vendor ABI. stdout
//! carries exactly one result (JSON envelope with `--json`); diagnostics go to
//! stderr. Exit codes: 0 ok, 2 usage/config/approval, 9 deploy, 1 other.

rust_i18n::i18n!("locales", fallback = "zh-CN");

mod action;
mod cli;
mod commands;
mod deploy;
mod lang;
mod modules;
mod omk;

use cli::Command;
use rkp_mini_core::error::{err, to_sync_error, Stage, SyncError};
use rkp_mini_runtime::config::Role;
use rkp_mini_runtime::context::Ctx;
use rkp_mini_runtime::output::{emit, print_error, Output};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (global, cmd) = match cli::parse(&args) {
        Ok(p) => p,
        Err(e) => {
            if args.iter().any(|a| a == "--json") {
                print_error(&SyncError::new(Stage::Config, "usage", e.to_string()), None);
            } else {
                eprintln!("error: {e}\n\n{}", cli::HELP);
            }
            std::process::exit(2);
        }
    };
    let json = global.json;
    let wrong_role = global.role.is_some_and(|r| r != Role::Receiver);
    let ctx = Ctx { global };
    let code = match dispatch(&ctx, cmd, wrong_role) {
        Ok(out) => emit(out, json),
        Err(e) => {
            let se = to_sync_error(&e);
            if json {
                print_error(&se, None);
            } else {
                eprintln!("error: {se}");
            }
            se.exit_code()
        }
    };
    std::process::exit(code);
}

fn dispatch(ctx: &Ctx, cmd: Command, wrong_role: bool) -> anyhow::Result<Output> {
    if wrong_role {
        return Err(err(
            Stage::Config,
            "wrong_role",
            "this command needs --role receiver (the deployer only writes local targets)",
        ));
    }
    match cmd {
        Command::Help => Ok(Output::ok(serde_json::Value::Null, cli::HELP.trim_end())),
        Command::ActionMessage { key, args } => {
            let text = action::render(&key, &args)?;
            Ok(Output::ok(serde_json::Value::Null, text))
        }
        Command::Version => Ok(Output::ok(
            serde_json::json!({ "version": cli::VERSION }),
            format!("rkp-mini-deployer {}", cli::VERSION),
        )),
        Command::Plan => commands::plan(ctx),
        Command::Apply {
            allow_identity_change,
            confirm_first,
            auto,
            if_staged,
        } => commands::apply(ctx, allow_identity_change, confirm_first, auto, if_staged),
        Command::Targets => Ok(commands::targets(std::path::Path::new(
            modules::MODULES_DIR,
        ))),
        Command::AddTarget { id, path, reload } => commands::add_target(ctx, &id, path, reload),
        Command::RemoveTarget { id } => commands::remove_target(ctx, &id),
        Command::Remove { targets, yes } => commands::remove(ctx, targets, yes),
        Command::UpdateDescription => commands::update_description(),
    }
}
