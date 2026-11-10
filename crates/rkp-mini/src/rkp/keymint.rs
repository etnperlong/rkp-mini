//! KeyMint TA transport: parent/child subprocess isolation around the vendor
//! `libkeymasterdeviceutils.so` `send_cmd` ABI.

use std::ffi::c_void;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use libloading::{Library, Symbol};

use crate::error::{err, AppError, Stage};

pub const VENDOR_LIB: &str = "/vendor/lib64/libkeymasterdeviceutils.so";
pub const SMCINVOKE: &str = "/dev/smcinvoke";

/// Response buffer size handed to `send_cmd`.
const RESPONSE_CAP: usize = 0xa000;
/// KeymasterUtils object allocation (over-allocated vs observed 0x40).
const OBJECT_SIZE: usize = 0x140;
const OBJECT_ALIGN: usize = 16;
/// TEE security level passed to the constructor.
const SECURITY_LEVEL_TEE: i32 = 1;

const LD_LIBRARY_PATH: &str =
    "/system/lib64:/vendor/lib64:/odm/lib64:/vendor/lib64/hw:/odm/lib64/hw";

const CTOR_SYM: &[u8] = b"_ZN14keymasterutils14KeymasterUtilsC1E26keymaster_security_level_t";
const DTOR_SYM: &[u8] = b"_ZN14keymasterutils14KeymasterUtilsD1Ev";
const SEND_SYM: &[u8] = b"_ZN14keymasterutils14KeymasterUtils8send_cmdEPvjS1_j";

type Ctor = unsafe extern "C" fn(*mut c_void, i32);
type Dtor = unsafe extern "C" fn(*mut c_void);
type SendCmd = unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void, u32) -> i32;

const REPLY_MAGIC: &[u8; 4] = b"RKP1";

#[derive(Debug)]
pub struct TaReply {
    /// Raw response buffer from the TA (feed to `protocol::decode_frame`).
    pub buffer: Vec<u8>,
}

/// Parent side: run the TA request in an isolated worker subprocess.
pub fn send(request: &[u8], timeout: Duration, tmp_base: &Path) -> Result<TaReply> {
    std::fs::create_dir_all(tmp_base)
        .map_err(|e| err(Stage::Ta, "ta_transport", format!("mkdir tmp: {e}")))?;
    let dir = unique_dir(tmp_base)?;
    let request_path = dir.join("request.bin");
    let reply_path = dir.join("reply.bin");

    std::fs::write(&request_path, request)
        .map_err(|e| err(Stage::Ta, "ta_transport", format!("write request: {e}")))?;

    let exe = std::env::current_exe()
        .map_err(|e| err(Stage::Ta, "ta_transport", format!("current_exe: {e}")))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__ta-worker")
        .arg(&dir)
        .env("LD_LIBRARY_PATH", LD_LIBRARY_PATH)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| err(Stage::Ta, "ta_transport", format!("spawn worker: {e}")))?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err(err(
                        Stage::Ta,
                        "ta_timeout",
                        format!("TA worker exceeded {}s", timeout.as_secs()),
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(err(Stage::Ta, "ta_transport", format!("wait worker: {e}")));
            }
        }
    }

    let reply = std::fs::read(&reply_path).map_err(|e| {
        err(
            Stage::Ta,
            "ta_worker",
            format!("worker produced no reply: {e}"),
        )
    })?;
    let _ = std::fs::remove_dir_all(&dir);
    parse_reply(&reply)
}

fn unique_dir(base: &Path) -> Result<PathBuf> {
    let name = format!("ta-{}-{}", std::process::id(), crate::util::now_unix());
    let dir = base.join(name);
    crate::util::ensure_dir_0700(&dir)
        .map_err(|e| err(Stage::Ta, "ta_transport", format!("mkdir worker dir: {e}")))?;
    Ok(dir)
}

fn parse_reply(reply: &[u8]) -> Result<TaReply> {
    if reply.len() < 12 || &reply[0..4] != REPLY_MAGIC {
        return Err(err(Stage::Ta, "ta_worker", "reply has bad magic/length"));
    }
    let status = u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]);
    let len = u32::from_le_bytes([reply[8], reply[9], reply[10], reply[11]]) as usize;
    if 12 + len > reply.len() {
        return Err(err(Stage::Ta, "ta_worker", "reply length overflow"));
    }
    let payload = &reply[12..12 + len];
    if status == 0 {
        return Ok(TaReply {
            buffer: payload.to_vec(),
        });
    }
    // Error payload: "code\nmessage".
    let text = String::from_utf8_lossy(payload);
    let (code, message) = text.split_once('\n').unwrap_or(("ta_transport", &text));
    Err(anyhow::Error::new(AppError::new(
        Stage::Ta,
        code,
        message.to_string(),
    )))
}

/// Child entry point (`__ta-worker <dir>`): read request, call TA, write reply.
pub fn worker_main(dir: &Path) -> i32 {
    let request = match std::fs::read(dir.join("request.bin")) {
        Ok(r) => r,
        Err(_) => return 1,
    };
    let result = std::panic::catch_unwind(|| ta_send(&request));
    let reply = match result {
        Ok(Ok(buffer)) => encode_reply(0, &buffer),
        Ok(Err((code, msg))) => encode_reply(1, format!("{code}\n{msg}").as_bytes()),
        Err(_) => encode_reply(1, b"ta_transport\nvendor library panicked"),
    };
    if std::fs::write(dir.join("reply.bin"), reply).is_err() {
        return 1;
    }
    0
}

fn encode_reply(status: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len());
    out.extend_from_slice(REPLY_MAGIC);
    out.extend_from_slice(&status.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// The actual vendor-library call. Returns the raw response buffer or `(code, msg)`.
fn ta_send(request: &[u8]) -> std::result::Result<Vec<u8>, (&'static str, String)> {
    let lib = unsafe { Library::new(VENDOR_LIB) }
        .map_err(|e| ("ta_transport", format!("dlopen {VENDOR_LIB}: {e}")))?;
    let ctor: Symbol<Ctor> =
        unsafe { lib.get(CTOR_SYM) }.map_err(|e| ("ta_transport", format!("ctor symbol: {e}")))?;
    let dtor: Symbol<Dtor> =
        unsafe { lib.get(DTOR_SYM) }.map_err(|e| ("ta_transport", format!("dtor symbol: {e}")))?;
    let send_cmd: Symbol<SendCmd> = unsafe { lib.get(SEND_SYM) }
        .map_err(|e| ("ta_transport", format!("send_cmd symbol: {e}")))?;

    let layout = std::alloc::Layout::from_size_align(OBJECT_SIZE, OBJECT_ALIGN)
        .map_err(|e| ("ta_transport", format!("object layout: {e}")))?;
    // SAFETY: zeroed allocation of a valid non-zero layout; freed below after dtor.
    let obj = unsafe { std::alloc::alloc_zeroed(layout) };
    if obj.is_null() {
        return Err(("ta_transport", "object allocation failed".into()));
    }

    let mut response = vec![0u8; RESPONSE_CAP];
    response[0..4].copy_from_slice(&crate::rkp::protocol::SENTINEL.to_le_bytes());

    // SAFETY: obj is a live OBJECT_SIZE allocation; buffers are valid for their lengths.
    let rc = unsafe {
        ctor(obj as *mut c_void, SECURITY_LEVEL_TEE);
        let rc = send_cmd(
            obj as *mut c_void,
            request.as_ptr() as *mut c_void,
            request.len() as u32,
            response.as_mut_ptr() as *mut c_void,
            response.len() as u32,
        );
        dtor(obj as *mut c_void);
        rc
    };
    // SAFETY: same layout used for allocation; obj still valid, destructed above.
    unsafe { std::alloc::dealloc(obj, layout) };

    if rc != 0 {
        return Err(("ta_transport", format!("send_cmd returned {rc}")));
    }
    let head = i32::from_le_bytes([response[0], response[1], response[2], response[3]]);
    if head == crate::rkp::protocol::SENTINEL {
        return Err((
            "ta_no_response",
            "response buffer unchanged (sentinel)".into(),
        ));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_roundtrip_ok() {
        let enc = encode_reply(0, &[1, 2, 3, 4]);
        let r = parse_reply(&enc).unwrap();
        assert_eq!(r.buffer, vec![1, 2, 3, 4]);
    }

    #[test]
    fn reply_error_carries_code() {
        let enc = encode_reply(1, b"ta_no_response\nbuffer unchanged");
        let e = parse_reply(&enc).unwrap_err();
        let app = crate::error::to_app_error(&e);
        assert_eq!(app.code, "ta_no_response");
        assert!(app.message.contains("unchanged"));
    }

    #[test]
    fn reply_bad_magic() {
        assert!(parse_reply(b"XXXX00000000").is_err());
    }
}
