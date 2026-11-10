//! One command result: JSON `data`, human text and an optional failure that
//! still carries data (e.g. a deploy where one target failed).

use rkp_mini_core::error::SyncError;
use serde_json::Value;

pub struct Output {
    pub data: Value,
    pub text: String,
    pub failure: Option<SyncError>,
}

impl Output {
    pub fn ok(data: Value, text: impl Into<String>) -> Output {
        Output {
            data,
            text: text.into(),
            failure: None,
        }
    }
}

/// Print one error envelope. `data` is included when the failure still has
/// usable facts (TOFU, partial deploy).
pub fn print_error(e: &SyncError, data: Option<&Value>) {
    let mut env = serde_json::json!({
        "ok": false,
        "error": { "stage": e.stage.as_str(), "code": e.code, "message": e.message },
    });
    if let Some(d) = data {
        env["data"] = d.clone();
    }
    println!("{env}");
}

/// Write the result (exactly one envelope with `--json`) and return the exit
/// code. The caller owns `process::exit`.
pub fn emit(out: Output, json: bool) -> i32 {
    match &out.failure {
        None => {
            if json {
                println!("{}", serde_json::json!({ "ok": true, "data": out.data }));
            } else if !out.text.is_empty() {
                println!("{}", out.text);
            }
            0
        }
        Some(f) => {
            if json {
                print_error(f, Some(&out.data));
            } else {
                println!("{}", out.text);
                eprintln!("error: {f}");
            }
            f.exit_code()
        }
    }
}
