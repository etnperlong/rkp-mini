//! Sender push (design §10.3, §7.5) and receiver verification (§7.4).
//!
//! Both operate over a [`Transport`], so the full protocol is testable with
//! [`crate::transport::DirTransport`] and no network.

use age::secrecy::SecretString;
use anyhow::Result;

use crate::error::{err, Stage};
use crate::keybox::{self, KeyboxFacts, Validity};
use crate::manifest::{KeyboxSummary, Manifest};
use crate::state::SyncState;
use crate::transport::{Fetch, Transport};
use crate::{envelope, sha256, sha256_hex, DAY, MIN_PLAUSIBLE_NOW};

/// Maximum accepted `issued_at` lead over the local clock (seconds).
pub const ISSUED_AT_SKEW: i64 = 600;

fn check_clock(now: i64) -> Result<()> {
    if now < MIN_PLAUSIBLE_NOW {
        return Err(err(
            Stage::Validate,
            "clock_invalid",
            "local clock is before 2020; refusing time-based checks",
        ));
    }
    Ok(())
}

fn check_state_stream(state: Option<&SyncState>, stream_id: &str) -> Result<()> {
    if let Some(s) = state {
        if s.stream_id != stream_id {
            return Err(err(
                Stage::State,
                "state_stream_mismatch",
                format!(
                    "local state belongs to stream {} but config uses {stream_id}; \
                     run `rkp-sync forget` or re-import to switch streams",
                    s.stream_id
                ),
            ));
        }
    }
    Ok(())
}

fn summary(f: &KeyboxFacts) -> KeyboxSummary {
    KeyboxSummary {
        sha256: f.sha256,
        device_id: f.device_id.clone(),
        leaf_not_before: f.leaf_not_before,
        leaf_not_after: f.leaf_not_after,
        chain_sha256: f.chain_sha256,
    }
}

// ---------------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------------

pub struct PushRequest<'a> {
    pub stream_id: &'a str,
    pub keybox_xml: &'a [u8],
    pub meta_json: Option<&'a [u8]>,
    /// Passphrase that opens the current remote envelope.
    pub remote_passphrase: &'a SecretString,
    /// Passphrase for the new envelope (differs only for password rotation).
    pub passphrase: &'a SecretString,
    /// Sender's record of its last successful push.
    pub state: Option<&'a SyncState>,
    pub now: i64,
    pub min_validity_days: u32,
    /// Skip the remaining-validity requirement (manual emergency use).
    pub allow_expiring: bool,
    /// Publish a new seq even if the remote already carries this keybox.
    pub force: bool,
}

/// Head of a successfully decrypted remote envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHead {
    pub seq: u64,
    pub plaintext_sha256: [u8; 32],
    pub keybox_sha256: [u8; 32],
    pub token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseSource {
    /// No history anywhere: first version, `prev = null`.
    None,
    Remote,
    Local,
}

impl BaseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            BaseSource::None => "none",
            BaseSource::Remote => "remote",
            BaseSource::Local => "local",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PushOutcome {
    /// Remote already carried this keybox; nothing uploaded.
    pub noop: bool,
    pub seq: u64,
    pub plaintext_sha256: [u8; 32],
    /// SHA-256 of the uploaded armored envelope (None for no-op).
    pub ciphertext_sha256: Option<String>,
    pub facts: KeyboxFacts,
    pub remote_seq: Option<u64>,
    pub base: BaseSource,
    pub token: Option<String>,
}

/// Choose the seq/prev base (design §7.5).
pub fn choose_base(
    remote: Option<&RemoteHead>,
    local: Option<&SyncState>,
) -> (u64, Option<[u8; 32]>, BaseSource) {
    let local = local.filter(|s| s.has_verified());
    match (remote, local) {
        (Some(r), Some(l)) if r.seq < l.verified_seq => {
            (l.verified_seq, l.plaintext_digest(), BaseSource::Local)
        }
        (Some(r), _) => (r.seq, Some(r.plaintext_sha256), BaseSource::Remote),
        (None, Some(l)) => (l.verified_seq, l.plaintext_digest(), BaseSource::Local),
        (None, None) => (0, None, BaseSource::None),
    }
}

/// Read and decrypt the current remote head. Missing → `None`.
/// A remote that exists but cannot be decrypted aborts the push.
fn read_remote_head(
    t: &dyn Transport,
    passphrase: &SecretString,
    stream_id: &str,
) -> Result<Option<RemoteHead>> {
    let (bytes, token) = match t.fetch_fresh()? {
        Fetch::Missing => return Ok(None),
        Fetch::NotModified => {
            return Err(err(
                Stage::Remote,
                "remote_unexpected",
                "unconditional read returned 304",
            ))
        }
        Fetch::Body { bytes, token } => (bytes, token),
    };
    let plaintext = envelope::decrypt(&bytes, passphrase).map_err(|e| {
        let se = crate::error::to_sync_error(&e);
        err(
            se.stage,
            &se.code,
            format!(
                "{}; the remote envelope exists but cannot be opened — refusing to overwrite it",
                se.message
            ),
        )
    })?;
    let m = Manifest::decode(&plaintext)?;
    if m.stream_id != stream_id {
        return Err(err(
            Stage::Validate,
            "stream_mismatch",
            format!(
                "remote belongs to stream {} (expected {stream_id}); refusing to overwrite",
                m.stream_id
            ),
        ));
    }
    Ok(Some(RemoteHead {
        seq: m.seq,
        plaintext_sha256: sha256(&plaintext),
        keybox_sha256: m.keybox.sha256,
        token,
    }))
}

/// Validate, encrypt and upload the local keybox; read back to confirm.
pub fn push(t: &dyn Transport, req: &PushRequest) -> Result<PushOutcome> {
    check_clock(req.now)?;
    check_state_stream(req.state, req.stream_id)?;
    let min_remaining = if req.allow_expiring {
        0
    } else {
        i64::from(req.min_validity_days) * DAY
    };
    let facts = keybox::validate(
        req.keybox_xml,
        Validity {
            now: req.now,
            min_remaining_secs: min_remaining,
        },
    )?;

    let remote = read_remote_head(t, req.remote_passphrase, req.stream_id)?;
    let local_seq = req
        .state
        .filter(|s| s.has_verified())
        .map_or(0, |s| s.verified_seq);
    if let Some(r) = &remote {
        if !req.force && r.keybox_sha256 == facts.sha256 && r.seq >= local_seq {
            return Ok(PushOutcome {
                noop: true,
                seq: r.seq,
                plaintext_sha256: r.plaintext_sha256,
                ciphertext_sha256: None,
                facts,
                remote_seq: Some(r.seq),
                base: BaseSource::Remote,
                token: r.token.clone(),
            });
        }
    }

    let (base_seq, prev, base) = choose_base(remote.as_ref(), req.state);
    let seq = base_seq
        .checked_add(1)
        .ok_or_else(|| err(Stage::State, "seq_overflow", "seq exhausted"))?;
    let manifest = Manifest {
        stream_id: req.stream_id.to_string(),
        seq,
        prev,
        issued_at: req.now,
        keybox: summary(&facts),
        keybox_xml: req.keybox_xml.to_vec(),
        meta_json: req.meta_json.map(<[u8]>::to_vec),
    };
    let plaintext = zeroize::Zeroizing::new(manifest.encode()?);
    let plaintext_sha256 = sha256(&plaintext);
    let armored = envelope::encrypt(&plaintext, req.passphrase)?;
    let expected = remote.as_ref().and_then(|r| r.token.as_deref());
    t.put(armored.as_bytes(), expected)?;

    // Read back and confirm the remote now holds exactly what we wrote.
    let (bytes, token) = match t.fetch_fresh()? {
        Fetch::Body { bytes, token } => (bytes, token),
        _ => {
            return Err(err(
                Stage::Remote,
                "readback_failed",
                "remote is missing right after upload",
            ))
        }
    };
    let back = envelope::decrypt(&bytes, req.passphrase).map_err(|_| {
        err(
            Stage::Remote,
            "readback_mismatch",
            "read-back envelope does not decrypt with our passphrase (another writer?)",
        )
    })?;
    if sha256(&back) != plaintext_sha256 {
        return Err(err(
            Stage::Remote,
            "readback_mismatch",
            "read-back envelope differs from the upload (another writer?)",
        ));
    }
    Ok(PushOutcome {
        noop: false,
        seq,
        plaintext_sha256,
        ciphertext_sha256: Some(sha256_hex(armored.as_bytes())),
        facts,
        remote_seq: remote.map(|r| r.seq),
        base,
        token,
    })
}

/// Fold a push outcome into the sender's sync-state.
pub fn record_push(
    state: Option<SyncState>,
    t: &dyn Transport,
    stream_id: &str,
    out: &PushOutcome,
    now: i64,
) -> SyncState {
    let mut s = state.unwrap_or_else(|| SyncState::new(t.provider(), &t.location(), stream_id));
    s.provider = t.provider().to_string();
    s.location = t.location();
    s.verified_seq = out.seq;
    s.plaintext_sha256 = Some(hex::encode(out.plaintext_sha256));
    s.keybox_sha256 = Some(hex::encode(out.facts.sha256));
    s.device_id = Some(out.facts.device_id.clone());
    s.keybox_not_after = Some(out.facts.leaf_not_after);
    s.last_remote_token = out.token.clone();
    s.last_checked_at = Some(now);
    s.verified_at = Some(now);
    s.last_error = None;
    s
}

// ---------------------------------------------------------------------------
// Receiver
// ---------------------------------------------------------------------------

pub struct FetchRequest<'a> {
    pub stream_id: &'a str,
    pub passphrase: &'a SecretString,
    pub state: Option<&'a SyncState>,
    pub now: i64,
    pub min_validity_days: u32,
    /// Send the last remote token as `If-None-Match`.
    pub conditional: bool,
}

/// A decrypted, fully validated envelope.
#[derive(Debug, Clone)]
pub struct Received {
    pub manifest: Manifest,
    pub plaintext_sha256: [u8; 32],
    pub facts: KeyboxFacts,
    /// `verified_seq` before this fetch (None on first import).
    pub previous_seq: Option<u64>,
    /// One or more seqs were skipped (offline sender pushed several times).
    pub gap: bool,
    /// Same seq and plaintext as already verified.
    pub idempotent: bool,
    /// No local state: trust-on-first-use, needs explicit confirmation.
    pub first_import: bool,
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FetchOutcome {
    NotModified,
    Received(Box<Received>),
}

/// Apply the replay/rollback rules to a decrypted plaintext.
pub fn verify_received(
    plaintext: &[u8],
    stream_id: &str,
    state: Option<&SyncState>,
    now: i64,
    min_validity_days: u32,
) -> Result<Received> {
    check_clock(now)?;
    check_state_stream(state, stream_id)?;
    let m = Manifest::decode(plaintext)?;
    if m.stream_id != stream_id {
        return Err(err(
            Stage::Validate,
            "stream_mismatch",
            format!("envelope stream {} does not match {stream_id}", m.stream_id),
        ));
    }
    let digest = sha256(plaintext);
    let verified = state.filter(|s| s.has_verified());
    let mut gap = false;
    let mut idempotent = false;
    if let Some(s) = verified {
        let vs = s.verified_seq;
        if m.seq < vs {
            return Err(err(
                Stage::State,
                "sync_rollback",
                format!("remote seq {} is older than verified seq {vs}", m.seq),
            ));
        }
        if m.seq == vs {
            if s.plaintext_digest() != Some(digest) {
                return Err(err(
                    Stage::State,
                    "seq_conflict",
                    format!("remote seq {vs} differs from the verified version"),
                ));
            }
            idempotent = true;
        } else if m.seq == vs + 1 {
            if m.prev != s.plaintext_digest() {
                return Err(err(
                    Stage::State,
                    "prev_mismatch",
                    format!("seq {} does not chain to verified seq {vs}", m.seq),
                ));
            }
        } else {
            gap = true;
        }
    }
    if m.issued_at > now + ISSUED_AT_SKEW {
        return Err(err(
            Stage::Validate,
            "issued_in_future",
            "envelope issued_at is ahead of the local clock",
        ));
    }
    let facts = keybox::validate(
        &m.keybox_xml,
        Validity {
            now,
            min_remaining_secs: i64::from(min_validity_days) * DAY,
        },
    )?;
    if summary(&facts) != m.keybox {
        return Err(err(
            Stage::Validate,
            "manifest_mismatch",
            "manifest keybox summary does not match keybox_xml",
        ));
    }
    Ok(Received {
        manifest: m,
        plaintext_sha256: digest,
        facts,
        previous_seq: verified.map(|s| s.verified_seq),
        gap,
        idempotent,
        first_import: verified.is_none(),
        token: None,
    })
}

/// Download, decrypt and verify the remote envelope.
pub fn fetch(t: &dyn Transport, req: &FetchRequest) -> Result<FetchOutcome> {
    check_clock(req.now)?;
    let token = req
        .state
        .filter(|s| req.conditional && s.has_verified())
        .and_then(|s| s.last_remote_token.as_deref());
    let (bytes, token) = match t.fetch(token)? {
        Fetch::NotModified => return Ok(FetchOutcome::NotModified),
        Fetch::Missing => {
            return Err(err(
                Stage::Remote,
                "remote_missing",
                "no envelope at the configured location",
            ))
        }
        Fetch::Body { bytes, token } => (bytes, token),
    };
    let plaintext = envelope::decrypt(&bytes, req.passphrase)?;
    let mut r = verify_received(
        &plaintext,
        req.stream_id,
        req.state,
        req.now,
        req.min_validity_days,
    )?;
    r.token = token;
    Ok(FetchOutcome::Received(Box::new(r)))
}

/// Fold a verified envelope into the receiver's sync-state.
pub fn record_received(
    state: Option<SyncState>,
    provider: &str,
    location: &str,
    r: &Received,
    now: i64,
) -> SyncState {
    let mut s = state.unwrap_or_else(|| SyncState::new(provider, location, &r.manifest.stream_id));
    s.provider = provider.to_string();
    s.location = location.to_string();
    s.verified_seq = r.manifest.seq;
    s.plaintext_sha256 = Some(hex::encode(r.plaintext_sha256));
    s.keybox_sha256 = Some(hex::encode(r.facts.sha256));
    s.device_id = Some(r.facts.device_id.clone());
    s.keybox_not_after = Some(r.facts.leaf_not_after);
    s.last_remote_token = r.token.clone();
    s.last_checked_at = Some(now);
    s.verified_at = Some(now);
    s.last_error = None;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::to_sync_error;
    use crate::test_support::TestKeybox;
    use crate::transport::DirTransport;

    const STREAM: &str = "st_01JTESTSTREAM";

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn push_req<'a>(
        xml: &'a [u8],
        p: &'a SecretString,
        state: Option<&'a SyncState>,
    ) -> PushRequest<'a> {
        PushRequest {
            stream_id: STREAM,
            keybox_xml: xml,
            meta_json: None,
            remote_passphrase: p,
            passphrase: p,
            state,
            now: now(),
            min_validity_days: 14,
            allow_expiring: false,
            force: false,
        }
    }

    fn fetch_req<'a>(p: &'a SecretString, state: Option<&'a SyncState>) -> FetchRequest<'a> {
        FetchRequest {
            stream_id: STREAM,
            passphrase: p,
            state,
            now: now(),
            min_validity_days: 7,
            conditional: true,
        }
    }

    fn received(o: FetchOutcome) -> Received {
        match o {
            FetchOutcome::Received(r) => *r,
            FetchOutcome::NotModified => panic!("expected a body"),
        }
    }

    /// Build a plaintext directly (bypassing push) for replay tests.
    fn plaintext(kb: &TestKeybox, seq: u64, prev: Option<[u8; 32]>, issued_at: i64) -> Vec<u8> {
        let facts = keybox::validate_structure(kb.xml.as_bytes()).unwrap();
        Manifest {
            stream_id: STREAM.into(),
            seq,
            prev,
            issued_at,
            keybox: summary(&facts),
            keybox_xml: kb.xml.as_bytes().to_vec(),
            meta_json: None,
        }
        .encode()
        .unwrap()
    }

    fn code(e: anyhow::Error) -> String {
        to_sync_error(&e).code
    }

    #[test]
    fn push_fetch_end_to_end_with_dir_transport() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("remote").join("rkp-sync.age"),
        };
        let p = pw("correct horse battery staple");
        let kb1 = TestKeybox::new(60);

        // First push: seq 1, prev null.
        let out1 = push(&t, &push_req(kb1.xml.as_bytes(), &p, None)).unwrap();
        assert!(!out1.noop);
        assert_eq!(out1.seq, 1);
        assert_eq!(out1.base, BaseSource::None);
        let sender = record_push(None, &t, STREAM, &out1, now());
        // Remote never holds plaintext.
        let remote = std::fs::read_to_string(&t.path).unwrap();
        assert!(!remote.contains("PRIVATE KEY") && !remote.contains(&kb1.device_id));

        // Same keybox again: no-op.
        let again = push(&t, &push_req(kb1.xml.as_bytes(), &p, Some(&sender))).unwrap();
        assert!(again.noop);
        assert_eq!(again.seq, 1);

        // Receiver first import (TOFU).
        let r1 = received(fetch(&t, &fetch_req(&p, None)).unwrap());
        assert!(r1.first_import);
        assert_eq!(r1.manifest.seq, 1);
        assert_eq!(r1.manifest.keybox_xml, kb1.xml.as_bytes());
        let recv = record_received(None, "dir", &t.location(), &r1, now());

        // Unchanged remote → 304 path.
        assert!(matches!(
            fetch(&t, &fetch_req(&p, Some(&recv))).unwrap(),
            FetchOutcome::NotModified
        ));
        // Unconditional → idempotent re-emit.
        let mut uncond = fetch_req(&p, Some(&recv));
        uncond.conditional = false;
        assert!(received(fetch(&t, &uncond).unwrap()).idempotent);

        // New keybox → seq 2 chained to seq 1.
        let kb2 = TestKeybox::new(90);
        let out2 = push(&t, &push_req(kb2.xml.as_bytes(), &p, Some(&sender))).unwrap();
        assert_eq!(out2.seq, 2);
        let r2 = received(fetch(&t, &fetch_req(&p, Some(&recv))).unwrap());
        assert_eq!(r2.manifest.prev, Some(r1.plaintext_sha256));
        assert!(!r2.gap && !r2.first_import);
        assert_eq!(r2.previous_seq, Some(1));
    }

    #[test]
    fn push_refuses_undecryptable_or_foreign_remote() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let kb = TestKeybox::new(60);
        push(&t, &push_req(kb.xml.as_bytes(), &pw("one"), None)).unwrap();
        let before = std::fs::read(&t.path).unwrap();
        let e = push(&t, &push_req(kb.xml.as_bytes(), &pw("two"), None)).unwrap_err();
        assert_eq!(code(e), "decrypt_failed");
        assert_eq!(std::fs::read(&t.path).unwrap(), before, "remote untouched");

        let one = pw("one");
        let mut other = push_req(kb.xml.as_bytes(), &one, None);
        other.stream_id = "st_01JOTHERSTREAM";
        assert_eq!(code(push(&t, &other).unwrap_err()), "stream_mismatch");
    }

    #[test]
    fn push_guards_keybox_validity() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let p = pw("pw");
        let kb = TestKeybox::new(5);
        let e = push(&t, &push_req(kb.xml.as_bytes(), &p, None)).unwrap_err();
        assert_eq!(code(e), "keybox_expiring");
        assert!(!t.path.exists());
        let mut r = push_req(kb.xml.as_bytes(), &p, None);
        r.allow_expiring = true;
        assert_eq!(push(&t, &r).unwrap().seq, 1);
        assert_eq!(
            code(push(&t, &push_req(b"<x/>", &p, None)).unwrap_err()),
            "keybox_invalid"
        );
    }

    #[test]
    fn seq_continues_from_local_after_remote_loss() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let p = pw("pw");
        let kb = TestKeybox::new(60);
        let out = push(&t, &push_req(kb.xml.as_bytes(), &p, None)).unwrap();
        let mut st = record_push(None, &t, STREAM, &out, now());
        st.verified_seq = 7; // pretend we had pushed up to 7 before
        std::fs::remove_file(&t.path).unwrap();
        let out = push(&t, &push_req(kb.xml.as_bytes(), &p, Some(&st))).unwrap();
        assert_eq!(out.seq, 8);
        assert_eq!(out.base, BaseSource::Local);
    }

    #[test]
    fn password_rotation_reencrypts_same_keybox() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let (old, new) = (pw("old"), pw("new"));
        let kb = TestKeybox::new(60);
        let o1 = push(&t, &push_req(kb.xml.as_bytes(), &old, None)).unwrap();
        let st = record_push(None, &t, STREAM, &o1, now());
        let mut rot = push_req(kb.xml.as_bytes(), &new, Some(&st));
        rot.remote_passphrase = &old;
        rot.force = true;
        let o2 = push(&t, &rot).unwrap();
        assert_eq!(o2.seq, 2);
        assert!(fetch(&t, &fetch_req(&old, None)).is_err());
        assert!(fetch(&t, &fetch_req(&new, None)).is_ok());
    }

    #[test]
    fn replay_rules() {
        let kb = TestKeybox::new(60);
        let t0 = now();
        let p1 = plaintext(&kb, 5, None, t0);
        let r = verify_received(&p1, STREAM, None, t0, 7).unwrap();
        let st = record_received(None, "dir", "x", &r, t0);

        // Rollback.
        let old = plaintext(&kb, 4, None, t0);
        assert_eq!(
            code(verify_received(&old, STREAM, Some(&st), t0, 7).unwrap_err()),
            "sync_rollback"
        );
        // Same seq, different bytes.
        let conflict = plaintext(&kb, 5, None, t0 - 1);
        assert_eq!(
            code(verify_received(&conflict, STREAM, Some(&st), t0, 7).unwrap_err()),
            "seq_conflict"
        );
        // Same seq, same bytes → idempotent.
        assert!(
            verify_received(&p1, STREAM, Some(&st), t0, 7)
                .unwrap()
                .idempotent
        );
        // Next seq must chain.
        let bad_prev = plaintext(&kb, 6, Some([0u8; 32]), t0);
        assert_eq!(
            code(verify_received(&bad_prev, STREAM, Some(&st), t0, 7).unwrap_err()),
            "prev_mismatch"
        );
        let good = plaintext(&kb, 6, Some(r.plaintext_sha256), t0);
        assert!(
            !verify_received(&good, STREAM, Some(&st), t0, 7)
                .unwrap()
                .gap
        );
        // Gap allowed but flagged.
        let jump = plaintext(&kb, 9, Some([0u8; 32]), t0);
        assert!(
            verify_received(&jump, STREAM, Some(&st), t0, 7)
                .unwrap()
                .gap
        );
        // Future issued_at.
        let future = plaintext(&kb, 6, Some(r.plaintext_sha256), t0 + 3600);
        assert_eq!(
            code(verify_received(&future, STREAM, Some(&st), t0, 7).unwrap_err()),
            "issued_in_future"
        );
        // Bad clock.
        assert_eq!(
            code(verify_received(&good, STREAM, Some(&st), 1000, 7).unwrap_err()),
            "clock_invalid"
        );
        // Wrong stream.
        assert_eq!(
            code(verify_received(&good, "st_01JOTHERSTREAM", None, t0, 7).unwrap_err()),
            "stream_mismatch"
        );
        // Validity window.
        assert_eq!(
            code(verify_received(&good, STREAM, Some(&st), t0, 90).unwrap_err()),
            "keybox_expiring"
        );
    }

    #[test]
    fn manifest_summary_must_match_xml() {
        let kb = TestKeybox::new(60);
        let other = TestKeybox::new(60);
        let facts = keybox::validate_structure(other.xml.as_bytes()).unwrap();
        let p = Manifest {
            stream_id: STREAM.into(),
            seq: 1,
            prev: None,
            issued_at: now(),
            keybox: summary(&facts),
            keybox_xml: kb.xml.as_bytes().to_vec(),
            meta_json: None,
        }
        .encode()
        .unwrap();
        assert_eq!(
            code(verify_received(&p, STREAM, None, now(), 7).unwrap_err()),
            "manifest_mismatch"
        );
    }

    #[test]
    fn tampered_remote_rejected_on_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let t = DirTransport {
            path: dir.path().join("rkp-sync.age"),
        };
        let p = pw("pw");
        let kb = TestKeybox::new(60);
        push(&t, &push_req(kb.xml.as_bytes(), &p, None)).unwrap();
        let mut text = std::fs::read_to_string(&t.path).unwrap();
        let i = text.len() / 2;
        let c = if text.as_bytes()[i] == b'A' { "B" } else { "A" };
        text.replace_range(i..i + 1, c);
        std::fs::write(&t.path, text).unwrap();
        assert_eq!(
            code(fetch(&t, &fetch_req(&p, None)).unwrap_err()),
            "decrypt_failed"
        );
        assert_eq!(
            code(fetch(&t, &fetch_req(&pw("wrong"), None)).unwrap_err()),
            "decrypt_failed"
        );
        std::fs::remove_file(&t.path).unwrap();
        assert_eq!(
            code(fetch(&t, &fetch_req(&p, None)).unwrap_err()),
            "remote_missing"
        );
    }
}
