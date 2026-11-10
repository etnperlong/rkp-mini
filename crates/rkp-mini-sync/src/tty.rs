//! Interactive prompts on stderr/stdin. Secrets are read with echo disabled.
//!
//! Plain prompts and confirmations come from the shared runtime; only the
//! echo-off secret reader lives here, so the deployer never links it.

use std::io::{self, BufRead, Read, Write};

use anyhow::Result;
use rkp_mini_core::error::{err, Stage};
use rkp_mini_sync_core::secrecy::SecretString;
use rkp_mini_sync_core::secret::{secret_from_bytes, MAX_SECRET_FILE};
use zeroize::Zeroizing;

pub use rkp_mini_runtime::tty::{ask, confirm, stdin_is_tty};

/// Restores terminal attributes on drop.
struct EchoOff {
    saved: libc::termios,
}

impl EchoOff {
    fn new() -> Option<EchoOff> {
        // SAFETY: termios is plain data; tcgetattr fills it for a valid fd.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
                return None;
            }
            let saved = t;
            t.c_lflag &= !libc::ECHO;
            t.c_lflag |= libc::ECHONL;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) != 0 {
                return None;
            }
            Some(EchoOff { saved })
        }
    }
}

impl Drop for EchoOff {
    fn drop(&mut self) {
        // SAFETY: restores the attributes captured in `new` on the same fd.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.saved);
        }
    }
}

fn read_hidden(prompt: &str) -> Result<Zeroizing<Vec<u8>>> {
    eprint!("{prompt}");
    let _ = io::stderr().flush();
    let guard = EchoOff::new();
    let mut line = Zeroizing::new(String::new());
    let r = io::stdin().lock().read_line(&mut line);
    drop(guard);
    r.map_err(|e| err(Stage::Generic, "io", format!("read secret: {e}")))?;
    Ok(Zeroizing::new(line.as_bytes().to_vec()))
}

/// Read a secret: from the TTY (echo off, optionally twice) or one stdin payload.
pub fn read_secret(what: &str, confirm_twice: bool) -> Result<SecretString> {
    if stdin_is_tty() {
        let first = read_hidden(&format!("{what}: "))?;
        if confirm_twice {
            let second = read_hidden(&format!("Repeat {what}: "))?;
            if *first != *second {
                return Err(err(
                    Stage::Config,
                    "secret_mismatch",
                    "entries do not match",
                ));
            }
        }
        return secret_from_bytes(&first, what);
    }
    let mut buf = Zeroizing::new(Vec::new());
    io::stdin()
        .lock()
        .take(MAX_SECRET_FILE as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| err(Stage::Generic, "io", format!("read stdin: {e}")))?;
    if buf.len() > MAX_SECRET_FILE {
        return Err(err(
            Stage::Config,
            "secret_invalid",
            format!("{what} is too long"),
        ));
    }
    secret_from_bytes(&buf, what)
}
