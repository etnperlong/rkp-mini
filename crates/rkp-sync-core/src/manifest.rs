//! Canonical CBOR plaintext carried inside the age envelope (design §6.2).
//!
//! Encoding is fully determined: definite lengths, text keys in a fixed order,
//! no tags, no floats. Decoding re-encodes the parsed value and requires the
//! result to be byte-identical to the input, which rejects indefinite lengths,
//! non-minimal integers, reordered/duplicate/unknown keys and trailing bytes.

use anyhow::Result;
use ciborium::value::{Integer, Value};

use crate::error::{err, Stage};
use crate::{limits, valid_stream_id, PROTOCOL_VERSION};

/// Public facts about the keybox, duplicated in the manifest for quick checks.
/// The receiver recomputes every field from `keybox_xml` and requires equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboxSummary {
    pub sha256: [u8; 32],
    pub device_id: String,
    pub leaf_not_before: i64,
    pub leaf_not_after: i64,
    pub chain_sha256: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub stream_id: String,
    pub seq: u64,
    /// SHA-256 of the previous version's raw plaintext CBOR; `None` for the first.
    pub prev: Option<[u8; 32]>,
    pub issued_at: i64,
    pub keybox: KeyboxSummary,
    pub keybox_xml: Vec<u8>,
    /// Diagnostic only; never used for security decisions.
    pub meta_json: Option<Vec<u8>>,
}

const TOP_KEYS: [&str; 8] = [
    "v",
    "stream_id",
    "seq",
    "prev",
    "issued_at",
    "keybox",
    "keybox_xml",
    "meta_json",
];
const KEYBOX_KEYS: [&str; 5] = [
    "sha256",
    "device_id",
    "leaf_not_before",
    "leaf_not_after",
    "chain_sha256",
];

fn invalid(msg: impl Into<String>) -> anyhow::Error {
    err(Stage::Validate, "manifest_invalid", msg)
}

fn text(s: &str) -> Value {
    Value::Text(s.to_string())
}

fn opt_bytes(b: Option<&[u8]>) -> Value {
    b.map_or(Value::Null, |b| Value::Bytes(b.to_vec()))
}

impl Manifest {
    /// Check field-level invariants that do not need the keybox parser.
    pub fn check(&self) -> Result<()> {
        if !valid_stream_id(&self.stream_id) {
            return Err(invalid("stream_id has invalid format"));
        }
        if self.seq == 0 {
            return Err(invalid("seq must be >= 1"));
        }
        if self.keybox_xml.is_empty() || self.keybox_xml.len() > limits::MAX_KEYBOX_XML {
            return Err(invalid("keybox_xml empty or larger than 128 KiB"));
        }
        if let Some(m) = &self.meta_json {
            if m.len() > limits::MAX_META_JSON {
                return Err(invalid("meta_json larger than 16 KiB"));
            }
        }
        if self.keybox.device_id.is_empty() || self.keybox.device_id.len() > 128 {
            return Err(invalid("keybox.device_id empty or too long"));
        }
        Ok(())
    }

    /// Canonical CBOR encoding.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.check()?;
        let kb = &self.keybox;
        let keybox = Value::Map(vec![
            (text("sha256"), Value::Bytes(kb.sha256.to_vec())),
            (text("device_id"), text(&kb.device_id)),
            (
                text("leaf_not_before"),
                Value::Integer(kb.leaf_not_before.into()),
            ),
            (
                text("leaf_not_after"),
                Value::Integer(kb.leaf_not_after.into()),
            ),
            (text("chain_sha256"), Value::Bytes(kb.chain_sha256.to_vec())),
        ]);
        let top = Value::Map(vec![
            (text("v"), Value::Integer(PROTOCOL_VERSION.into())),
            (text("stream_id"), text(&self.stream_id)),
            (text("seq"), Value::Integer(self.seq.into())),
            (text("prev"), opt_bytes(self.prev.as_ref().map(|p| &p[..]))),
            (text("issued_at"), Value::Integer(self.issued_at.into())),
            (text("keybox"), keybox),
            (text("keybox_xml"), Value::Bytes(self.keybox_xml.clone())),
            (text("meta_json"), opt_bytes(self.meta_json.as_deref())),
        ]);
        let mut out = Vec::with_capacity(self.keybox_xml.len() + 512);
        ciborium::ser::into_writer(&top, &mut out).map_err(|e| {
            err(
                Stage::Generic,
                "cbor_encode",
                format!("encode manifest: {e}"),
            )
        })?;
        if out.len() > limits::MAX_PLAINTEXT {
            return Err(invalid("encoded manifest larger than 192 KiB"));
        }
        Ok(out)
    }

    /// Strict decode; see module docs for the canonical-encoding guarantee.
    pub fn decode(bytes: &[u8]) -> Result<Manifest> {
        if bytes.len() > limits::MAX_PLAINTEXT {
            return Err(invalid("plaintext larger than 192 KiB"));
        }
        let value: Value =
            ciborium::de::from_reader(bytes).map_err(|e| invalid(format!("CBOR parse: {e}")))?;
        let top = fields(value, &TOP_KEYS, "manifest")?;
        let mut it = top.into_iter();
        let mut next = || it.next().expect("length checked");

        let v = uint(next(), "v")?;
        if v != PROTOCOL_VERSION {
            return Err(err(
                Stage::Validate,
                "unsupported_version",
                format!("manifest v={v}; this build supports v={PROTOCOL_VERSION}"),
            ));
        }
        let stream_id = text_field(next(), "stream_id")?;
        let seq = uint(next(), "seq")?;
        let prev = match next() {
            Value::Null => None,
            other => Some(digest(other, "prev")?),
        };
        let issued_at = int(next(), "issued_at")?;
        let kb = fields(next(), &KEYBOX_KEYS, "keybox")?;
        let mut kit = kb.into_iter();
        let mut knext = || kit.next().expect("length checked");
        let keybox = KeyboxSummary {
            sha256: digest(knext(), "keybox.sha256")?,
            device_id: text_field(knext(), "keybox.device_id")?,
            leaf_not_before: int(knext(), "keybox.leaf_not_before")?,
            leaf_not_after: int(knext(), "keybox.leaf_not_after")?,
            chain_sha256: digest(knext(), "keybox.chain_sha256")?,
        };
        let keybox_xml = bytes_field(next(), "keybox_xml")?;
        let meta_json = match next() {
            Value::Null => None,
            other => Some(bytes_field(other, "meta_json")?),
        };
        let m = Manifest {
            stream_id,
            seq,
            prev,
            issued_at,
            keybox,
            keybox_xml,
            meta_json,
        };
        m.check()?;
        if m.encode()? != bytes {
            return Err(invalid("non-canonical CBOR encoding"));
        }
        Ok(m)
    }
}

/// Unwrap a map whose text keys must equal `keys` exactly, in order.
fn fields(value: Value, keys: &[&str], what: &str) -> Result<Vec<Value>> {
    let Value::Map(entries) = value else {
        return Err(invalid(format!("{what} is not a map")));
    };
    if entries.len() != keys.len() {
        return Err(invalid(format!(
            "{what} has {} fields, expected {}",
            entries.len(),
            keys.len()
        )));
    }
    let mut out = Vec::with_capacity(keys.len());
    for ((k, v), want) in entries.into_iter().zip(keys) {
        match k {
            Value::Text(t) if t == *want => out.push(v),
            _ => return Err(invalid(format!("{what}: expected key `{want}`"))),
        }
    }
    Ok(out)
}

fn integer(v: Value, name: &str) -> Result<Integer> {
    match v {
        Value::Integer(i) => Ok(i),
        _ => Err(invalid(format!("{name} is not an integer"))),
    }
}

fn uint(v: Value, name: &str) -> Result<u64> {
    u64::try_from(integer(v, name)?).map_err(|_| invalid(format!("{name} out of range")))
}

fn int(v: Value, name: &str) -> Result<i64> {
    i64::try_from(integer(v, name)?).map_err(|_| invalid(format!("{name} out of range")))
}

fn text_field(v: Value, name: &str) -> Result<String> {
    match v {
        Value::Text(t) => Ok(t),
        _ => Err(invalid(format!("{name} is not a text string"))),
    }
}

fn bytes_field(v: Value, name: &str) -> Result<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err(invalid(format!("{name} is not a byte string"))),
    }
}

fn digest(v: Value, name: &str) -> Result<[u8; 32]> {
    bytes_field(v, name)?
        .try_into()
        .map_err(|_| invalid(format!("{name} is not 32 bytes")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_sync_error;

    fn sample() -> Manifest {
        Manifest {
            stream_id: "st_01JTESTSTREAM".into(),
            seq: 42,
            prev: Some([7u8; 32]),
            issued_at: 1_762_750_000,
            keybox: KeyboxSummary {
                sha256: [1u8; 32],
                device_id: "rkp-mini-0123456789ab".into(),
                leaf_not_before: 1_762_700_000,
                leaf_not_after: 1_794_000_000,
                chain_sha256: [2u8; 32],
            },
            keybox_xml: b"<AndroidAttestation/>".to_vec(),
            meta_json: None,
        }
    }

    fn code(r: Result<Manifest>) -> String {
        to_sync_error(&r.unwrap_err()).code
    }

    #[test]
    fn roundtrip_is_byte_stable() {
        let m = sample();
        let a = m.encode().unwrap();
        let back = Manifest::decode(&a).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.encode().unwrap(), a);

        let mut first = sample();
        first.seq = 1;
        first.prev = None;
        first.meta_json = Some(b"{}".to_vec());
        let b = first.encode().unwrap();
        assert_eq!(Manifest::decode(&b).unwrap(), first);
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut b = sample().encode().unwrap();
        b.push(0x00);
        assert_eq!(code(Manifest::decode(&b)), "manifest_invalid");
    }

    #[test]
    fn rejects_reordered_and_extra_keys() {
        // Swap `v` and `stream_id` ordering.
        let m = sample();
        let reordered = Value::Map(vec![
            (text("stream_id"), text(&m.stream_id)),
            (text("v"), Value::Integer(1.into())),
        ]);
        let mut b = Vec::new();
        ciborium::ser::into_writer(&reordered, &mut b).unwrap();
        assert_eq!(code(Manifest::decode(&b)), "manifest_invalid");
    }

    #[test]
    fn rejects_indefinite_length_map() {
        // Re-encode the canonical manifest's top map with indefinite length.
        let canonical = sample().encode().unwrap();
        assert_eq!(canonical[0], 0xA8); // map(8)
        let mut b = vec![0xBF];
        b.extend_from_slice(&canonical[1..]);
        b.push(0xFF);
        assert_eq!(code(Manifest::decode(&b)), "manifest_invalid");
    }

    #[test]
    fn rejects_bad_fields() {
        let mut m = sample();
        m.seq = 0;
        assert!(m.encode().is_err());

        let mut m = sample();
        m.stream_id = "bad".into();
        assert!(m.encode().is_err());

        let mut m = sample();
        m.keybox_xml = vec![b'x'; limits::MAX_KEYBOX_XML + 1];
        assert!(m.encode().is_err());

        let mut m = sample();
        m.meta_json = Some(vec![b'x'; limits::MAX_META_JSON + 1]);
        assert!(m.encode().is_err());
    }

    #[test]
    fn rejects_unknown_version() {
        let mut b = sample().encode().unwrap();
        // map(8), text(1) "v", uint 1 -> patch the value to 2.
        assert_eq!(&b[..4], &[0xA8, 0x61, b'v', 0x01]);
        b[3] = 0x02;
        assert_eq!(code(Manifest::decode(&b)), "unsupported_version");
    }
}
