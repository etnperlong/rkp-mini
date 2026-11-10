//! `rkp-sync`: encrypted keybox sync (sender) and deployment (receiver).
//!
//! stdout carries exactly one result (JSON envelope with `--json`);
//! diagnostics go to stderr. Exit codes (appendix B): 0 ok, 2 usage/config/
//! risk acknowledgement, 8 sync/network/decrypt/validation/state, 9 deploy,
//! 1 other.

mod app;
mod cli;
mod common;
mod config;
mod deploy;
mod http;
mod receiver;
mod remote;
mod sender;
mod tty;

use app::{Ctx, Output};
use cli::Command;
use rkp_sync_core::error::{to_sync_error, SyncError};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (global, cmd) = match cli::parse(&args) {
        Ok(p) => p,
        Err(e) => {
            if args.iter().any(|a| a == "--json") {
                print_error(
                    &SyncError::new(rkp_sync_core::error::Stage::Config, "usage", e.to_string()),
                    None,
                );
            } else {
                eprintln!("error: {e}\n\n{}", cli::HELP);
            }
            std::process::exit(2);
        }
    };
    let json = global.json;
    let ctx = Ctx { global };
    match dispatch(&ctx, cmd) {
        Ok(out) => {
            match &out.failure {
                None => {
                    if json {
                        println!("{}", serde_json::json!({ "ok": true, "data": out.data }));
                    } else if !out.text.is_empty() {
                        println!("{}", out.text);
                    }
                }
                Some(f) => {
                    if json {
                        print_error(f, Some(&out.data));
                    } else {
                        println!("{}", out.text);
                        eprintln!("error: {f}");
                    }
                }
            }
            std::process::exit(out.failure.as_ref().map_or(0, SyncError::exit_code));
        }
        Err(e) => {
            let se = to_sync_error(&e);
            if json {
                print_error(&se, None);
            } else {
                eprintln!("error: {se}");
            }
            std::process::exit(se.exit_code());
        }
    }
}

fn print_error(e: &SyncError, data: Option<&serde_json::Value>) {
    let mut env = serde_json::json!({
        "ok": false,
        "error": { "stage": e.stage.as_str(), "code": e.code, "message": e.message },
    });
    if let Some(d) = data {
        env["data"] = d.clone();
    }
    println!("{env}");
}

fn dispatch(ctx: &Ctx, cmd: Command) -> anyhow::Result<Output> {
    if let Some(role) = cmd.role() {
        if ctx.global.role.is_some_and(|r| r != role) {
            return Err(rkp_sync_core::error::err(
                rkp_sync_core::error::Stage::Config,
                "wrong_role",
                format!("this command is for the {} role", role.as_str()),
            ));
        }
    }
    match cmd {
        Command::Help => Ok(Output::ok(serde_json::Value::Null, cli::HELP.trim_end())),
        Command::Version => Ok(Output::ok(
            serde_json::json!({ "version": cli::VERSION, "protocol": rkp_sync_core::PROTOCOL_VERSION }),
            format!(
                "rkp-sync {} (sync protocol v{})",
                cli::VERSION,
                rkp_sync_core::PROTOCOL_VERSION
            ),
        )),
        Command::Init {
            provider,
            stream_id,
        } => common::init(ctx, provider, stream_id),
        Command::Enable { accept_risk } => common::enable(ctx, accept_risk),
        Command::Disable => common::disable(ctx),
        Command::Forget { yes, staging } => common::forget(ctx, yes, staging),
        Command::SecretSet(kind) => common::secret_set(ctx, kind),
        Command::Status => common::status(ctx),
        Command::CheckAuto => common::check_auto(ctx),
        Command::Push {
            force,
            allow_expiring,
            auto,
        } => sender::push(ctx, force, allow_expiring, auto),
        Command::Share { label } => sender::share(ctx, label),
        Command::RotatePassword => sender::rotate_password(ctx),
        Command::Import { input, yes, test } => receiver::import(ctx, &input, yes, test),
        Command::Fetch {
            accept_first,
            no_cache,
            auto,
        } => receiver::fetch(ctx, accept_first, no_cache, auto),
        Command::DeployApply {
            allow_identity_change,
            confirm_first,
            auto,
        } => receiver::deploy_apply(ctx, allow_identity_change, confirm_first, auto),
        Command::DeployAddTarget { id, path, reload } => {
            receiver::add_target(ctx, &id, path, reload)
        }
        Command::DeployRemoveTarget { id } => receiver::remove_target(ctx, &id),
        Command::DeployRemove { targets, yes } => receiver::deploy_remove(ctx, targets, yes),
        Command::Run { auto } => receiver::run(ctx, auto),
    }
}
