//! Strict keybox XML validation (design §11.3), shared by sender and receiver.
//!
//! The accepted shape is exactly what `rkp-mini` produces:
//!
//! ```text
//! <AndroidAttestation>
//!   [<NumberOfKeyboxes>1</NumberOfKeyboxes>]
//!   <Keybox DeviceID="rkp-mini-<12 hex>">
//!     <Key algorithm="ecdsa">
//!       <PrivateKey format="pem">SEC1 EC PRIVATE KEY</PrivateKey>
//!       <CertificateChain>
//!         [<NumberOfCertificates>N</NumberOfCertificates>]
//!         <Certificate format="pem">…</Certificate> × 2..=8, leaf→root
//! ```
//!
//! DTDs/entities are rejected before and during parsing. Certificate
//! signatures, root trust and revocation are **not** checked; callers must say
//! so in user-facing output.

use anyhow::Result;
use base64::Engine;
use der::{Decode, Encode};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use roxmltree::{Document, Node, ParsingOptions};
use x509_cert::Certificate;
use zeroize::Zeroizing;

use crate::error::{err, Stage};
use crate::{limits, sha256, sha256_hex};

pub const MIN_CERTS: usize = 2;
pub const MAX_CERTS: usize = 8;
/// Allowed clock skew for `not_before` (seconds).
pub const NOT_BEFORE_SKEW: i64 = 300;

/// Public facts derived from a validated keybox. Contains no key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboxFacts {
    pub sha256: [u8; 32],
    pub device_id: String,
    pub leaf_not_before: i64,
    pub leaf_not_after: i64,
    /// SHA-256 over the concatenated DER certificates, leaf→root.
    pub chain_sha256: [u8; 32],
    pub certificates: usize,
    pub leaf_subject: String,
}

/// Validity requirements applied after structural checks.
#[derive(Debug, Clone, Copy)]
pub struct Validity {
    pub now: i64,
    /// Leaf must remain valid for at least this many seconds after `now`.
    pub min_remaining_secs: i64,
}

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    err(Stage::Validate, "keybox_invalid", msg)
}

/// Parse and validate keybox bytes; returns derived public facts.
pub fn validate(xml: &[u8], validity: Validity) -> Result<KeyboxFacts> {
    let facts = validate_structure(xml)?;
    if facts.leaf_not_before > validity.now + NOT_BEFORE_SKEW {
        return Err(err(
            Stage::Validate,
            "keybox_not_yet_valid",
            "leaf not_before is in the future",
        ));
    }
    if facts.leaf_not_after < validity.now.saturating_add(validity.min_remaining_secs) {
        return Err(err(
            Stage::Validate,
            "keybox_expiring",
            format!(
                "leaf expires at {} which is within the minimum validity window",
                facts.leaf_not_after
            ),
        ));
    }
    Ok(facts)
}

/// Structural/cryptographic-binding checks only (no clock checks).
pub fn validate_structure(xml: &[u8]) -> Result<KeyboxFacts> {
    if xml.is_empty() || xml.len() > limits::MAX_KEYBOX_XML {
        return Err(invalid("keybox empty or larger than 128 KiB"));
    }
    let text = std::str::from_utf8(xml).map_err(|_| invalid("keybox is not UTF-8"))?;
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") || text.contains("<!doctype") {
        return Err(invalid("DTD/entity declarations are not allowed"));
    }
    let opts = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 4096,
    };
    let doc =
        Document::parse_with_options(text, opts).map_err(|e| invalid(format!("XML parse: {e}")))?;

    let root = doc.root_element();
    expect_name(root, "AndroidAttestation")?;
    let mut keybox = None;
    for child in elements(root) {
        match child.tag_name().name() {
            "NumberOfKeyboxes" => {
                if node_text(child).trim() != "1" {
                    return Err(invalid("NumberOfKeyboxes must be 1"));
                }
            }
            "Keybox" => {
                if keybox.replace(child).is_some() {
                    return Err(invalid("exactly one <Keybox> is required"));
                }
            }
            other => return Err(invalid(format!("unexpected element <{other}>"))),
        }
    }
    let keybox = keybox.ok_or_else(|| invalid("missing <Keybox>"))?;
    let device_id = keybox
        .attribute("DeviceID")
        .ok_or_else(|| invalid("missing Keybox DeviceID"))?;

    let mut key = None;
    for child in elements(keybox) {
        expect_name(child, "Key")?;
        match child.attribute("algorithm") {
            Some("ecdsa") => {}
            Some(a) => {
                return Err(invalid(format!(
                    "unsupported key algorithm `{a}`; only the EC-only RKP Mini layout is accepted"
                )))
            }
            None => return Err(invalid("Key without algorithm")),
        }
        if key.replace(child).is_some() {
            return Err(invalid("exactly one <Key algorithm=\"ecdsa\"> is required"));
        }
    }
    let key = key.ok_or_else(|| invalid("no <Key algorithm=\"ecdsa\">"))?;

    let mut private = None;
    let mut chain_node = None;
    for child in elements(key) {
        match child.tag_name().name() {
            "PrivateKey" => {
                if private.replace(child).is_some() {
                    return Err(invalid("duplicate <PrivateKey>"));
                }
            }
            "CertificateChain" => {
                if chain_node.replace(child).is_some() {
                    return Err(invalid("duplicate <CertificateChain>"));
                }
            }
            other => return Err(invalid(format!("unexpected element <{other}> in <Key>"))),
        }
    }
    let private = private.ok_or_else(|| invalid("missing <PrivateKey>"))?;
    let chain_node = chain_node.ok_or_else(|| invalid("missing <CertificateChain>"))?;
    expect_pem_format(private)?;
    let key_der = Zeroizing::new(single_pem(&node_text(private), "EC PRIVATE KEY")?);
    let secret = SecretKey::from_sec1_der(&key_der)
        .map_err(|_| invalid("PrivateKey is not a SEC1 P-256 key"))?;
    let public_sec1 = secret.public_key().to_encoded_point(false);

    let mut declared = None;
    let mut certs = Vec::new();
    for child in elements(chain_node) {
        match child.tag_name().name() {
            "NumberOfCertificates" => {
                let n: usize = node_text(child)
                    .trim()
                    .parse()
                    .map_err(|_| invalid("NumberOfCertificates is not a number"))?;
                if declared.replace(n).is_some() {
                    return Err(invalid("duplicate <NumberOfCertificates>"));
                }
            }
            "Certificate" => {
                expect_pem_format(child)?;
                if certs.len() == MAX_CERTS {
                    return Err(invalid("more than 8 certificates"));
                }
                certs.push(parse_cert(&single_pem(&node_text(child), "CERTIFICATE")?)?);
            }
            other => {
                return Err(invalid(format!(
                    "unexpected element <{other}> in <CertificateChain>"
                )))
            }
        }
    }
    if certs.len() < MIN_CERTS {
        return Err(invalid("certificate chain needs at least 2 certificates"));
    }
    if declared.is_some_and(|n| n != certs.len()) {
        return Err(invalid("NumberOfCertificates does not match the chain"));
    }
    for pair in certs.windows(2) {
        if pair[0].issuer != pair[1].subject {
            return Err(invalid("certificate chain is not ordered leaf→root"));
        }
    }
    let root = certs.last().expect("len >= 2");
    if root.issuer != root.subject {
        return Err(invalid("last certificate is not a self-issued root"));
    }
    let leaf = &certs[0];
    if leaf.public_key != public_sec1.as_bytes() {
        return Err(invalid("private key does not match the leaf certificate"));
    }
    let derived = format!("rkp-mini-{}", &sha256_hex(public_sec1.as_bytes())[..12]);
    if device_id != derived {
        return Err(invalid("DeviceID does not match the key-derived id"));
    }

    let mut concat = Vec::new();
    for c in &certs {
        concat.extend_from_slice(&c.der);
    }
    Ok(KeyboxFacts {
        sha256: sha256(xml),
        device_id: derived,
        leaf_not_before: leaf.not_before,
        leaf_not_after: leaf.not_after,
        chain_sha256: sha256(&concat),
        certificates: certs.len(),
        leaf_subject: leaf.subject_text.clone(),
    })
}

struct Cert {
    der: Vec<u8>,
    subject: Vec<u8>,
    issuer: Vec<u8>,
    subject_text: String,
    not_before: i64,
    not_after: i64,
    public_key: Vec<u8>,
}

fn parse_cert(der_bytes: &[u8]) -> Result<Cert> {
    let cert =
        Certificate::from_der(der_bytes).map_err(|e| invalid(format!("certificate DER: {e}")))?;
    let tbs = &cert.tbs_certificate;
    let enc = |n: &x509_cert::name::Name| n.to_der().map_err(|_| invalid("certificate name"));
    Ok(Cert {
        der: der_bytes.to_vec(),
        subject: enc(&tbs.subject)?,
        issuer: enc(&tbs.issuer)?,
        subject_text: tbs.subject.to_string(),
        not_before: tbs.validity.not_before.to_unix_duration().as_secs() as i64,
        not_after: tbs.validity.not_after.to_unix_duration().as_secs() as i64,
        public_key: tbs
            .subject_public_key_info
            .subject_public_key
            .raw_bytes()
            .to_vec(),
    })
}

fn elements<'a, 'i>(n: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(|c| c.is_element())
}

fn expect_name(n: Node, want: &str) -> Result<()> {
    if n.tag_name().namespace().is_some() || n.tag_name().name() != want {
        return Err(invalid(format!(
            "expected <{want}>, found <{}>",
            n.tag_name().name()
        )));
    }
    Ok(())
}

fn expect_pem_format(n: Node) -> Result<()> {
    match n.attribute("format") {
        Some("pem") | None => Ok(()),
        Some(_) => Err(invalid("only format=\"pem\" is supported")),
    }
}

/// Concatenated text content of an element (no child elements allowed).
fn node_text(n: Node) -> String {
    n.children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .collect()
}

/// Exactly one PEM block with `label`; nothing but whitespace around it.
fn single_pem(text: &str, label: &str) -> Result<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let t = text.trim();
    let body = t
        .strip_prefix(&begin)
        .and_then(|r| r.strip_suffix(&end))
        .ok_or_else(|| invalid(format!("expected a single {label} PEM block")))?;
    if body.contains("-----") {
        return Err(invalid(format!("expected a single {label} PEM block")));
    }
    let b64: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| invalid(format!("{label} PEM body is not valid base64")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_sync_error;
    use crate::test_support::{keybox_xml, TestKeybox};

    const NOW_SLACK: Validity = Validity {
        now: 0,
        min_remaining_secs: 0,
    };

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn code(r: Result<KeyboxFacts>) -> String {
        to_sync_error(&r.unwrap_err()).code
    }

    #[test]
    fn accepts_rkp_mini_layout() {
        let kb = TestKeybox::new(30);
        let facts = validate(
            kb.xml.as_bytes(),
            Validity {
                now: now(),
                min_remaining_secs: 7 * crate::DAY,
            },
        )
        .unwrap();
        assert_eq!(facts.device_id, kb.device_id);
        assert_eq!(facts.certificates, 2);
        assert_eq!(facts.sha256, sha256(kb.xml.as_bytes()));
        assert_eq!(facts.chain_sha256, sha256(&kb.chain_der));
    }

    #[test]
    fn rejects_expiring_and_future() {
        let kb = TestKeybox::new(5);
        let r = validate(
            kb.xml.as_bytes(),
            Validity {
                now: now(),
                min_remaining_secs: 7 * crate::DAY,
            },
        );
        assert_eq!(code(r), "keybox_expiring");
        let r = validate(
            kb.xml.as_bytes(),
            Validity {
                now: now() - 30 * crate::DAY,
                min_remaining_secs: 0,
            },
        );
        assert_eq!(code(r), "keybox_not_yet_valid");
    }

    #[test]
    fn rejects_dtd_and_entities() {
        let kb = TestKeybox::new(30);
        let with_dtd = kb.xml.replacen(
            "<?xml version=\"1.0\"?>\n",
            "<?xml version=\"1.0\"?>\n<!DOCTYPE x [<!ENTITY e SYSTEM \"file:///etc/passwd\">]>\n",
            1,
        );
        assert_eq!(
            code(validate(with_dtd.as_bytes(), NOW_SLACK)),
            "keybox_invalid"
        );
    }

    #[test]
    fn rejects_oversize() {
        let big = vec![b' '; limits::MAX_KEYBOX_XML + 1];
        assert_eq!(code(validate(&big, NOW_SLACK)), "keybox_invalid");
    }

    #[test]
    fn rejects_mismatched_key() {
        let a = TestKeybox::new(30);
        let b = TestKeybox::new(30);
        // Keep a's chain + DeviceID, swap in b's private key.
        let xml = keybox_xml(&b.key_pem, &a.chain_pems, &a.device_id);
        let e = to_sync_error(&validate(xml.as_bytes(), NOW_SLACK).unwrap_err());
        assert!(
            e.message.contains("does not match the leaf"),
            "{}",
            e.message
        );
    }

    #[test]
    fn rejects_wrong_device_id() {
        let kb = TestKeybox::new(30);
        let xml = keybox_xml(&kb.key_pem, &kb.chain_pems, "rkp-mini-000000000000");
        let e = to_sync_error(&validate(xml.as_bytes(), NOW_SLACK).unwrap_err());
        assert!(e.message.contains("DeviceID"), "{}", e.message);
    }

    #[test]
    fn rejects_bad_chain_shapes() {
        let kb = TestKeybox::new(30);
        // Single certificate.
        let xml = keybox_xml(&kb.key_pem, &kb.chain_pems[..1], &kb.device_id);
        assert_eq!(code(validate(xml.as_bytes(), NOW_SLACK)), "keybox_invalid");
        // Root→leaf order.
        let rev: Vec<String> = kb.chain_pems.iter().rev().cloned().collect();
        let xml = keybox_xml(&kb.key_pem, &rev, &kb.device_id);
        let e = to_sync_error(&validate(xml.as_bytes(), NOW_SLACK).unwrap_err());
        assert!(e.message.contains("leaf→root"), "{}", e.message);
        // Nine certificates.
        let many: Vec<String> = (0..9).map(|_| kb.chain_pems[0].clone()).collect();
        let xml = keybox_xml(&kb.key_pem, &many, &kb.device_id);
        assert_eq!(code(validate(xml.as_bytes(), NOW_SLACK)), "keybox_invalid");
    }

    #[test]
    fn rejects_rsa_and_extra_elements() {
        let kb = TestKeybox::new(30);
        let rsa = kb.xml.replace("algorithm=\"ecdsa\"", "algorithm=\"rsa\"");
        assert_eq!(code(validate(rsa.as_bytes(), NOW_SLACK)), "keybox_invalid");
        let extra = kb.xml.replace("<Keybox ", "<Evil/><Keybox ");
        assert_eq!(
            code(validate(extra.as_bytes(), NOW_SLACK)),
            "keybox_invalid"
        );
        let pkcs8 = kb.xml.replace("EC PRIVATE KEY", "PRIVATE KEY");
        assert_eq!(
            code(validate(pkcs8.as_bytes(), NOW_SLACK)),
            "keybox_invalid"
        );
    }
}
