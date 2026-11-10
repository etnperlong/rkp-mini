//! OhMyKeymint compatibility precheck (docs/REMOTE_SYNC_DEVELOP.md §4.2, §11.4).
//!
//! OhMyKeymint does not parse keybox XML: it scans the raw text with four
//! regexes derived from **OhMyKeymint 1.2.6, commit `9069c6c`** (`src/keybox.rs`:
//! `KEY_BLOCK_RE`, `PRIVATE_KEY_RE`, `CERT_COUNT_RE`, `CERT_RE`, `decode_pem`,
//! `from_xml_str`) plus `common/src/crypto/ec.rs::import_sec1_private_key`. On
//! any parse failure it overwrites the active keybox with its bundled template,
//! so a keybox that is valid for [`rkp_mini_core::keybox::validate_structure`]
//! (a strict XML DOM check) but not for that scanner would be silently replaced.
//! This module answers exactly one question: would OhMyKeymint load *the same
//! private key and certificate chain* the deployer validated?
//!
//! Rule: this precheck may be **stricter** than OhMyKeymint, never looser. Any
//! doubt (a form OhMyKeymint reads differently, or that this scanner cannot
//! prove identical) rejects the target with [`crate::deploy`]'s
//! `omk_incompatible` code instead of writing it.
//!
//! Re-sync this file whenever OhMyKeymint's `src/keybox.rs` scanner or
//! `import_sec1_private_key` changes; the emulation below tracks the revision
//! quoted above, not the newest upstream commit.
//!
//! Known residual (not implemented): OhMyKeymint also compares the leaf
//! certificate's full SubjectPublicKeyInfo DER with the one BoringSSL derives
//! from the private key. A leaf whose SPKI uses a non-named-curve
//! `AlgorithmIdentifier` (e.g. explicit EC parameters) is not detected here;
//! the key/DeviceID comparison below catches a different key, not a different
//! SPKI encoding of the same key.

use std::str;

use base64::Engine;
use der::asn1::ObjectIdentifier;
use der::Decode;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use rkp_mini_core::keybox::KeyboxFacts;
use rkp_mini_core::{sha256, sha256_hex};
use zeroize::Zeroizing;

/// secp256r1 / prime256v1 (`1.2.840.10045.3.1.7`), the only curve OMK imports.
const SECP256R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

// Fixed, secret-free reasons: no keybox text, base64 or key bytes ever appear
// in an error, log or panic message (AGENTS.md "Secrets").
const NOT_UTF8: &str = "the keybox is not UTF-8 text";
const NO_KEY_BLOCK: &str =
    "no `<Key algorithm=\"ecdsa\">` block in the literal form OhMyKeymint scans for";
const MANY_KEY_BLOCKS: &str = "OhMyKeymint would see more than one <Key> block";
const WRONG_ALGORITHM: &str =
    "the <Key> algorithm is not the EC-only algorithm OhMyKeymint accepts";
const NO_PRIVATE_KEY: &str = "missing <PrivateKey> block in the literal form OhMyKeymint scans for";
const PEM: &str = "a PEM payload is not plain base64 lines between BEGIN/END lines";
const NO_CERT_COUNT: &str = "missing or malformed <NumberOfCertificates>";
const CERT_COUNT: &str = "the number of certificates differs from the count OhMyKeymint reads";
const CHAIN: &str = "OhMyKeymint would read a different certificate chain than the validated one";
const KEY_PARAMS: &str = "the EC private key lacks the named-curve parameters OhMyKeymint requires";
const KEY: &str = "OhMyKeymint would read a different private key than the validated one";

/// Would OhMyKeymint load exactly the key and chain described by `facts`?
///
/// `Err` carries a fixed, secret-free reason for `omk_incompatible`.
pub fn check(xml: &[u8], facts: &KeyboxFacts) -> Result<(), &'static str> {
    let text = str::from_utf8(xml).map_err(|_| NOT_UTF8)?;

    let blocks = scan_key_blocks(text);
    if blocks.is_empty() {
        return Err(NO_KEY_BLOCK);
    }
    if blocks.len() > 1 {
        return Err(MANY_KEY_BLOCKS);
    }
    let block = &blocks[0];
    if !matches!(block.algorithm.trim(), "ecdsa" | "ec") {
        return Err(WRONG_ALGORITHM);
    }

    let private = private_key_body(block.body).ok_or(NO_PRIVATE_KEY)?;
    let key_der = Zeroizing::new(decode_pem(private).ok_or(PEM)?);

    let declared = cert_count(block.body).ok_or(NO_CERT_COUNT)?;
    let payloads = scan_certificates(block.body);
    if payloads.is_empty() || payloads.len() != declared {
        return Err(CERT_COUNT);
    }
    let mut chain = Vec::with_capacity(payloads.len());
    for payload in payloads {
        chain.push(decode_pem(payload).ok_or(PEM)?);
    }
    let mut concatenated = Vec::new();
    for der in &chain {
        concatenated.extend_from_slice(der);
    }
    if sha256(&concatenated) != facts.chain_sha256 {
        return Err(CHAIN);
    }

    // R6: same key, in a form OMK's `import_sec1_private_key` accepts.
    let ec = sec1::EcPrivateKey::from_der(key_der.as_slice()).map_err(|_| KEY_PARAMS)?;
    let parameters = ec.parameters.as_ref().ok_or(KEY_PARAMS)?;
    if parameters.named_curve() != Some(SECP256R1) {
        return Err(KEY_PARAMS);
    }
    let secret = SecretKey::from_sec1_der(key_der.as_slice()).map_err(|_| KEY)?;
    let public = secret.public_key().to_encoded_point(false);
    let derived = format!("rkp-mini-{}", &sha256_hex(public.as_bytes())[..12]);
    if derived != facts.device_id {
        return Err(KEY);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Regex emulation
// ---------------------------------------------------------------------------

/// `KEY_BLOCK_RE = (?s)<Key\s+algorithm="([^"]+)">\s*(.*?)\s*</Key>`.
struct KeyBlock<'a> {
    /// Capture 1, untrimmed (callers apply `trim()` like upstream).
    algorithm: &'a str,
    /// Capture 2: text between the opening tag and `</Key>`, trailing
    /// whitespace removed (upstream's greedy `\s*` before the closing tag).
    body: &'a str,
    /// Index just past `</Key>`: where `captures_iter` resumes.
    end: usize,
}

/// One `KEY_BLOCK_RE` attempt anchored at the `<Key` occurrence at `start`.
fn key_block_at(text: &str, start: usize) -> Option<KeyBlock<'_>> {
    let prefix = "<Key";
    let after_tag = start + prefix.len();
    let whitespace_end = skip_whitespace(text, after_tag);
    if whitespace_end == after_tag {
        return None;
    }
    let quoted = text[whitespace_end..].strip_prefix("algorithm=\"")?;
    let value_len = quoted.find('"')?;
    if value_len == 0 {
        return None;
    }
    let value_end = whitespace_end + "algorithm=\"".len() + value_len;
    if !text[value_end..].starts_with("\">") {
        return None;
    }
    let body_start = skip_whitespace(text, value_end + 2);
    let body_end = body_start + text[body_start..].find("</Key>")?;
    Some(KeyBlock {
        algorithm: &text[whitespace_end + "algorithm=\"".len()..value_end],
        body: text[body_start..body_end].trim_end(),
        end: body_end + "</Key>".len(),
    })
}

/// `KEY_BLOCK_RE.captures_iter`: leftmost, non-overlapping matches.
fn scan_key_blocks(text: &str) -> Vec<KeyBlock<'_>> {
    let mut blocks = Vec::new();
    let mut pos = 0;
    while let Some(offset) = text[pos..].find("<Key") {
        let start = pos + offset;
        match key_block_at(text, start) {
            Some(block) => {
                pos = block.end;
                blocks.push(block);
            }
            // A failed candidate start is skipped; `<Key` cannot overlap
            // itself, so resuming at `start + 1` visits every candidate.
            None => pos = start + 1,
        }
    }
    blocks
}

/// `PRIVATE_KEY_RE = (?s)<PrivateKey[^>]*>\s*(.*?)\s*</PrivateKey>`, `captures`.
fn private_key_body(block: &str) -> Option<&str> {
    let mut pos = 0;
    while let Some(offset) = block[pos..].find("<PrivateKey") {
        let start = pos + offset;
        let after_tag = start + "<PrivateKey".len();
        // `[^>]*` cannot cross `>`, so the first `>` closes the open tag.
        if let Some(gt) = block[after_tag..].find('>') {
            let body_start = skip_whitespace(block, after_tag + gt + 1);
            if let Some(close) = block[body_start..].find("</PrivateKey>") {
                return Some(block[body_start..body_start + close].trim_end());
            }
        }
        pos = start + 1;
    }
    None
}

/// `CERT_COUNT_RE` on the first occurrence only (stricter than upstream, which
/// could fall through to a later occurrence after a malformed one).
fn cert_count(body: &str) -> Option<usize> {
    let open = "<NumberOfCertificates>";
    let close = "</NumberOfCertificates>";
    let start = body.find(open)? + open.len();
    let end = start + body[start..].find(close)?;
    let raw = body[start..end].trim();
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<usize>().ok()
}

/// `CERT_RE = (?s)<Certificate(?:\s+[^>]*)?>\s*(.*?)\s*</Certificate>`,
/// `captures_iter`: leftmost, non-overlapping payload captures.
fn scan_certificates(body: &str) -> Vec<&str> {
    let mut payloads = Vec::new();
    let mut pos = 0;
    while let Some(offset) = body[pos..].find("<Certificate") {
        let start = pos + offset;
        let Some(tag_end) = certificate_tag_end(body, start + "<Certificate".len()) else {
            // `<CertificateChain` and friends: failed candidate, keep scanning.
            pos = start + 1;
            continue;
        };
        let body_start = skip_whitespace(body, tag_end);
        // No closing tag after this candidate means no later candidate can
        // match either, so the scan ends here.
        let Some(close) = body[body_start..].find("</Certificate>") else {
            break;
        };
        let payload_end = body_start + close;
        payloads.push(body[body_start..payload_end].trim_end());
        pos = payload_end + "</Certificate>".len();
    }
    payloads
}

/// `(?:\s+[^>]*)?>` after `<Certificate`: either `>` directly, or at least one
/// whitespace character followed by anything up to the next `>`.
fn certificate_tag_end(text: &str, after_tag: usize) -> Option<usize> {
    if text[after_tag..].starts_with('>') {
        return Some(after_tag + 1);
    }
    if !text[after_tag..].chars().next()?.is_whitespace() {
        return None;
    }
    Some(after_tag + text[after_tag..].find('>')? + 1)
}

/// First index at or after `pos` whose char is not `char::is_whitespace`.
/// Rust's `\s` (Unicode `White_Space`) and `char::is_whitespace` are the same set.
fn skip_whitespace(text: &str, pos: usize) -> usize {
    for (offset, c) in text[pos..].char_indices() {
        if !c.is_whitespace() {
            return pos + offset;
        }
    }
    text.len()
}

/// Exact port of OhMyKeymint's `decode_pem`: per-line `trim`, drop empty and
/// `-----BEGIN …`/`-----END …` lines, concatenate, base64 `STANDARD`.
fn decode_pem(pem: &str) -> Option<Vec<u8>> {
    let mut base64_body = String::new();
    for line in pem.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("-----BEGIN ") || line.starts_with("-----END ") {
            continue;
        }
        base64_body.push_str(line);
    }
    if base64_body.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(base64_body.as_bytes())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::pkcs8::LineEnding;
    use rkp_mini_core::keybox::validate_structure;
    use rkp_mini_core::test_support::{keybox_xml, TestKeybox};

    fn facts_of(xml: &str) -> KeyboxFacts {
        validate_structure(xml.as_bytes()).expect("variant must stay structurally valid")
    }

    #[test]
    fn accepts_the_issuer_layout() {
        let kb = TestKeybox::new(300);
        let facts = facts_of(&kb.xml);
        assert_eq!(check(kb.xml.as_bytes(), &facts), Ok(()));
    }

    #[test]
    fn rejects_a_keybox_whose_key_omits_the_named_curve() {
        // The pre-fix layout: `SecretKey::to_sec1_pem` writes `parameters: None`
        // and OhMyKeymint's `import_sec1_private_key` refuses it.
        let kb = TestKeybox::new(300);
        let old = SecretKey::from_sec1_pem(&kb.key_pem)
            .unwrap()
            .to_sec1_pem(LineEnding::LF)
            .unwrap()
            .to_string();
        let variant = kb.xml.replace(&kb.key_pem, &old);
        // Our DOM validator accepts it: a proven divergence.
        let facts = facts_of(&variant);
        assert_eq!(check(variant.as_bytes(), &facts), Err(KEY_PARAMS));
    }

    #[test]
    fn rejects_a_missing_certificate_count() {
        let kb = TestKeybox::new(300);
        let count = format!(
            "<NumberOfCertificates>{}</NumberOfCertificates>\n",
            kb.chain_pems.len()
        );
        let variant = kb.xml.replace(&count, "");
        let facts = facts_of(&variant);
        assert_eq!(check(variant.as_bytes(), &facts), Err(NO_CERT_COUNT));
    }

    #[test]
    fn rejects_single_quoted_and_spaced_algorithm_attributes() {
        let kb = TestKeybox::new(300);
        let single_quoted = kb.xml.replace("algorithm=\"ecdsa\"", "algorithm='ecdsa'");
        assert_eq!(
            check(single_quoted.as_bytes(), &facts_of(&single_quoted)),
            Err(NO_KEY_BLOCK)
        );

        let spaced = kb
            .xml
            .replace("algorithm=\"ecdsa\">", "algorithm=\"ecdsa\" >");
        assert_eq!(
            check(spaced.as_bytes(), &facts_of(&spaced)),
            Err(NO_KEY_BLOCK)
        );

        let reordered = kb.xml.replace(
            "<Key algorithm=\"ecdsa\">",
            "<Key x=\"1\" algorithm=\"ecdsa\">",
        );
        assert_eq!(
            check(reordered.as_bytes(), &facts_of(&reordered)),
            Err(NO_KEY_BLOCK)
        );
    }

    #[test]
    fn rejects_a_certificate_pem_on_one_line() {
        let kb = TestKeybox::new(300);
        let mut pems = kb.chain_pems.clone();
        pems[0] = pems[0].lines().map(str::trim).collect::<String>();
        assert!(!pems[0].contains('\n'));
        let variant = keybox_xml(&kb.key_pem, &pems, &kb.device_id);
        assert_eq!(check(variant.as_bytes(), &facts_of(&variant)), Err(PEM));
    }

    #[test]
    fn rejects_base64_whitespace_inside_a_line() {
        let kb = TestKeybox::new(300);
        let mut lines: Vec<String> = kb.chain_pems[0].lines().map(str::to_string).collect();
        // Insert a space in the middle of the first base64 line.
        lines[1].insert(4, ' ');
        let mut collapsed = String::new();
        for line in &lines {
            collapsed.push_str(line);
            collapsed.push('\n');
        }
        let mut pems = kb.chain_pems.clone();
        pems[0] = collapsed;
        let variant = keybox_xml(&kb.key_pem, &pems, &kb.device_id);
        assert_eq!(check(variant.as_bytes(), &facts_of(&variant)), Err(PEM));
    }

    #[test]
    fn rejects_a_key_block_hidden_in_an_xml_comment() {
        let kb = TestKeybox::new(300);
        let comment = "    <!-- <Key algorithm=\"rsa\"><PrivateKey>x</PrivateKey></Key> -->\n";
        let variant = kb.xml.replace("<Keybox ", &format!("{comment}<Keybox "));
        let facts = facts_of(&variant);
        assert_eq!(check(variant.as_bytes(), &facts), Err(MANY_KEY_BLOCKS));
    }

    #[test]
    fn rejects_a_certificate_wrapped_in_cdata() {
        let kb = TestKeybox::new(300);
        let variant = kb
            .xml
            .replace(
                "                <Certificate format=\"pem\">\n",
                "                <Certificate format=\"pem\"><![CDATA[\n",
            )
            .replace(
                "                </Certificate>\n",
                "                ]]></Certificate>\n",
            );
        let facts = facts_of(&variant);
        assert_eq!(check(variant.as_bytes(), &facts), Err(PEM));
    }

    #[test]
    fn rejects_a_different_chain() {
        let staged = TestKeybox::new(300);
        let other = TestKeybox::new(300);
        let other_facts = facts_of(&other.xml);
        assert_eq!(
            check(staged.xml.as_bytes(), &other_facts),
            Err(CHAIN),
            "the keybox text is valid but is not the chain described by the facts"
        );
    }
}
