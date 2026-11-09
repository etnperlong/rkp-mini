mod cli;
mod config;
mod error;
mod rkp;
mod status;
#[cfg(test)]
mod test_support;
mod util;

use cli::Command;
use error::{err, to_app_error, Stage};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", cli::HELP);
            std::process::exit(2);
        }
    };

    let json = command_is_json(&cmd);

    match dispatch(cmd) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            let app = to_app_error(&e);
            if json {
                print_error_json(&app);
            } else {
                eprintln!("❌ {app}");
                if let Some(h) = &app.head_hex {
                    eprintln!("   head_hex: {h}");
                }
            }
            std::process::exit(app.exit_code());
        }
    }
}

fn command_is_json(cmd: &Command) -> bool {
    match cmd {
        Command::Status(a) => a.json,
        Command::Issue(a) => a.json,
        _ => false,
    }
}

fn print_error_json(app: &error::AppError) {
    let mut err_obj = serde_json::json!({
        "stage": app.stage.as_str(),
        "code": app.code,
        "message": app.message,
    });
    if let Some(s) = app.ta_status {
        err_obj["ta_status"] = serde_json::json!(s);
    }
    if let Some(h) = &app.head_hex {
        err_obj["head_hex"] = serde_json::json!(h);
    }
    let env = serde_json::json!({ "ok": false, "error": err_obj });
    println!("{env}");
}

fn dispatch(cmd: Command) -> anyhow::Result<i32> {
    match cmd {
        Command::Help => {
            print!("{}", cli::HELP);
            Ok(0)
        }
        Command::Version => {
            println!("rkp-mini {}", cli::VERSION);
            Ok(0)
        }
        Command::ConfigInit { config } => {
            config::init(&config::resolve_path(&config))?;
            Ok(0)
        }
        Command::ConfigShow { config } => {
            config::show(&config::resolve_path(&config))?;
            Ok(0)
        }
        Command::TaWorker { dir } => Ok(rkp::keymint::worker_main(&dir)),
        Command::Status(a) => status::run(&a),
        Command::Issue(_) => Err(err(
            Stage::Generic,
            "not_implemented",
            "command not yet wired",
        )),
    }
}
