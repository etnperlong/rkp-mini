//! Interactive prompts on stderr/stdin.
//!
//! Only plain (non-secret) prompts live here: secret reading stays in the sync
//! binary so the deployer never links the `age`/secret code path.

use std::io::{self, BufRead, Write};

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};

pub fn stdin_is_tty() -> bool {
    // SAFETY: isatty only inspects the descriptor.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

pub fn read_line() -> Result<String> {
    let mut s = String::new();
    io::stdin()
        .lock()
        .read_line(&mut s)
        .map_err(|e| err(Stage::Generic, "io", format!("read stdin: {e}")))?;
    Ok(s.trim_end_matches(['\n', '\r']).to_string())
}

/// Ask a question on the TTY; returns the answer line.
pub fn ask(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    read_line()
}

/// y/N confirmation; defaults to "no".
pub fn confirm(prompt: &str) -> Result<bool> {
    let a = ask(&format!("{prompt} [y/N] "))?;
    Ok(matches!(a.trim(), "y" | "Y" | "yes" | "YES"))
}
