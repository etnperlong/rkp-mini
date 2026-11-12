//! `rkp-mini-sync`: encrypted keybox sync (sender push, receiver fetch).
//!
//! stdout carries exactly one result (JSON envelope with `--json`);
//! diagnostics go to stderr. Exit codes (appendix B): 0 ok, 2 usage/config/
//! risk acknowledgement, 8 sync/network/decrypt/validation/state, 1 other.
//! Deployment (exit 9) belongs to `rkp-mini-deployer`.

mod app;
mod check;
mod cli;
mod common;
mod http;
mod receiver;
mod remote;
mod sender;
mod tty;

use cli::Command;
use rkp_mini_core::error::{err, to_sync_error, Stage, SyncError};
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
    let ctx = Ctx { global };
    let code = match dispatch(&ctx, cmd) {
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

fn dispatch(ctx: &Ctx, cmd: Command) -> anyhow::Result<Output> {
    if let Some(role) = cmd.role() {
        if ctx.global.role.is_some_and(|r| r != role) {
            return Err(err(
                Stage::Config,
                "wrong_role",
                format!("this command is for the {} role", role.as_str()),
            ));
        }
    }
    match cmd {
        Command::Help => Ok(Output::ok(serde_json::Value::Null, cli::HELP.trim_end())),
        Command::Version => Ok(Output::ok(
            serde_json::json!({ "version": cli::VERSION, "protocol": rkp_mini_core::PROTOCOL_VERSION }),
            format!(
                "rkp-mini-sync {} (sync protocol v{})",
                cli::VERSION,
                rkp_mini_core::PROTOCOL_VERSION
            ),
        )),
        Command::Init {
            provider,
            stream_id,
        } => common::init(ctx, provider, stream_id),
        Command::Risk => common::risk(ctx),
        Command::Enable { accept_risk } => common::enable(ctx, accept_risk),
        Command::Disable => common::disable(ctx),
        Command::Forget {
            yes,
            staging,
            reset,
        } => common::forget(ctx, yes, staging, reset),
        Command::SecretSet {
            kind,
            from_file,
            hex,
        } => common::secret_set(ctx, kind, from_file, hex),
        Command::ConfigShow => common::config_show(ctx),
        Command::ConfigSet { key, value } => common::config_set(ctx, &key, &value),
        Command::Status => common::status(ctx),
        Command::CheckRemote => check::check_remote(ctx),
        Command::CheckAuto { require_verified } => common::check_auto(ctx, require_verified),
        Command::RemoteDelete { yes } => check::remote_delete(ctx, yes),
        Command::Push {
            force,
            allow_expiring,
            auto,
        } => sender::push(ctx, force, allow_expiring, auto),
        Command::Share { label } => sender::share(ctx, label),
        Command::RotatePassword { from_file, hex } => sender::rotate_password(ctx, from_file, hex),
        Command::Import {
            input,
            yes,
            test,
            check,
        } => receiver::import(ctx, &input, yes, test, check),
        Command::Fetch {
            accept_first,
            no_cache,
            auto,
        } => receiver::fetch(ctx, accept_first, no_cache, auto),
    }
}
