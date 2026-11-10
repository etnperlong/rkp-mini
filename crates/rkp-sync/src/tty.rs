//! Interactive prompts on stderr/stdin. Secrets are read with echo disabled.

use std::io::{self, BufRead, Read, Write};

use anyhow::Result;
use rkp_sync_core::error::{err, Stage};
use rkp_sync_core::secrecy::SecretString;
use rkp_sync_core::secret::{secret_from_bytes, MAX_SECRET_FILE};
use zeroize::Zeroizing;

pub fn stdin_is_tty() -> bool {
    // SAFETY: isatty only inspects the descriptor.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

fn read_line() -> Result<String> {
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
