//! age v1 passphrase (scrypt) encryption with ASCII armor (design §6.1).
//!
//! Work factor: encryption uses scrypt `log_n = 18` (the age reference
//! implementation default, ~256 MiB). Decryption accepts up to `log_n = 20`
//! so a slower receiver can still open files from a faster sender, while a
//! hostile file cannot demand more than ~1 GiB / tens of seconds.
//!
//! Every authentication/format failure maps to `decrypt_failed`: the caller
//! cannot distinguish a wrong passphrase from tampering or truncation.

use std::io::{Read, Write};
use std::iter;

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::secrecy::SecretString;
use age::{scrypt, DecryptError, Decryptor, Encryptor};
use anyhow::Result;
use zeroize::Zeroizing;

use rkp_mini_core::error::{err, Stage};
use rkp_mini_core::limits;

/// scrypt work factor for new envelopes.
pub const ENCRYPT_LOG_N: u8 = 18;
/// Highest scrypt work factor accepted when decrypting.
pub const MAX_DECRYPT_LOG_N: u8 = 20;

const ARMOR_BEGIN: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";

/// Encrypt `plaintext` to `passphrase`, returning ASCII-armored text.
pub fn encrypt(plaintext: &[u8], passphrase: &SecretString) -> Result<String> {
    encrypt_with_work_factor(plaintext, passphrase, ENCRYPT_LOG_N)
}

fn encrypt_with_work_factor(
    plaintext: &[u8],
    passphrase: &SecretString,
    log_n: u8,
) -> Result<String> {
    if plaintext.len() > limits::MAX_PLAINTEXT {
        return Err(err(
            Stage::Validate,
            "plaintext_too_large",
            "plaintext larger than 192 KiB",
        ));
    }
    let mut recipient = scrypt::Recipient::new(passphrase.clone());
    recipient.set_work_factor(log_n);
    let encryptor = Encryptor::with_recipients(iter::once(&recipient as _))
        .map_err(|e| err(Stage::Generic, "encrypt_failed", format!("age: {e}")))?;
    let mut out = Vec::with_capacity(plaintext.len() * 4 / 3 + 512);
    let io = |e: std::io::Error| err(Stage::Generic, "encrypt_failed", format!("age: {e}"));
    let armored = ArmoredWriter::wrap_output(&mut out, Format::AsciiArmor).map_err(io)?;
    let mut w = encryptor.wrap_output(armored).map_err(io)?;
    w.write_all(plaintext).map_err(io)?;
    w.finish().and_then(|a| a.finish()).map_err(io)?;
    if out.len() > limits::MAX_ENVELOPE {
        return Err(err(
            Stage::Validate,
            "envelope_too_large",
            "encrypted file larger than 256 KiB",
        ));
    }
    Ok(String::from_utf8(out).expect("armored output is ASCII"))
}

/// Decrypt an armored age file. The result is zeroized on drop.
pub fn decrypt(envelope: &[u8], passphrase: &SecretString) -> Result<Zeroizing<Vec<u8>>> {
    if envelope.len() > limits::MAX_ENVELOPE {
        return Err(err(
            Stage::Decrypt,
            "envelope_too_large",
            "remote file larger than 256 KiB",
        ));
    }
    // Armor only: a binary age file is not a valid stream object.
    let start = envelope
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(envelope.len());
    if !envelope[start..].starts_with(ARMOR_BEGIN) {
        return Err(err(
            Stage::Decrypt,
            "not_armored",
            "remote file is not an ASCII-armored age file",
        ));
    }
    let failed = || err(Stage::Decrypt, "decrypt_failed", "decryption failed");
    let decryptor = Decryptor::new_buffered(ArmoredReader::new(envelope)).map_err(|_| failed())?;
    if !decryptor.is_scrypt() {
        return Err(err(
            Stage::Decrypt,
            "not_passphrase",
            "age file is not passphrase-encrypted",
        ));
    }
    let mut identity = scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(MAX_DECRYPT_LOG_N);
    let reader = decryptor
        .decrypt(iter::once(&identity as _))
        .map_err(|e| match e {
            DecryptError::ExcessiveWork { required, .. } => err(
                Stage::Decrypt,
                "excessive_work",
                format!("scrypt work factor {required} exceeds limit {MAX_DECRYPT_LOG_N}"),
            ),
            _ => failed(),
        })?;
    let mut out = Zeroizing::new(Vec::with_capacity(envelope.len()));
    // Read at most one byte past the limit to detect oversize plaintext.
    reader
        .take(limits::MAX_PLAINTEXT as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| failed())?;
    if out.len() > limits::MAX_PLAINTEXT {
        return Err(err(
            Stage::Decrypt,
            "plaintext_too_large",
            "decrypted payload larger than 192 KiB",
        ));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn encrypt_fast(plaintext: &[u8], passphrase: &SecretString) -> String {
    // Tests use a tiny work factor; production always uses ENCRYPT_LOG_N.
    encrypt_with_work_factor(plaintext, passphrase, 2).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkp_mini_core::error::to_sync_error;

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn code(r: Result<Zeroizing<Vec<u8>>>) -> String {
        to_sync_error(&r.unwrap_err()).code
    }

    #[test]
    fn roundtrip() {
        let p = pw("correct horse battery staple");
        let armored = encrypt_fast(b"hello keybox", &p);
        assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(!armored.contains("hello"));
        assert_eq!(&**decrypt(armored.as_bytes(), &p).unwrap(), b"hello keybox");
    }

    #[test]
    fn production_work_factor_roundtrip() {
        let p = pw("pass phrase with spaces ");
        let armored = encrypt(b"x", &p).unwrap();
        assert_eq!(&**decrypt(armored.as_bytes(), &p).unwrap(), b"x");
    }

    #[test]
    fn wrong_password_rejected() {
        let armored = encrypt_fast(b"secret", &pw("right"));
        assert_eq!(
            code(decrypt(armored.as_bytes(), &pw("wrong"))),
            "decrypt_failed"
        );
        // Leading/trailing spaces are significant (never trimmed).
        assert_eq!(
            code(decrypt(armored.as_bytes(), &pw(" right"))),
            "decrypt_failed"
        );
    }

    #[test]
    fn tampered_rejected() {
        let p = pw("pw");
        let armored = encrypt_fast(&[0u8; 4096], &p);
        let lines: Vec<&str> = armored.lines().collect();
        // Flip one base64 char in the body (avoid header/footer lines).
        let mid = lines.len() / 2;
        let mut bad_line: Vec<u8> = lines[mid].as_bytes().to_vec();
        bad_line[10] = if bad_line[10] == b'A' { b'B' } else { b'A' };
        let mut out = lines[..mid].join("\n");
        out.push('\n');
        out.push_str(std::str::from_utf8(&bad_line).unwrap());
        out.push('\n');
        out.push_str(&lines[mid + 1..].join("\n"));
        out.push('\n');
        assert_eq!(code(decrypt(out.as_bytes(), &p)), "decrypt_failed");
    }

    #[test]
    fn truncated_rejected() {
        let p = pw("pw");
        let armored = encrypt_fast(&[0u8; 4096], &p);
        let lines: Vec<&str> = armored.lines().collect();
        let mut cut = lines[..lines.len() - 3].join("\n");
        cut.push_str("\n-----END AGE ENCRYPTED FILE-----\n");
        assert_eq!(code(decrypt(cut.as_bytes(), &p)), "decrypt_failed");
    }

    #[test]
    fn oversize_and_binary_rejected() {
        let p = pw("pw");
        let big = vec![b'A'; limits::MAX_ENVELOPE + 1];
        assert_eq!(code(decrypt(&big, &p)), "envelope_too_large");
        assert_eq!(code(decrypt(b"age-encryption.org/v1\n", &p)), "not_armored");
        assert!(encrypt(&vec![0u8; limits::MAX_PLAINTEXT + 1], &p).is_err());
    }
}
