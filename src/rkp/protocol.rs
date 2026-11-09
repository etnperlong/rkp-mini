//! `0x3122` CSR_V2 request encoding, response framing, and CSR validation.

use anyhow::Result;
use ciborium::value::{Integer, Value};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::PublicKey;

use crate::error::{err, Stage};

/// Vendor command header: `0x3122` little-endian = `22 31 00 00`.
pub const CMD_HEADER: [u8; 4] = 0x3122u32.to_le_bytes();
/// Max request size accepted by the TA path.
pub const MAX_REQUEST: usize = 0x9000;
/// Response sentinel the worker preloads into the first 4 bytes.
pub const SENTINEL: i32 = i32::MIN;

fn int(n: i64) -> Value {
    Value::Integer(Integer::from(n))
}

fn encode(v: &Value) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    ciborium::into_writer(v, &mut out)
        .map_err(|e| err(Stage::Csr, "csr_invalid", format!("cbor encode: {e}")))?;
    Ok(out)
}

fn decode(bytes: &[u8]) -> Result<Value> {
    ciborium::from_reader(bytes)
        .map_err(|e| err(Stage::Csr, "csr_invalid", format!("cbor decode: {e}")))
}

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

fn as_int(v: &Value) -> Option<i128> {
    match v {
        Value::Integer(i) => Some((*i).into()),
        _ => None,
    }
}

fn map_get<'a>(map: &'a [(Value, Value)], key: i64) -> Option<&'a Value> {
    map.iter()
        .find(|(k, _)| as_int(k) == Some(key as i128))
        .map(|(_, v)| v)
}

/// Build the COSE_Key (EC2/ES256/P-256) for our generated public key.
pub fn cose_key(public: &PublicKey) -> Value {
    let pt = public.to_encoded_point(false);
    let x = pt.x().expect("uncompressed point has x").to_vec();
    let y = pt.y().expect("uncompressed point has y").to_vec();
    Value::Map(vec![
        (int(1), int(2)),           // kty: EC2
        (int(3), int(-7)),          // alg: ES256
        (int(-1), int(1)),          // crv: P-256
        (int(-2), Value::Bytes(x)), // x
        (int(-3), Value::Bytes(y)), // y
    ])
}

/// Extract (kty, crv, x, y) from a COSE_Key map for comparison.
fn cose_pub(map: &[(Value, Value)]) -> Option<(i128, i128, Vec<u8>, Vec<u8>)> {
    let kty = as_int(map_get(map, 1)?)?;
    let crv = as_int(map_get(map, -1)?)?;
    let x = as_bytes(map_get(map, -2)?)?.to_vec();
    let y = as_bytes(map_get(map, -3)?)?.to_vec();
    Some((kty, crv, x, y))
}

/// Build the full `0x3122` request: header + CBOR `{4: challenge, 57: bstr([key])}`.
pub fn build_request(challenge: &[u8], key: &Value) -> Result<Vec<u8>> {
    if challenge.is_empty() || challenge.len() > 64 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            format!("challenge length {} out of range 1..=64", challenge.len()),
        ));
    }
    let keys_to_sign = encode(&Value::Array(vec![key.clone()]))?;
    let map = Value::Map(vec![
        (int(4), Value::Bytes(challenge.to_vec())),
        (int(57), Value::Bytes(keys_to_sign)),
    ]);
    let body = encode(&map)?;

    let mut req = Vec::with_capacity(4 + body.len());
    req.extend_from_slice(&CMD_HEADER);
    req.extend_from_slice(&body);
    if req.len() > MAX_REQUEST {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            format!("request {} bytes exceeds {MAX_REQUEST}", req.len()),
        ));
    }
    Ok(req)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLayout {
    StatusLen,
    StatusTotal,
    LenOnly,
    StatusOnly,
    Bare,
}

impl FrameLayout {
    pub fn name(self) -> &'static str {
        match self {
            FrameLayout::StatusLen => "status+len",
            FrameLayout::StatusTotal => "status+total",
            FrameLayout::LenOnly => "len",
            FrameLayout::StatusOnly => "status",
            FrameLayout::Bare => "bare",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub layout: FrameLayout,
    pub status: i32,
    /// Raw TA payload (feed to [`unwrap_csr`]).
    pub payload: Vec<u8>,
}

fn i32_le(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Last non-zero byte index + 1 (number of meaningful bytes written).
fn written_len(buf: &[u8]) -> usize {
    buf.iter()
        .rposition(|&b| b != 0)
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// True if `bytes` structurally looks like an AuthenticatedRequest (no binding check).
fn structural_csr_ok(bytes: &[u8]) -> bool {
    let Ok(csr) = unwrap_csr(bytes) else {
        return false;
    };
    let Ok(v) = decode(&csr) else { return false };
    let Some(arr) = as_array(&v) else {
        return false;
    };
    if arr.len() < 4 {
        return false;
    }
    if as_int(&arr[0]) != Some(1) {
        return false;
    }
    match as_array(&arr[2]) {
        Some(dice) if dice.len() >= 2 => {}
        _ => return false,
    }
    matches!(as_array(&arr[3]), Some(sig) if sig.len() == 4)
}

/// Try full slice, then with trailing zero padding stripped.
fn structural_with_padding(bytes: &[u8]) -> Option<Vec<u8>> {
    if structural_csr_ok(bytes) {
        return Some(bytes.to_vec());
    }
    let trimmed = &bytes[..written_len(bytes)];
    if !trimmed.is_empty() && structural_csr_ok(trimmed) {
        return Some(trimmed.to_vec());
    }
    None
}

/// Decode the TA response buffer into a [`Frame`], trying layouts in order.
pub fn decode_frame(buf: &[u8]) -> Result<Frame> {
    // StatusLen: [i32 status][u32 len][payload]
    if buf.len() >= 8 {
        let status = i32_le(&buf[0..4]);
        let len = u32_le(&buf[4..8]) as usize;
        if status == 0 && len > 0 && 8 + len <= buf.len() && structural_csr_ok(&buf[8..8 + len]) {
            return Ok(Frame {
                layout: FrameLayout::StatusLen,
                status,
                payload: buf[8..8 + len].to_vec(),
            });
        }
    }
    // StatusTotal: [i32 status][u32 total(incl 8-byte head)][payload]
    if buf.len() >= 8 {
        let status = i32_le(&buf[0..4]);
        let total = u32_le(&buf[4..8]) as usize;
        if status == 0 && total >= 8 && total <= buf.len() {
            let payload = &buf[8..total];
            if !payload.is_empty() && structural_csr_ok(payload) {
                return Ok(Frame {
                    layout: FrameLayout::StatusTotal,
                    status,
                    payload: payload.to_vec(),
                });
            }
        }
    }
    // LenOnly: [u32 len][payload]
    if buf.len() >= 4 {
        let len = u32_le(&buf[0..4]) as usize;
        if len > 0 && 4 + len <= buf.len() && structural_csr_ok(&buf[4..4 + len]) {
            return Ok(Frame {
                layout: FrameLayout::LenOnly,
                status: 0,
                payload: buf[4..4 + len].to_vec(),
            });
        }
    }
    // StatusOnly: [i32 status][payload] (allow trailing padding)
    if buf.len() >= 4 {
        let status = i32_le(&buf[0..4]);
        if status == 0 {
            if let Some(payload) = structural_with_padding(&buf[4..]) {
                return Ok(Frame {
                    layout: FrameLayout::StatusOnly,
                    status,
                    payload,
                });
            }
        }
    }
    // Bare: pure payload (allow trailing padding)
    if let Some(payload) = structural_with_padding(buf) {
        return Ok(Frame {
            layout: FrameLayout::Bare,
            status: 0,
            payload,
        });
    }

    // Nothing matched: report a KeyMint error code if the head is a small negative.
    if buf.len() >= 4 {
        let status = i32_le(&buf[0..4]);
        if (-1000..=-1).contains(&status) {
            let app = crate::error::AppError::new(Stage::Ta, "ta_rejected", keymint_hint(status))
                .with_ta_status(status)
                .with_head_hex(hex::encode(&buf[..buf.len().min(64)]));
            return Err(anyhow::Error::new(app));
        }
    }
    let app = crate::error::AppError::new(
        Stage::Ta,
        "unrecognized_frame",
        "TA response did not match any known frame layout",
    )
    .with_head_hex(hex::encode(&buf[..buf.len().min(64)]));
    Err(anyhow::Error::new(app))
}

/// Human hint for a known KeyMint status code (spec §10.2).
pub fn keymint_hint(status: i32) -> String {
    let (enum_name, hint) = match status {
        -18 => (
            "INCOMPATIBLE_KEY_FORMAT",
            "firmware wants MacedPublicKey / different key format; Native V2 unavailable",
        ),
        -21 => (
            "INVALID_INPUT_LENGTH",
            "request length/structure wrong; check that key 57 is a bstr",
        ),
        -40 => (
            "INVALID_TAG",
            "vendor command / frame mismatch; check ABI and frame layout",
        ),
        -100 => (
            "UNIMPLEMENTED",
            "this TA does not implement 0x3122 (common on KeyMint 3)",
        ),
        -1000 => ("UNKNOWN_ERROR", "vendor internal error; inspect head_hex"),
        _ => ("", "vendor-specific KeyMint error"),
    };
    if enum_name.is_empty() {
        format!("KeyMint rejected the CSR request with status {status}; {hint}")
    } else {
        format!("KeyMint rejected the CSR request with status {status} ({enum_name}); {hint}")
    }
}

/// Unwrap the TA payload to raw AuthenticatedRequest CBOR bytes.
///
/// Accepts a bare CBOR array, or a `{61: bstr(AuthenticatedRequest)}` wrapper.
pub fn unwrap_csr(payload: &[u8]) -> Result<Vec<u8>> {
    let v = decode(payload)?;
    match v {
        Value::Array(_) => {
            // Re-encode to drop any trailing padding bytes.
            encode(&v)
        }
        Value::Map(m) => {
            let inner = map_get(&m, 61)
                .ok_or_else(|| err(Stage::Csr, "csr_invalid", "wrapper map missing key 61"))?;
            as_bytes(inner)
                .ok_or_else(|| err(Stage::Csr, "csr_invalid", "key 61 is not a byte string"))
                .map(|b| b.to_vec())
        }
        _ => Err(err(
            Stage::Csr,
            "csr_invalid",
            "payload is neither array nor map",
        )),
    }
}

/// Validate CSR structure and binding to our challenge and public key (spec §9.8).
pub fn validate_csr(csr: &[u8], challenge: &[u8], key: &Value) -> Result<()> {
    let v = decode(csr)?;
    let arr = as_array(&v).ok_or_else(|| err(Stage::Csr, "csr_invalid", "CSR is not an array"))?;
    if arr.len() != 4 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            format!("AuthenticatedRequest has {} items, expected 4", arr.len()),
        ));
    }
    if as_int(&arr[0]) != Some(1) {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "AuthenticatedRequest version != 1",
        ));
    }
    let dice = as_array(&arr[2])
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "DICE chain is not an array"))?;
    if dice.len() < 2 {
        return Err(err(Stage::Csr, "csr_invalid", "DICE chain too short"));
    }
    let signed = as_array(&arr[3])
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "SignedData is not an array"))?;
    if signed.len() != 4 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "SignedData is not a 4-item COSE_Sign1",
        ));
    }
    if as_bytes(&signed[0]).map(|b| b.is_empty()).unwrap_or(true) {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "SignedData protected header is empty",
        ));
    }
    if as_bytes(&signed[3]).map(|b| b.is_empty()).unwrap_or(true) {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "SignedData signature is empty",
        ));
    }

    // SignedData payload = bstr( [challenge, csr_payload] )
    let sd_payload = as_bytes(&signed[2]).ok_or_else(|| {
        err(
            Stage::Csr,
            "csr_invalid",
            "SignedData payload is not a bstr",
        )
    })?;
    let sd = decode(sd_payload)?;
    let sd_arr = as_array(&sd).ok_or_else(|| {
        err(
            Stage::Csr,
            "csr_invalid",
            "SignedData payload is not an array",
        )
    })?;
    if sd_arr.len() != 2 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "SignedData payload must be [challenge, csr]",
        ));
    }
    let got_challenge = as_bytes(&sd_arr[0])
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "challenge in CSR is not a bstr"))?;
    if got_challenge != challenge {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "CSR challenge does not match request",
        ));
    }

    // csr_payload = bstr( [3, "keymint", DeviceInfo, keys] )
    let csr_payload = as_bytes(&sd_arr[1])
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "CsrPayload is not a bstr"))?;
    let cp = decode(csr_payload)?;
    let cp_arr = as_array(&cp)
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "CsrPayload is not an array"))?;
    if cp_arr.len() != 4 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "CsrPayload must have 4 items",
        ));
    }
    if as_int(&cp_arr[0]) != Some(3) {
        return Err(err(Stage::Csr, "csr_invalid", "CsrPayload version != 3"));
    }
    let keys = as_array(&cp_arr[3])
        .ok_or_else(|| err(Stage::Csr, "csr_invalid", "keys is not an array"))?;
    if keys.len() != 1 {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            format!("expected exactly 1 key, got {}", keys.len()),
        ));
    }
    let signed_key = match &keys[0] {
        Value::Map(m) => cose_pub(m),
        _ => None,
    }
    .ok_or_else(|| {
        err(
            Stage::Csr,
            "csr_invalid",
            "signed key is not a valid COSE_Key",
        )
    })?;
    let our_key = match key {
        Value::Map(m) => cose_pub(m),
        _ => None,
    }
    .ok_or_else(|| {
        err(
            Stage::Csr,
            "csr_invalid",
            "reference key is not a valid COSE_Key",
        )
    })?;
    if signed_key != our_key {
        return Err(err(
            Stage::Csr,
            "csr_invalid",
            "signed public key does not match generated key",
        ));
    }
    Ok(())
}

/// Append `{ "fingerprint": fp }` as a 5th item (spec §9.9), idempotent.
pub fn append_fingerprint(csr: &[u8], fp: &str) -> Result<Vec<u8>> {
    let v = decode(csr)?;
    let mut arr = match v {
        Value::Array(a) => a,
        _ => return Err(err(Stage::Csr, "csr_invalid", "CSR is not an array")),
    };
    match arr.len() {
        4 => arr.push(Value::Map(vec![(
            Value::Text("fingerprint".into()),
            Value::Text(fp.into()),
        )])),
        5 => {
            let already = matches!(&arr[4], Value::Map(m) if m.iter().any(|(k, _)|
                matches!(k, Value::Text(t) if t == "fingerprint")));
            if !already {
                return Err(err(
                    Stage::Csr,
                    "csr_invalid",
                    "5-item CSR without a fingerprint map; refusing to guess",
                ));
            }
        }
        n => {
            return Err(err(
                Stage::Csr,
                "csr_invalid",
                format!("unexpected CSR length {n}"),
            ))
        }
    }
    encode(&Value::Array(arr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::SecretKey;
    use rand_core::OsRng;

    fn test_key() -> (SecretKey, PublicKey, Value) {
        let sk = SecretKey::random(&mut OsRng);
        let pk = sk.public_key();
        let ck = cose_key(&pk);
        (sk, pk, ck)
    }

    /// Build a minimal AuthenticatedRequest CBOR bound to challenge + key.
    fn build_csr(challenge: &[u8], key: &Value) -> Vec<u8> {
        let csr_payload = encode(&Value::Array(vec![
            int(3),
            Value::Text("keymint".into()),
            Value::Map(vec![]),
            Value::Array(vec![key.clone()]),
        ]))
        .unwrap();
        let sd_payload = encode(&Value::Array(vec![
            Value::Bytes(challenge.to_vec()),
            Value::Bytes(csr_payload),
        ]))
        .unwrap();
        let signed = Value::Array(vec![
            Value::Bytes(vec![0xa0]),     // protected (non-empty)
            Value::Map(vec![]),           // unprotected
            Value::Bytes(sd_payload),     // payload
            Value::Bytes(vec![0x01; 64]), // signature
        ]);
        let auth = Value::Array(vec![
            int(1),
            Value::Map(vec![]),
            Value::Array(vec![Value::Bytes(vec![1]), Value::Bytes(vec![2])]), // DICE >=2
            signed,
        ]);
        encode(&auth).unwrap()
    }

    #[test]
    fn request_header_and_shape() {
        let (_, _, key) = test_key();
        let req = build_request(&[7u8; 16], &key).unwrap();
        assert_eq!(&req[0..4], &[0x22, 0x31, 0x00, 0x00]);
        let map = decode(&req[4..]).unwrap();
        let m = match map {
            Value::Map(m) => m,
            _ => panic!("not a map"),
        };
        assert_eq!(m.len(), 2);
        // key 57 is a bstr whose content is [COSE_Key]
        let inner = as_bytes(map_get(&m, 57).unwrap()).unwrap();
        let keys = decode(inner).unwrap();
        let keys = as_array(&keys).unwrap();
        assert_eq!(keys.len(), 1);
        // coordinates are 32 bytes
        if let Value::Map(ck) = &keys[0] {
            assert_eq!(as_bytes(map_get(ck, -2).unwrap()).unwrap().len(), 32);
            assert_eq!(as_bytes(map_get(ck, -3).unwrap()).unwrap().len(), 32);
        } else {
            panic!("key not a map");
        }
    }

    #[test]
    fn request_rejects_bad_challenge() {
        let (_, _, key) = test_key();
        assert!(build_request(&[], &key).is_err());
        assert!(build_request(&[0u8; 65], &key).is_err());
    }

    #[test]
    fn frame_layouts() {
        let (_, _, key) = test_key();
        let chal = [9u8; 16];
        let csr = build_csr(&chal, &key);

        // StatusLen
        let mut b = Vec::new();
        b.extend_from_slice(&0i32.to_le_bytes());
        b.extend_from_slice(&(csr.len() as u32).to_le_bytes());
        b.extend_from_slice(&csr);
        let f = decode_frame(&b).unwrap();
        assert_eq!(f.layout, FrameLayout::StatusLen);

        // StatusTotal
        let mut b = Vec::new();
        b.extend_from_slice(&0i32.to_le_bytes());
        b.extend_from_slice(&((csr.len() + 8) as u32).to_le_bytes());
        b.extend_from_slice(&csr);
        assert_eq!(decode_frame(&b).unwrap().layout, FrameLayout::StatusTotal);

        // LenOnly
        let mut b = Vec::new();
        b.extend_from_slice(&(csr.len() as u32).to_le_bytes());
        b.extend_from_slice(&csr);
        assert_eq!(decode_frame(&b).unwrap().layout, FrameLayout::LenOnly);

        // StatusOnly with trailing padding
        let mut b = Vec::new();
        b.extend_from_slice(&0i32.to_le_bytes());
        b.extend_from_slice(&csr);
        b.extend_from_slice(&[0u8; 32]);
        assert_eq!(decode_frame(&b).unwrap().layout, FrameLayout::StatusOnly);

        // Bare
        let f = decode_frame(&csr).unwrap();
        assert_eq!(f.layout, FrameLayout::Bare);
    }

    #[test]
    fn frame_negative_status() {
        let mut b = (-100i32).to_le_bytes().to_vec();
        b.extend_from_slice(&[0u8; 16]);
        let e = decode_frame(&b).unwrap_err();
        let app = crate::error::to_app_error(&e);
        assert_eq!(app.code, "ta_rejected");
        assert_eq!(app.ta_status, Some(-100));
    }

    #[test]
    fn frame_unrecognized() {
        let b = vec![0x55u8; 32];
        let e = decode_frame(&b).unwrap_err();
        let app = crate::error::to_app_error(&e);
        assert_eq!(app.code, "unrecognized_frame");
        assert!(app.head_hex.is_some());
    }

    #[test]
    fn unwrap_bare_and_wrapped() {
        let (_, _, key) = test_key();
        let csr = build_csr(&[1u8; 16], &key);
        // bare
        assert_eq!(unwrap_csr(&csr).unwrap(), csr);
        // wrapped {61: bstr}
        let wrapped = encode(&Value::Map(vec![(int(61), Value::Bytes(csr.clone()))])).unwrap();
        assert_eq!(unwrap_csr(&wrapped).unwrap(), csr);
    }

    #[test]
    fn validate_csr_binding() {
        let (_, _, key) = test_key();
        let chal = [3u8; 16];
        let csr = build_csr(&chal, &key);
        assert!(validate_csr(&csr, &chal, &key).is_ok());
        // wrong challenge
        assert!(validate_csr(&csr, &[4u8; 16], &key).is_err());
        // wrong key
        let (_, _, other) = test_key();
        assert!(validate_csr(&csr, &chal, &other).is_err());
    }

    #[test]
    fn append_fingerprint_idempotent() {
        let (_, _, key) = test_key();
        let csr = build_csr(&[5u8; 16], &key);
        let five = append_fingerprint(&csr, "example/reference/test").unwrap();
        let v = decode(&five).unwrap();
        let arr = as_array(&v).unwrap();
        assert_eq!(arr.len(), 5);
        // original 4 items preserved
        let orig = decode(&csr).unwrap();
        let orig_arr = as_array(&orig).unwrap();
        assert_eq!(&arr[0..4], &orig_arr[0..4]);
        // re-applying does not add a 6th item
        let again = append_fingerprint(&five, "example/reference/test").unwrap();
        let v2 = decode(&again).unwrap();
        assert_eq!(as_array(&v2).unwrap().len(), 5);
    }
}
