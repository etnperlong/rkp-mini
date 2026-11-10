//! DER chain splitting, leaf→root ordering, and leaf public-key binding.

use anyhow::Result;
use der::{Decode, Encode};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::PublicKey;
use x509_cert::Certificate;

use crate::error::{err, Stage};

const MAX_CERTS: usize = 8;
const MAX_CHAIN_BYTES: usize = 0x4000;

#[derive(Debug, Clone)]
pub struct CertInfo {
    pub der: Vec<u8>,
    pub subject_der: Vec<u8>,
    pub issuer_der: Vec<u8>,
    pub subject_cn: String,
    pub not_before: i64,
    pub not_after: i64,
    /// Raw SubjectPublicKeyInfo bit-string bytes (SEC1 point for P-256).
    pub public_key: Vec<u8>,
}

/// Split concatenated DER certificates by TLV length and parse each.
pub fn parse_der_chain(data: &[u8]) -> Result<Vec<CertInfo>> {
    if data.is_empty() || data.len() > MAX_CHAIN_BYTES {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "chain empty or too large",
        ));
    }
    let mut certs = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        let len = der_tlv_len(&data[offset..])?;
        if offset + len > data.len() {
            return Err(err(
                Stage::Chain,
                "chain_invalid",
                "certificate length overruns buffer",
            ));
        }
        let slice = &data[offset..offset + len];
        certs.push(parse_one(slice)?);
        offset += len;
        if certs.len() > MAX_CERTS {
            return Err(err(Stage::Chain, "chain_invalid", "too many certificates"));
        }
    }
    if certs.is_empty() {
        return Err(err(Stage::Chain, "chain_invalid", "no certificates parsed"));
    }
    Ok(certs)
}

/// Total length (header + content) of the DER TLV starting at `buf[0]`.
fn der_tlv_len(buf: &[u8]) -> Result<usize> {
    if buf.len() < 2 || buf[0] != 0x30 {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "expected DER SEQUENCE (0x30)",
        ));
    }
    let first = buf[1];
    if first < 0x80 {
        return Ok(2 + first as usize);
    }
    let n = (first & 0x7f) as usize;
    if n == 0 || n > 4 || buf.len() < 2 + n {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "bad DER length encoding",
        ));
    }
    let mut len = 0usize;
    for &b in &buf[2..2 + n] {
        len = (len << 8) | b as usize;
    }
    Ok(2 + n + len)
}

fn parse_one(der_bytes: &[u8]) -> Result<CertInfo> {
    let cert = Certificate::from_der(der_bytes)
        .map_err(|e| err(Stage::Chain, "chain_invalid", format!("parse cert: {e}")))?;
    let tbs = &cert.tbs_certificate;
    let subject_der = tbs.subject.to_der().map_err(|e| {
        err(
            Stage::Chain,
            "chain_invalid",
            format!("encode subject: {e}"),
        )
    })?;
    let issuer_der = tbs
        .issuer
        .to_der()
        .map_err(|e| err(Stage::Chain, "chain_invalid", format!("encode issuer: {e}")))?;
    let not_before = tbs.validity.not_before.to_unix_duration().as_secs() as i64;
    let not_after = tbs.validity.not_after.to_unix_duration().as_secs() as i64;
    let public_key = tbs
        .subject_public_key_info
        .subject_public_key
        .raw_bytes()
        .to_vec();
    Ok(CertInfo {
        der: der_bytes.to_vec(),
        subject_der,
        issuer_der,
        subject_cn: tbs.subject.to_string(),
        not_before,
        not_after,
        public_key,
    })
}

/// Return certificate indices ordered leaf→root, validating the linkage.
pub fn order_leaf_to_root(chain: &[CertInfo]) -> Result<Vec<usize>> {
    if chain.len() == 1 {
        return Ok(vec![0]);
    }
    // Leaf = the cert no other cert is issued by (its subject is nobody's issuer).
    let leaf = (0..chain.len())
        .find(|&i| {
            !chain
                .iter()
                .enumerate()
                .any(|(j, c)| j != i && c.issuer_der == chain[i].subject_der)
        })
        .ok_or_else(|| err(Stage::Chain, "chain_invalid", "no leaf found (cycle?)"))?;

    let mut order = vec![leaf];
    let mut current = leaf;
    loop {
        let cur = &chain[current];
        // self-signed => root reached
        if cur.issuer_der == cur.subject_der {
            break;
        }
        let issuers: Vec<usize> = (0..chain.len())
            .filter(|&j| j != current && chain[j].subject_der == cur.issuer_der)
            .collect();
        if issuers.len() != 1 {
            return Err(err(
                Stage::Chain,
                "chain_invalid",
                format!("expected exactly 1 issuer, found {}", issuers.len()),
            ));
        }
        let next = issuers[0];
        if order.contains(&next) {
            return Err(err(Stage::Chain, "chain_invalid", "issuer cycle detected"));
        }
        order.push(next);
        current = next;
    }
    if order.len() != chain.len() {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "not all certificates lie on the leaf→root path",
        ));
    }
    Ok(order)
}

/// Verify the leaf's public key equals our generated P-256 key (SEC1 uncompressed).
pub fn ensure_leaf_matches(leaf: &CertInfo, key: &PublicKey) -> Result<()> {
    let expected = key.to_encoded_point(false);
    if leaf.public_key != expected.as_bytes() {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "leaf public key does not match the generated key",
        ));
    }
    Ok(())
}

/// Check leaf validity window: not_after in the future, not_before not far ahead.
pub fn ensure_leaf_valid(leaf: &CertInfo, now: i64) -> Result<()> {
    if leaf.not_after <= now {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "leaf certificate already expired",
        ));
    }
    if leaf.not_before > now + 300 {
        return Err(err(
            Stage::Chain,
            "chain_invalid",
            "leaf not_before is in the future",
        ));
    }
    Ok(())
}

/// Parse, order leaf→root, verify leaf key + validity. Returns ordered certs.
pub fn prepare(data: &[u8], key: &PublicKey, now: i64) -> Result<Vec<CertInfo>> {
    let chain = parse_der_chain(data)?;
    let order = order_leaf_to_root(&chain)?;
    let ordered: Vec<CertInfo> = order.iter().map(|&i| chain[i].clone()).collect();
    ensure_leaf_matches(&ordered[0], key)?;
    ensure_leaf_valid(&ordered[0], now)?;
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_chain, TestChain};

    #[test]
    fn parse_and_order() {
        let TestChain { der, public, .. } = test_chain();
        let certs = parse_der_chain(&der).unwrap();
        assert_eq!(certs.len(), 2);
        let order = order_leaf_to_root(&certs).unwrap();
        // leaf first
        ensure_leaf_matches(&certs[order[0]], &public).unwrap();
    }

    #[test]
    fn leaf_key_mismatch() {
        let TestChain { der, .. } = test_chain();
        let certs = parse_der_chain(&der).unwrap();
        let order = order_leaf_to_root(&certs).unwrap();
        let other = p256::SecretKey::random(&mut rand_core::OsRng).public_key();
        assert!(ensure_leaf_matches(&certs[order[0]], &other).is_err());
    }

    #[test]
    fn prepare_full() {
        let TestChain { der, public, .. } = test_chain();
        let ordered = prepare(&der, &public, crate::util::now_unix()).unwrap();
        assert_eq!(ordered.len(), 2);
        ensure_leaf_matches(&ordered[0], &public).unwrap();
    }

    #[test]
    fn expired_leaf_rejected() {
        let TestChain { der, public, .. } = test_chain();
        // 10 years in the future -> leaf already expired relative to then
        let future = crate::util::now_unix() + 10 * 365 * 86_400;
        assert!(prepare(&der, &public, future).is_err());
    }
}
