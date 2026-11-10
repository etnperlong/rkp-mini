//! TrickyStore-compatible `keybox.xml` generation and inspection.

use std::path::Path;

use anyhow::Result;
use base64::Engine;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::LineEnding;
use p256::SecretKey;

use crate::error::{err, Stage};
use crate::rkp::chain::{self, CertInfo};
use crate::util;

/// Structured keybox status (used by `status` and the description logic).
#[derive(Debug, Clone, Default)]
pub struct KeyboxInfo {
    pub exists: bool,
    pub valid: bool,
    pub certificates: usize,
    pub leaf_subject: String,
    pub not_after: Option<i64>,
    pub key_match: bool,
    pub error: Option<String>,
}

const PEM_WRAP: usize = 64;

fn pem_block(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut body = String::new();
    for chunk in b64.as_bytes().chunks(PEM_WRAP) {
        body.push_str(std::str::from_utf8(chunk).unwrap());
        body.push('\n');
    }
    format!("-----BEGIN {label}-----\n{body}-----END {label}-----\n")
}

/// Concatenated PEM of a cert chain (for `cert-chain.pem`).
pub fn chain_pem(chain: &[CertInfo]) -> String {
    let mut out = String::new();
    for cert in chain {
        out.push_str(&pem_block("CERTIFICATE", &cert.der));
    }
    out
}

/// SEC1 `EC PRIVATE KEY` PEM for a secret key.
pub fn private_key_pem(secret: &SecretKey) -> Result<String> {
    secret
        .to_sec1_pem(LineEnding::LF)
        .map(|z| z.to_string())
        .map_err(|e| {
            err(
                Stage::Chain,
                "chain_invalid",
                format!("encode SEC1 key: {e}"),
            )
        })
}

/// Device id: `rkp-mini-` + first 12 hex of sha256(public key SEC1).
pub fn device_id(public: &p256::PublicKey) -> String {
    let sec1 = public.to_encoded_point(false);
    let hash = util::sha256_hex(sec1.as_bytes());
    format!("rkp-mini-{}", &hash[..12])
}

/// Build the keybox XML (SEC1 private key + leaf→root cert chain).
pub fn build(secret: &SecretKey, chain: &[CertInfo], device_id: &str) -> Result<String> {
    let private_pem = secret.to_sec1_pem(LineEnding::LF).map_err(|e| {
        err(
            Stage::Chain,
            "chain_invalid",
            format!("encode SEC1 key: {e}"),
        )
    })?;

    let mut certs_xml = String::new();
    for cert in chain {
        let pem = pem_block("CERTIFICATE", &cert.der);
        certs_xml.push_str("                <Certificate format=\"pem\">\n");
        certs_xml.push_str(&pem);
        certs_xml.push_str("                </Certificate>\n");
    }

    let xml = format!(
        "<?xml version=\"1.0\"?>\n\
<AndroidAttestation>\n\
    <NumberOfKeyboxes>1</NumberOfKeyboxes>\n\
    <Keybox DeviceID=\"{device_id}\">\n\
        <Key algorithm=\"ecdsa\">\n\
            <PrivateKey format=\"pem\">\n\
{private}\
            </PrivateKey>\n\
            <CertificateChain>\n\
                <NumberOfCertificates>{n}</NumberOfCertificates>\n\
{certs}\
            </CertificateChain>\n\
        </Key>\n\
    </Keybox>\n\
</AndroidAttestation>\n",
        device_id = util::xml_escape(device_id),
        private = &*private_pem,
        n = chain.len(),
        certs = certs_xml,
    );
    Ok(xml)
}

/// Extract base64 bodies of all `BEGIN/END <label>` blocks from text.
fn extract_pem_blocks(text: &str, label: &str) -> Vec<Vec<u8>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(b) = rest.find(&begin) {
        let after = &rest[b + begin.len()..];
        let Some(e) = after.find(&end) else { break };
        let body: String = after[..e].chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(der) = base64::engine::general_purpose::STANDARD.decode(body.as_bytes()) {
            out.push(der);
        }
        rest = &after[e + end.len()..];
    }
    out
}

/// Inspect a keybox.xml for status display (spec §9.13). Never panics.
pub fn inspect(path: &Path) -> KeyboxInfo {
    let mut info = KeyboxInfo::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return info, // exists = false
    };
    info.exists = true;

    let cert_ders = extract_pem_blocks(&text, "CERTIFICATE");
    if cert_ders.is_empty() {
        info.error = Some("no certificates in keybox".into());
        return info;
    }
    let mut concat = Vec::new();
    for d in &cert_ders {
        concat.extend_from_slice(d);
    }
    let certs = match chain::parse_der_chain(&concat) {
        Ok(c) => c,
        Err(e) => {
            info.error = Some(crate::error::to_app_error(&e).message);
            return info;
        }
    };
    info.certificates = certs.len();
    let order = match chain::order_leaf_to_root(&certs) {
        Ok(o) => o,
        Err(e) => {
            info.error = Some(crate::error::to_app_error(&e).message);
            return info;
        }
    };
    let leaf = &certs[order[0]];
    info.leaf_subject = leaf.subject_cn.clone();
    info.not_after = Some(leaf.not_after);

    // Private key match (if present).
    let key_ders = extract_pem_blocks(&text, "EC PRIVATE KEY");
    if let Some(der) = key_ders.first() {
        match SecretKey::from_sec1_der(der) {
            Ok(sk) => {
                let pk = sk.public_key();
                info.key_match = chain::ensure_leaf_matches(leaf, &pk).is_ok();
                if !info.key_match {
                    info.error = Some("private key does not match leaf".into());
                }
            }
            Err(e) => info.error = Some(format!("bad private key: {e}")),
        }
    } else {
        info.key_match = false;
        info.error = Some("no matching private key".into());
    }

    info.valid = info.error.is_none() && info.key_match;
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_chain, TestChain};

    #[test]
    fn build_inspect_roundtrip() {
        let TestChain {
            der,
            secret,
            public,
            ..
        } = test_chain();
        let certs = chain::prepare(&der, &public, util::now_unix()).unwrap();
        let did = device_id(&public);
        let xml = build(&secret, &certs, &did).unwrap();
        assert!(xml.contains("EC PRIVATE KEY"));
        assert!(xml.contains(&did));
        assert!(xml.contains("<NumberOfCertificates>2</NumberOfCertificates>"));

        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), &xml).unwrap();
        let info = inspect(f.path());
        assert!(info.exists);
        assert!(info.valid, "error: {:?}", info.error);
        assert!(info.key_match);
        assert_eq!(info.certificates, 2);
        assert_eq!(info.not_after, Some(certs[0].not_after));
    }

    #[test]
    fn device_id_stable() {
        let TestChain { public, .. } = test_chain();
        let a = device_id(&public);
        let b = device_id(&public);
        assert_eq!(a, b);
        assert!(a.starts_with("rkp-mini-"));
        assert_eq!(a.len(), "rkp-mini-".len() + 12);
    }

    #[test]
    fn inspect_missing_key_invalid() {
        let TestChain { der, public, .. } = test_chain();
        let certs = chain::prepare(&der, &public, util::now_unix()).unwrap();
        // XML with only certs, no private key
        let mut xml = String::from("<AndroidAttestation>\n");
        for c in &certs {
            xml.push_str(&pem_block("CERTIFICATE", &c.der));
        }
        xml.push_str("</AndroidAttestation>\n");
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), &xml).unwrap();
        let info = inspect(f.path());
        assert!(info.exists);
        assert!(!info.valid);
        assert!(!info.key_match);
    }

    #[test]
    fn xml_escaping() {
        let TestChain {
            der,
            secret,
            public,
            ..
        } = test_chain();
        let certs = chain::prepare(&der, &public, util::now_unix()).unwrap();
        let xml = build(&secret, &certs, "a&b<c>\"d'").unwrap();
        assert!(xml.contains("a&amp;b&lt;c&gt;&quot;d&apos;"));
    }
}
