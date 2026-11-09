//! Google RKP server I/O: `fetchEekChain` (challenge) and `signCertificates`.

use std::io::Read;
use std::time::Duration;

use anyhow::Result;
use base64::Engine;
use ciborium::value::Value;

use crate::config::Config;
use crate::error::{err, Stage};

const CONTENT_CBOR: &str = "application/cbor";
const MAX_CHAIN_LEN: usize = 0x4000;

fn as_array(v: &Value) -> Option<&Vec<Value>> {
    match v {
        Value::Array(a) => Some(a),
        _ => None,
    }
}

fn as_bytes(v: &Value) -> Option<&[u8]> {
    match v {
        Value::Bytes(b) => Some(b),
        _ => None,
    }
}

fn build_agent(cfg: &Config) -> Result<ureq::Agent> {
    let mut builder =
        ureq::AgentBuilder::new().timeout(Duration::from_secs(cfg.request_timeout_seconds));
    if !cfg.proxy.is_empty() {
        let proxy = ureq::Proxy::new(&cfg.proxy)
            .map_err(|e| err(Stage::Config, "config_invalid", format!("proxy: {e}")))?;
        builder = builder.proxy(proxy);
    }
    Ok(builder.build())
}

/// Map a non-2xx HTTP status to a staged error (444 => device_not_registered).
fn map_http_status(code: u16, stage: Stage) -> anyhow::Error {
    if code == 444 {
        return err(
            Stage::Server,
            "device_not_registered",
            "server returned HTTP 444: device is not registered with Google RKP",
        );
    }
    if (400..500).contains(&code) {
        err(stage, "http_client", format!("HTTP {code}"))
    } else {
        err(stage, "http_server", format!("HTTP {code}"))
    }
}

/// POST `body` to `url`, return the response body bytes, mapping errors to `stage`.
fn post_cbor(agent: &ureq::Agent, url: &str, body: &[u8], stage: Stage) -> Result<Vec<u8>> {
    let resp = agent
        .post(url)
        .set("Content-Type", CONTENT_CBOR)
        .set("Accept", CONTENT_CBOR)
        .send_bytes(body);
    match resp {
        Ok(r) => {
            let mut buf = Vec::new();
            r.into_reader()
                .take((MAX_CHAIN_LEN * 4) as u64)
                .read_to_end(&mut buf)
                .map_err(|e| err(stage, "http_client", format!("read body: {e}")))?;
            Ok(buf)
        }
        Err(ureq::Error::Status(code, _)) => Err(map_http_status(code, stage)),
        Err(ureq::Error::Transport(t)) => Err(err(stage, "http_client", format!("transport: {t}"))),
    }
}

/// Fetch an RKP challenge via `:fetchEekChain`.
pub fn fetch_challenge(cfg: &Config, fingerprint: &str) -> Result<Vec<u8>> {
    let agent = build_agent(cfg)?;
    let url = format!("{}:fetchEekChain", cfg.server_url);
    let body = encode_eek_request(fingerprint)?;
    let resp = post_cbor(&agent, &url, &body, Stage::Challenge)?;
    parse_challenge(&resp)
}

fn encode_eek_request(fingerprint: &str) -> Result<Vec<u8>> {
    let req = Value::Map(vec![
        (
            Value::Text("fingerprint".into()),
            Value::Text(fingerprint.into()),
        ),
        (Value::Text("id".into()), Value::Integer(42.into())),
    ]);
    let mut out = Vec::new();
    ciborium::into_writer(&req, &mut out).map_err(|e| {
        err(
            Stage::Challenge,
            "challenge_invalid",
            format!("encode: {e}"),
        )
    })?;
    Ok(out)
}

/// Parse `[ [eek chains...], challenge_bstr ]` and return the challenge.
pub fn parse_challenge(body: &[u8]) -> Result<Vec<u8>> {
    let v: Value = ciborium::from_reader(body).map_err(|e| {
        err(
            Stage::Challenge,
            "challenge_invalid",
            format!("decode: {e}"),
        )
    })?;
    let arr = as_array(&v).ok_or_else(|| {
        err(
            Stage::Challenge,
            "challenge_invalid",
            "response is not an array",
        )
    })?;
    if arr.len() < 2 {
        return Err(err(
            Stage::Challenge,
            "challenge_invalid",
            "response array too short",
        ));
    }
    let challenge = as_bytes(&arr[arr.len() - 1]).ok_or_else(|| {
        err(
            Stage::Challenge,
            "challenge_invalid",
            "challenge is not a byte string",
        )
    })?;
    if challenge.is_empty() || challenge.len() > 64 {
        return Err(err(
            Stage::Challenge,
            "challenge_invalid",
            format!("challenge length {} out of range 1..=64", challenge.len()),
        ));
    }
    Ok(challenge.to_vec())
}

/// Submit the CSR via `:signCertificates`, return the concatenated DER chain (leaf→root).
pub fn sign_certificates(cfg: &Config, csr: &[u8], challenge: &[u8]) -> Result<Vec<u8>> {
    let agent = build_agent(cfg)?;
    let challenge_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge);
    let url = format!(
        "{}:signCertificates?challenge={}",
        cfg.server_url, challenge_b64
    );
    let resp = post_cbor(&agent, &url, csr, Stage::Server)?;
    parse_sign_response(&resp)
}

/// Parse `[[shared, [unique...]]]`, return `unique[0] ++ shared` (leaf→root DER).
pub fn parse_sign_response(body: &[u8]) -> Result<Vec<u8>> {
    let v: Value = ciborium::from_reader(body)
        .map_err(|e| err(Stage::Server, "http_client", format!("decode: {e}")))?;
    let top = as_array(&v)
        .ok_or_else(|| err(Stage::Server, "http_client", "response is not an array"))?;
    if top.is_empty() {
        return Err(err(Stage::Server, "http_client", "empty response array"));
    }
    // top[0] may be the [shared,[unique]] pair, or the response may be flat.
    let pair = match as_array(&top[0]) {
        Some(inner) => inner,
        None => top,
    };
    if pair.len() < 2 {
        return Err(err(Stage::Server, "http_client", "cert pair too short"));
    }
    let shared = as_bytes(&pair[0]).ok_or_else(|| {
        err(
            Stage::Server,
            "http_client",
            "shared cert is not a byte string",
        )
    })?;
    let unique = as_array(&pair[1])
        .ok_or_else(|| err(Stage::Server, "http_client", "unique certs is not an array"))?;
    if unique.len() != 1 {
        return Err(err(
            Stage::Server,
            "http_client",
            format!("expected exactly 1 unique cert, got {}", unique.len()),
        ));
    }
    let leaf = as_bytes(&unique[0]).ok_or_else(|| {
        err(
            Stage::Server,
            "http_client",
            "unique cert is not a byte string",
        )
    })?;

    let mut chain = Vec::with_capacity(leaf.len() + shared.len());
    chain.extend_from_slice(leaf);
    chain.extend_from_slice(shared);
    if chain.is_empty() || chain.len() > MAX_CHAIN_LEN {
        return Err(err(
            Stage::Server,
            "http_client",
            format!("cert chain length {} out of range", chain.len()),
        ));
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(v: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        ciborium::into_writer(v, &mut out).unwrap();
        out
    }

    #[test]
    fn challenge_parse_ok() {
        let body = encode(&Value::Array(vec![
            Value::Array(vec![]), // eek chains (ignored for V2)
            Value::Bytes(vec![1, 2, 3, 4]),
        ]));
        assert_eq!(parse_challenge(&body).unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn challenge_parse_rejects() {
        // too short
        assert!(parse_challenge(&encode(&Value::Array(vec![Value::Bytes(vec![1])]))).is_err());
        // challenge not bytes
        let body = encode(&Value::Array(vec![
            Value::Array(vec![]),
            Value::Integer(5.into()),
        ]));
        assert!(parse_challenge(&body).is_err());
        // oversized challenge
        let body = encode(&Value::Array(vec![
            Value::Array(vec![]),
            Value::Bytes(vec![0u8; 65]),
        ]));
        assert!(parse_challenge(&body).is_err());
    }

    #[test]
    fn sign_response_nested() {
        let body = encode(&Value::Array(vec![Value::Array(vec![
            Value::Bytes(vec![0xAA, 0xBB]),                     // shared
            Value::Array(vec![Value::Bytes(vec![0x11, 0x22])]), // unique[0] = leaf
        ])]));
        // leaf ++ shared
        assert_eq!(
            parse_sign_response(&body).unwrap(),
            vec![0x11, 0x22, 0xAA, 0xBB]
        );
    }

    #[test]
    fn sign_response_rejects_multiple_unique() {
        let body = encode(&Value::Array(vec![Value::Array(vec![
            Value::Bytes(vec![0xAA]),
            Value::Array(vec![Value::Bytes(vec![1]), Value::Bytes(vec![2])]),
        ])]));
        assert!(parse_sign_response(&body).is_err());
    }

    #[test]
    fn http_444_maps_to_device_not_registered() {
        let e = map_http_status(444, Stage::Server);
        let app = crate::error::to_app_error(&e);
        assert_eq!(app.code, "device_not_registered");
        let e = map_http_status(403, Stage::Server);
        assert_eq!(crate::error::to_app_error(&e).code, "http_client");
        let e = map_http_status(503, Stage::Server);
        assert_eq!(crate::error::to_app_error(&e).code, "http_server");
    }
}
