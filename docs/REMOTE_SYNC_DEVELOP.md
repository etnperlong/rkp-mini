# RKP Mini Remote Sync (rkp-mini-sync) Design

[简体中文](REMOTE_SYNC_DEVELOP_ZH.md) | **English**

> English translation of the Chinese original ([REMOTE_SYNC_DEVELOP_ZH.md](REMOTE_SYNC_DEVELOP_ZH.md)), which is authoritative where the two differ.

> Status: CLI/protocol implemented (2026-11-10); the **§19 WebUI revision** is a decision added the same day; implement per §19.
> Reader: a new agent session taking over the implementation.
> Read first: repository root `AGENTS.md` (Rust-side constraints and verification commands), then this document.
> Scope: adds "encrypted sync + receiver-side deployment"; does not change the existing KeyMint `0x3122` issuance protocol, but **expands the repository scope** (see §13).
> The decisions here were confirmed through product discussion; do not re-argue them during implementation unless a factual error is found.

---

## 0. One sentence

Encrypt the `keybox.xml` issued by `rkp-mini-issuer` into a single ciphertext file with **age v1 passphrase encryption**, and upload it to the user's own **GitHub Gist / WebDAV / public HTTPS**; the receiver imports the location through a **URL Scheme that carries no passphrase**, manually enters the passphrase to decrypt, verify, and write the local `sync-state`, and then an **independent deploy layer** decides whether to write OhMyKeymint, TEESimulator, or another target.

- No server.
- No account registration, device registration, or remote revocation list.
- The cloud only ever sees ciphertext.
- URLs/QR codes never contain the age passphrase, the Gist token, or the WebDAV password.
- The sync layer and the deploy layer are decoupled; `sync-state` never contains target/path.
- A separate `rkp-mini-sync` binary; network/passphrase/sync dependencies are not stuffed into `rkp-mini-issuer`.

---

## 1. Settled decisions (do not re-argue)

| Decision | Conclusion | Rationale |
|---|---|---|
| Server | **Not built at all** | Use existing Gist/WebDAV/public HTTP; avoids deployment, accounts, and revocation services |
| Encryption | age v1 passphrase + ASCII armor | Mature, simple passphrase mode, Gist needs text |
| manifest | Inside the age ciphertext (CBOR) | No extra signing public key; the passphrase provides confidentiality and integrity at once |
| Remote file | A single `rkp-sync.age` | Atomic replacement is simple; no multi-file transaction |
| Gist receiver | **Direct link, no GitHub token needed** | The raw URL of a secret gist can be read directly; the passphrase protects the content |
| Gist sender | Needs a `gist` scope token | Creating/updating a gist requires authentication |
| WebDAV | Custom path/filename; password entered separately | Compatible with Jianguoyun etc.; the URL carries no password |
| Public HTTP | Supports `provider=url` | Some WebDAV setups have public share links; the passphrase is still required |
| URL Scheme | Only location and stream metadata | Passphrase/token never enter the URL/QR code |
| State | Split into `sync-state` and `deploy-state` | The sync layer does not know the deploy target, which keeps them decoupled |
| `sync-state` | No `target` / `target_path` / owner / reload | The deploy method is decided by the deploy layer after receiving |
| Binary | Separate `rkp-mini-sync` | The receiver does not need KeyMint/TA; smaller attack surface and fewer dependencies |
| Automatic runs | `secret-file` by default | Both sender and receiver can run unattended for long periods |
| Receiver device | A **new device**, cross-device use | Must accept the risk on first enable in the UI |
| Default switch | `sync.enabled = false` | Remote private-key delivery is a high-risk capability |
| Identity rotation | `scheduled` by default | Avoids replacing the keybox on every push; rotates automatically only near expiry |
| Management UI | **WebUI is the primary entry point** (the sender is folded into the `rkp_mini_issuer` WebUI; the receiver module ships its own WebUI), the CLI keeps equivalent capability | See §19 |
| Secret write | The WebUI writes the **hex-encoded** secret into a 0600 inbox file through `ksu.exec` using shell builtins; `rkp-mini-sync` reads, validates, persists, and deletes the inbox file | `ksu.exec` goes through the root shell's stdin, not argv; see §19.2 |
| Non-interactive confirmation | `enable --accept-risk <version>`; `fetch --accept-first <sha>`; `rkp-mini-deployer apply --confirm-first/--allow-identity-change <sha>` | Confirmation is bound to the exact version the user saw, which prevents concurrent replacement |
| Automatic issuance | `auto_issue_days` means "remaining-days threshold" | No unconditional periodic issuance |
| Tool configuration | **Not synced** | OMK `[crypto]` and TEESim `config.json` are device/identity specific |
| Extra signatures | No Ed25519 by default | The passphrase is the only trust anchor; less public-key distribution |
| Revocation | Change the passphrase + re-issue the keybox | Without a server, an old keybox cannot truly be revoked remotely |

---

## 2. Goals and non-goals

### 2.1 Goals

1. The sender can encrypt the current `keybox.xml` and upload it to Gist / WebDAV / public HTTPS.
2. The receiver can import the remote location through a URL Scheme, enter the age passphrase, and download, decrypt, and verify.
3. The receiver can independently decide which tool, which path, and which permissions the verified artifact is deployed to.
4. Long-term unattended operation: the sender periodically checks/issues/pushes, the receiver periodically pulls/deploys.
5. Identity rotation policy, so "automatic update" does not accidentally delete OMK's key.
6. All credentials (age passphrase, Gist token, WebDAV password, keybox private key) never appear in URLs, QR codes, logs, or WebUI text.
7. When remote private-key delivery is first enabled, the user must explicitly accept the risk; afterwards it can be disabled in settings.

### 2.2 Non-goals

- No Worker/R2/D1, no account system, no device registration, no remote revocation list.
- No multi-writer concurrency control; one stream allows exactly one sender.
- No keybox content editing, certificate chain modification, or key reuse.
- No syncing of OMK `config.toml` / `injector.toml` or TEESim `config.json`.
- No delivery of arbitrary remote commands/paths.
- No promise that Play Integrity passes; cross-device use is not promised to be legitimate RKP provisioning.
- No Ed25519 manifest signing (kept as a future hardening option).
- No automatic rollback to an older seq; rollback requires a human to publish a higher seq again.
- No server-style "device revocation"; revocation = change the passphrase + re-issue.

---

## 3. Terminology

| Term | Meaning |
|---|---|
| sender | The issuing device running `rkp-mini-issuer` |
| receiver | The receiver module/program on the new device |
| stream | One sync link; one `stream_id` + one age passphrase |
| provider | `gist` / `webdav` / `url` |
| envelope | `rkp-sync.age`, the single file after age encryption |
| plaintext | The decrypted CBOR manifest + keybox |
| verified artifact | The keybox file after `fetch` decrypted and verified it |
| deploy target | OMK / TEESim / TrickyStore / custom |
| identity change | The `keybox_sha256` of the new keybox differs from the deployed one |
| rotation | The keybox identity changes because RKP re-issued it |
| `auto_issue_days` | Issuance is triggered when the leaf's remaining days fall below this value (a threshold, not a fixed period) |

---

## 4. Known facts (do not re-research)

The following facts were confirmed on 2026-11-10 by reading this repository's source, the OhMyKeymint source, and the TEESimulator source.

### 4.1 RKP-Mini

- Module id: `rkp_mini_issuer`; binary: `/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer`.
- Data root: `/data/misc/keystore/rkp-mini`; default output: `output/`.
- Output files: `keybox.xml`, `cert-chain.pem`, `cert-chain.der`, `private-key.pem`, `meta.json`; credential/data files are `0600`, the output directory is `0700`.
- `keybox.xml` is **EC-only**: one `<Key>` with `algorithm="ecdsa"`, a SEC1 `EC PRIVATE KEY`, and the certificate chain leaf→root.
- `DeviceID` format: `rkp-mini-` + `sha256(public key SEC1)[:12]` (`src/rkp/keybox.rs`).
- Each `issue` generates a new P-256 key; on failure the old `keybox.xml` is kept.
- `meta.json` contains public diagnostic fields such as `issued_at`, `leaf_not_after`, `device_id`, `chain_sha256`; it contains no private key.
- `auto_issue_days` is currently a reserved field (default 0 = off); scheduling is not implemented yet.
- The current scope explicitly excludes "remote private-key deployment"; this design requires changing that scope (§13).

### 4.2 OhMyKeymint (OMK)

The following §4.2–§4.3 were re-checked against upstream source on 2026-11-10 (OMK `9069c6c`/1.2.6, TEESimulator `dev@06f9f2e`/canary-76, TEESimulator-RS `6d241e5`/v6.0.1, TrickyStore `master@3a515c5` and the 1.4.1 release package); all are conclusions from reading source, not verified on a real device.

- Module id: `oh_my_keymint` (`/data/adb/modules/oh_my_keymint`).
- Active keybox: `/data/misc/keystore/omk/keybox.xml`, a compile-time constant, not configurable.
- Permissions: `0600`, owner `1017:1017` (keystore uid); set by `post-fs-data.sh` and when keymint starts. The process drops privileges to 1017 after start, so a replacement file that is `root:root 0600` fails to read (the old keybox is kept).
- Hot reload: inotify watches the parent directory with mask `IN_CLOSE_WRITE | IN_CREATE | IN_MOVED_TO`, and only processes events whose name equals `keybox.xml` → a temp file in the same directory plus `rename(2)` reloads immediately, while the temp file's own events are ignored. **Do not write in place** to `keybox.xml`: on `IN_CREATE` the file is empty, and after parsing fails OMK overwrites it with the built-in template.
- Restart switch: `/data/adb/omk/restart.keymint` (or `restart.all`) is polled for existence by the wrapper every 2 seconds; once consumed it is deleted and the keymint process is restarted. Replacing the keybox does not need it.
- Parsing is a regex scan, not XML DOM: it requires the literals `<Key algorithm="ecdsa">` (or `ec`), `<PrivateKey>`, and `<NumberOfCertificates>` with a count matching `<Certificate>`; EC is required, RSA optional. The SEC1 private key must carry `parameters` (namedCurve secp256r1); `common/src/crypto/ec.rs::import_sec1_private_key` errors out when that field is missing. A parse failure overwrites the file with the built-in template. `rkp-mini-issuer` now encodes the private key as required (SEC1 + named curve OID); this repository's DOM validation treats `NumberOfCertificates` as optional and does not require SEC1 `parameters`, so a hand-built keybox or one issued by an older version can pass validation and still be rejected by OMK. The deploy layer therefore runs an OMK compatibility pre-check before writing (§11.4), and skips the `omk` target with `omk_incompatible` when it is incompatible; keyboxes issued before this change lack `parameters` and must be re-issued.
- `[crypto]` in `config.toml` is the device's unique secret; `injector.toml` is device/app scope configuration. **Neither is synced**.
- `retire_stale_keybox_bound_keys` defaults to `true`: replacing the keybox deletes client keys bound to the old keybox. Automatic rotation must warn.

### 4.3 TEESimulator / TEESimulator-RS / TrickyStore

These are three different projects; keep them apart:

| Project | Module id | keybox | watcher (directory-level inotify) |
|---|---|---|---|
| TEESimulator (JingMatrix, `dev`) | `teesim` | `/data/adb/teesim/<profile.keybox>`, default `keybox.xml` | `CLOSE_WRITE \| MOVED_TO \| DELETE`; only handles `config.json`, `overrides.json`, and `*.xml` |
| TEESimulator-RS (Enginex0) | `tricky_store` | `/data/adb/tricky_store/keybox.xml` (the `[name.xml]` section of `target.txt` can specify other files per package) | `CLOSE_WRITE \| MOVED_TO \| DELETE`; `*.xml` invalidates the cache, and the file is re-read on next use |
| TrickyStore (5ec1cff) | `tricky_store` | `/data/adb/tricky_store/keybox.xml`, not configurable | `CLOSE_WRITE \| DELETE \| MOVED_FROM \| MOVED_TO`; only handles the name `keybox.xml` |

Common points: the reading process is root, so a `0:0 / 0600` file is usable; a temp file in the same directory plus `rename(2)` produces `IN_MOVED_TO` and triggers a reload without a restart. The temp file name **must not end in `.xml`** (otherwise TEESimulator triggers a reload on a half-written file); `.<name>.tmp.<pid>` satisfies this.

TEESimulator (`teesim`):

- The data directory `/data/adb/teesim` is `0700`; `/data/adb/tricky_store` is used only at install time for a one-off copy of the old keybox, and is not read at runtime.
- `profiles.<id>.keybox` in `config.json` is a relative filename, e.g. `"keybox": "keybox.xml"`; the file must sit directly under `/data/adb/teesim` and its name must end in `.xml` for the watcher to reload it.
- After a reload it re-attests existing keys (`ReAttest`); the key blobs do not change.
- `rust/teesim-km/src/attest.rs` explicitly supports EC-only; each `<Key>` chain has at least 2 certificates. The README says "RSA + ECDSA", which disagrees with the source; trust the target version's actual behavior.
- If a profile's keybox fails to parse, the committed profile table may end up empty and all targets temporarily lose the TA.
- The WebUI writes `0644` when importing a keybox, but the directory is `0700`; a receiver writing `0600` that root reads also works.
- `config.json` contains profiles, app scope, device identity, etc. **Not synced**.

TEESimulator-RS and TrickyStore (`tricky_store`, same module id and path, mutually exclusive installation, sharing the `tricky_store` target):

- Both accept EC-only; TrickyStore 1.1.0 requires `NumberOfCertificates` to match the actual count, while 1.4.1 does not read that field.
- They do not delete client keys. TEESimulator-RS only clears the patched chain cache: keys already generated/cached before the replacement keep returning the old certificate chain until the daemon restarts; the open-source TrickyStore likewise does not clear the cache of already generated keys.
- A parse failure does not fall back to the old keybox (the symptom is that no attestation key is available).

Private keys missing SEC1 `parameters` (namedCurve), i.e. the output of `rkp-mini-issuer` before this change, are handled as follows by each target (still source-reading conclusions, not verified on a real device):

- TrickyStore (open source `3a515c5` + BouncyCastle 1.78.1, and the 1.4.1 dex) **rejects**: `PEMParser` yields a `PEMKeyPair` whose AlgorithmIdentifier parameters are null, so `JcaPEMKeyConverter.getKeyPair` fails immediately; the exception happens after `keyboxes.clear()`, so the whole keybox is discarded and no attestation patching happens at all.
- TEESimulator-RS (`6d241e5` + BouncyCastle 1.83) **rejects**: the same `JcaPEMKeyConverter` fails ("EC domain parameters must be encoded in the algorithm identifier"); only that `<Key>` is dropped, so an EC-only keybox has no usable attestation key (-75).
- TEESimulator (JingMatrix `06f9f2e`) **accepts**: the private key is just opaque DER, and the built-in BoringSSL patch (`rust/patches/kmr-crypto-boring-ec-group.patch`) parses SEC1 with the group supplied by the caller, so `parameters` is optional.

So an old keybox only works on TEESimulator; working on OhMyKeymint / TrickyStore / TEESimulator-RS requires re-issuing. Only the `omk` target has a pre-write check (`omk_incompatible`); `tricky_store` has none.

### 4.4 Gist / WebDAV

- A GitHub Gist "secret" is not private: anyone with the URL can read it; the ciphertext still depends on the age passphrase.
- A secret gist's raw URL can be read without a token; the anonymous API is rate limited (about 60 requests/hour).
- Gist updates use `PATCH /gists/{id}`; the revision is committed atomically and history retains old ciphertext.
- Gist direct links should use the "latest version" URL, not a URL with a revision.
- WebDAV (e.g. Jianguoyun) uses an app password; ETag/If-Match/MOVE support varies by provider.

---

## 5. Architecture and trust boundaries

### 5.1 Layers

```mermaid
flowchart LR
  A[rkp-mini<br/>issue keybox] --> B[rkp-mini-sync push<br/>CBOR + age + upload]
  B --> C[(Gist / WebDAV / public HTTPS)]
  C --> D[rkp-mini-sync fetch<br/>download + decrypt + verify]
  D --> E[verified artifact<br/>staging 0600]
  E --> F[deploy layer<br/>receiver-side local decision]
  F --> G[OhMyKeymint]
  F --> H[TEESimulator]
  F --> I[TrickyStore / custom]
```

| Layer | Responsibility | Explicitly does not know |
|---|---|---|
| issuer | Produces the keybox | Receiver deploy target |
| sync | Remote addressing, encryption/decryption, seq/replay protection, download, verification, staging | target/path/owner/reload |
| deploy | Target selection, compatibility checks, permissions, atomic writes, reload, deploy state | provider, passphrase, URL, seq |
| UI | Shows state, risk acceptance, accepts passphrase/location | Never shows keybox/private key/passphrase |

### 5.2 Trust boundaries

| Object | Trust level | Notes |
|---|---|---|
| age passphrase | **Fully trusted** | The holder can decrypt and can forge new ciphertext; must be strong and unique per stream |
| Gist / WebDAV / HTTP | Untrusted (confidentiality/integrity), only availability is trusted | Can be read, modified, replayed, deleted |
| URL/QR code | Untrusted | Only a location hint; wrong content fails decryption or is rejected by state |
| `sync-state` / `deploy-state` | Locally trusted | Replay protection/deploy records; loss enters TOFU |
| Local adapter configuration | Locally trusted | Sole authority over which file, permissions, reload |
| keybox | Highly sensitive credential | Private key; never goes into URLs/logs/WebUI/cloud plaintext |

### 5.3 Threat model

| Threat | Consequence | Control |
|---|---|---|
| Cloud reads the ciphertext | Private key leak | age passphrase encryption; no plaintext in the cloud |
| Cloud tampers with the ciphertext | Malicious keybox injected | age AEAD authentication; decryption failure is a rejection |
| Cloud replays old ciphertext | Downgrade/expiry | `seq` + `prev` + local `sync-state` + leaf expiry |
| Cloud deletes/withholds | Updates stop | Keep the current keybox; state warning; retry next round |
| Gist token leak | Gist written badly | Minimal token scope; single sender; read-back verification; revoke the token |
| WebDAV password leak | Files written badly | The app password can be revoked; temp file + MOVE; If-Match |
| URL phishing | Points at attacker storage | Useless without the passphrase; the receiver shows the location and requires confirmation |
| Passphrase leak | Complete compromise | Strong passphrase, `secret-file` at `0600`, rotate the passphrase + re-issue |
| Cross-device keybox | Impersonation/integrity failure | Risk acceptance on first enable; the docs state plainly that this is not legitimate RKP provisioning |
| Local root compromised | Passphrase/keybox leak | Cannot be protected against a root threat; rotate + re-issue |
| State lost | TOFU replay window | Manually check seq/time/device_id/expiry on first import |
| OMK automatic rotation | Old key deleted | identity gate + `retire_stale_keybox_bound_keys` warning |
| SSRF / XXE | Local service or parser attacked | Restrict provider/URL, forbid DTD/entities, size limits |
| Custom scheme hijack | Phishing receiver | Prefer pasting/scanning inside the receiver; the URL carries no passphrase |

---

## 6. Ciphertext format

### 6.1 Remote file

```text
rkp-sync.age
```

- Content: age v1 **ASCII armor** text.
- Encryption mode: age passphrase (scrypt + ChaCha20-Poly1305).
- Fixed filename; on Gist, one gist per stream; WebDAV allows a custom filename, but a stable one is recommended.
- No compression (the keybox is tiny; avoids extra parsing surface).

### 6.2 Plaintext CBOR

```text
{
  v: 1,                         // uint
  stream_id: "st_01J...",       // text
  seq: 42,                      // uint64, >= 1
  prev: h'...' | null,          // SHA-256 of the previous plaintext; null for the first version
  issued_at: 1762750000,        // int64 Unix seconds
  keybox: {
    sha256: h'...',             // SHA-256 of keybox_xml
    device_id: "rkp-mini-...",  // text
    leaf_not_before: 1762700000,
    leaf_not_after: 1794000000,
    chain_sha256: h'...'
  },
  keybox_xml: h'...',           // raw XML bytes
  meta_json: h'...' | null      // optional, diagnostic; not used in security decisions
}
```

Encoding rules (so independent implementations are reproducible):

- Use CBOR definite-length; indefinite-length is forbidden.
- Map keys are text; the field order is fixed as listed above.
- No tags, no floating point numbers.
- Strings are UTF-8; byte strings use bstr.
- `seq` starts at 1; the sender must not go backwards.
- `prev` = SHA-256 of the previous version's **raw plaintext CBOR bytes**; null for the first version.
- `plaintext_sha256` is not in the CBOR; the receiver computes it over the raw decrypted bytes.
- `keybox.sha256` = SHA-256 of the raw `keybox_xml` bytes.
- `meta_json` is diagnostic only; the receiver must not base security decisions on it.

### 6.3 Size limits

| Object | Limit |
|---|---|
| Whole age file | 256 KiB |
| Decrypted CBOR | 192 KiB |
| `keybox_xml` | 128 KiB |
| `meta_json` | 16 KiB |
| URL-decoded JSON | 4 KiB |
| Single Gist/WebDAV response | 256 KiB |

Anything over the limit is rejected outright; do not buffer without bound while reading.

### 6.4 Passphrase rules

- UTF-8; strip only a single trailing `\n` or `\r\n` from the input line; **do not trim leading/trailing spaces**.
- At least 6 random words or 20+ random characters is recommended; unique per stream.
- No "passphrase check field"; decryption failure reports `decrypt_failed` uniformly.
- The passphrase must not appear in logs, error messages, URLs, QR codes, external process argv, or environment variables; for the WebUI write path see §19.2 (through shell stdin + builtin commands, producing no argv that contains the passphrase).
- `secret-file` holds the raw passphrase; when reading, strip only the trailing newline; the file is `0600`.
- Use age's default scrypt parameters; if low-end devices are noticeably slow, record the timing, but do not lower to unacceptable security parameters.

### 6.5 Why not Ed25519

- The manifest is inside the ciphertext, already authenticated by age AEAD;
- An extra signature requires distributing/pinning the signing public key, which conflicts with "no server, lightweight";
- If the passphrase leaks, the signature does not save you;
- Kept as a future hardening option, not part of v1.

---

## 7. State

### 7.1 `sync-state.json`

Describes only "which version the sync layer has verified"; **contains no target/path**.

```json
{
  "v": 1,
  "provider": "gist",
  "location": "https://gist.githubusercontent.com/<owner>/<id>/raw/rkp-sync.age",
  "stream_id": "st_01J...",
  "verified_seq": 42,
  "plaintext_sha256": "...",
  "keybox_sha256": "...",
  "device_id": "rkp-mini-...",
  "keybox_not_after": 1794000000,
  "last_remote_token": "etag-or-updated_at",
  "last_checked_at": 1762750000,
  "verified_at": 1762750000,
  "last_error": null
}
```

- `verified_seq` is the highest seq that has been decrypted and passed verification, not the "deployed" seq.
- The sender reuses the same structure to record "the last successful push": `verified_seq` = the uploaded seq, `plaintext_sha256` = that version's plaintext hash; used to continue the seq after the remote is deleted/recreated.
- The deploy layer records the actual deployment via `deploy-state`.
- `location` may be updated when a Gist is recreated or the URL changes, but `stream_id` does not change; the same stream must be verified successfully before updating.

### 7.2 `deploy-state.json`

Maintained by the receiver deploy layer:

```json
{
  "v": 1,
  "stream_id": "st_01J...",
  "last_seen_keybox_sha256": "...",
  "last_seen_seq": 42,
  "deployed_keybox_sha256": "...",
  "deployed_seq": 41,
  "deployed_at": 1762750000,
  "targets": [
    {
      "id": "omk",
      "path": "/data/misc/keystore/omk/keybox.xml",
      "owner": "1017:1017",
      "mode": "0600",
      "deployed_sha256": "...",
      "deployed_at": 1762750000,
      "reload": "auto",
      "reload_status": "ok",
      "last_error": null
    }
  ],
  "pending": null
}
```

- `pending` can record `{seq, keybox_sha256}` for "fetched but not deployed because of the identity gate".
- One keybox may map to several targets; each target records success/failure independently.
- Deploy information is not written back into `sync-state`.

### 7.3 verified artifact

- After `rkp-mini-sync fetch` decrypts and verifies, it writes:
  - `<receiver data dir>/staging/<keybox_sha256>.xml`, `0600`.
  - `staging/latest.json`, `0600`, recording `{seq, keybox_sha256, plaintext_sha256, device_id, leaf_not_after}`.
- `fetch --json` outputs the staging path and metadata.
- `rkp-mini-deployer apply` reads staging and deploys per the local target configuration; staging is cleaned up afterwards (or the most recent one is kept for diagnostics).
- staging is a plaintext private-key file; directory `0700`, files `0600`; it must not enter backups/logs/WebUI.

### 7.4 Replay protection rules

Receiver processing order:

1. Decrypt → parse CBOR → validate schema/size/stream_id.
2. `seq > sync-state.verified_seq` → continue.
3. `seq == verified_seq`:
   - same `plaintext_sha256` → idempotent, re-emitting the artifact is allowed (for deploy retries);
   - different → `seq_conflict`, reject.
4. `seq < verified_seq` → `sync_rollback`, reject.
5. When `seq == verified_seq + 1`, require `prev == sync-state.plaintext_sha256`.
6. Gaps (several versions missed while offline) are allowed, but record `gap=true` and warn; do not require downloading all intermediate versions.
7. `issued_at <= now + 600`; a bad local clock (< 2020) → `clock_invalid`.
8. Leaf `not_after >= now + min_validity_days`; otherwise `keybox_expiring`.
9. First import (no state): TOFU, requires manual confirmation, showing seq/time/device_id/expiry/target impact.
10. State lost: also TOFU; this is the residual replay window of a server-less design.

### 7.5 seq after remote deletion/recreation

- The sender keeps `verified_seq` and `plaintext_sha256` locally (the values of the last successful push).
- Base selection: `base = (remote.seq >= local.verified_seq) ? remote : local`; use local when the remote does not exist.
- New seq = `base.seq + 1`, not always 1.
- `prev` = `base.plaintext_sha256`; null only when there is no history at all.
- Remote exists but fails to decrypt: stop, do not overwrite; do not assume "the remote is broken, start over".
- After switching Gist/WebDAV files, the receiver updates `location` but keeps `verified_seq`; a lower seq at the new location is rejected.
- If the sender state is lost and the remote is lost too, the sequence may restart at 1; the receiver rejects the low seq, and the user must confirm a new stream or import a new stream_id.

---

## 8. Remote providers

### 8.1 GitHub Gist

**Best practice:**

| Item | Conclusion |
|---|---|
| Count | One gist per stream; do not put several streams in one gist |
| Gist ID | Use the GitHub-generated ID; do not try to customize it |
| Filename | Fixed `rkp-sync.age`; keep the same name on updates |
| Type | `public: false` (secret); secret ≠ private |
| Description | Generic description, no device/model/stream information |
| Sender | PAT with only the `gist` scope, stored `0600` |
| Receiver | GET the ciphertext URL directly, no token needed |
| Direct link | Latest-version URL, no revision |
| Update | `PATCH /gists/{id}` replaces the file |
| History | Revisions keep old ciphertext, still decryptable with the old passphrase |
| Rate | Anonymous API about 60/hour; the receiver's polling interval is well above that |

Direct link construction:

```text
https://gist.githubusercontent.com/<owner>/<gist_id>/raw/rkp-sync.age
```

Do not use:

```text
https://gist.githubusercontent.com/<owner>/<gist_id>/raw/<revision>/rkp-sync.age
```

A URL with a revision points at the old version forever.

Receiver-side reading without a token:

- GET the URL above directly; secret gists are anonymously accessible.
- If the response is cached by a CDN, add `Cache-Control: no-cache`; with a 6–12 hour polling interval this is usually imperceptible.
- If the direct link 404s or higher limits are needed, optionally use `GET https://api.github.com/gists/{id}` (still without a token), parse `files["rkp-sync.age"].content`; if `truncated`, fetch `raw_url` and verify the host is `gist.githubusercontent.com`.
- The receiver does not require a GitHub token; it is used only when the user explicitly chooses "read with token".

Sender-side flow:

1. First time: `POST /gists` with `public: false`, single file.
2. After that: `PATCH /gists/{id}`, replacing the content of the same filename.
3. Read back/parse `updated_at` or the content hash to confirm the write.
4. Retry on failure; do not have several senders writing concurrently.

### 8.2 WebDAV

- Custom paths/filenames are allowed, e.g. `/rkp/phone-a-keybox.age`.
- Sender:
  1. `PUT` to a temp file in the same directory `rkp-sync.age.tmp`;
  2. `MOVE` to the target with `Overwrite: T`;
  3. When MOVE is unsupported, `PUT` the target + `If-Match` (if the server supports it).
- Receiver:
  - `HEAD` for ETag/Last-Modified;
  - `GET` + `If-None-Match`, no-op on 304;
  - without ETag support, GET directly and rely on the decrypted `seq/hash`.
- Authentication: username + app password; the URL Scheme may contain the username (not secret), **never the password**.
- https only; reject cross-host redirects; reject URL userinfo.
- Failures: 401/403 stop and prompt; 409/412/423 re-read/retry next round; 507 warns about the quota.
- The filename can be custom but should be stable; renaming leaves the old ciphertext behind and the receiver must re-import the location.

### 8.3 Public HTTPS file

New `provider = "url"`:

- GET a public HTTPS file directly; no authentication.
- Suitable for WebDAV public share links and static file servers.
- The age passphrase is still required.
- Validation:
  - only `https://` allowed; `http://` rejected by default (explicit switch needed for local debugging);
  - reject URL userinfo;
  - reject localhost/private IPs by default, to prevent SSRF; `--allow-private` for local debugging;
  - restrict redirects to the same host/scheme;
  - limit the response size;
  - do not trust `Content-Type`; parse as age armor.
- If the URL needs a Cookie/Basic Auth, use the WebDAV provider instead; do not try to put credentials into the URL.

### 8.4 Comparison

| | Gist | WebDAV | Public URL |
|---|---|---|---|
| Receiver auth | No token (secret raw URL) | Username + app password | None |
| Sender auth | PAT `gist` | Username + app password | None (read-only) |
| Atomic update | Revision atomic, no CAS | PUT+MOVE / If-Match | Not applicable |
| Custom filename | Fixed | Allowed | Depends on the server |
| Rate limiting | GitHub | Provider | Provider/CDN |
| Suitable for | Quick, no self-hosting | Self-hosted/cloud drive | Public sharing/static sites |

---

## 9. URL Scheme

### 9.1 Format

```text
rkpsync://v1/import?p=<base64url(JSON)>
```

- `p`: unpadded base64url; after decoding, UTF-8 JSON.
- Whole URI limit 2 KiB; JSON limit 4 KiB.
- The QR code content is exactly that URI.
- Pasting the raw URI/JSON inside the receiver is also supported; do not rely on clicking a custom scheme in a browser.

### 9.2 schema

Common fields:

| Field | Type | Required | Notes |
|---|---|---|---|
| `v` | int | Yes | Fixed 1 |
| `provider` | string | Yes | `gist` / `webdav` / `url` |
| `stream_id` | string | Yes | Must match the ciphertext; not secret |
| `label` | string | No | UI display, ≤64 characters, no sensitive information |
| `format` | string | Yes | Fixed `age-armor` |

Gist:

| Field | Required | Notes |
|---|---|---|
| `url` | Recommended | Latest direct link; the receiver prefers it |
| `gist_id` | No | Resolved through the API when `url` is absent (may be tokenless) |
| `filename` | No | Defaults to `rkp-sync.age` |

WebDAV:

| Field | Required | Notes |
|---|---|---|
| `endpoint` | Yes | `https://.../dav/` |
| `path` | Yes | Fixed file path; used after normalization |
| `user` | No | Username; the password is entered separately |

Public URL:

| Field | Required | Notes |
|---|---|---|
| `url` | Yes | Public HTTPS file |

### 9.3 Examples

Gist:

```json
{
  "v": 1,
  "provider": "gist",
  "url": "https://gist.githubusercontent.com/owner/abc123/raw/rkp-sync.age",
  "gist_id": "abc123",
  "filename": "rkp-sync.age",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

WebDAV:

```json
{
  "v": 1,
  "provider": "webdav",
  "endpoint": "https://dav.jianguoyun.com/dav/",
  "path": "/rkp/phone-a-keybox.age",
  "user": "me@example.com",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

Public URL:

```json
{
  "v": 1,
  "provider": "url",
  "url": "https://example.com/rkp/rkp-sync.age",
  "stream_id": "st_01J...",
  "label": "phone-a",
  "format": "age-armor"
}
```

### 9.4 Security validation

The receiver must:

- Reject any `password`, `passphrase`, `token`, `secret`, or `key` field.
- Reject unknown fields (to prevent smuggling).
- Reject URL userinfo (`https://user:pass@...`).
- Allow only known providers; `endpoint`/`url` must be https.
- Reject localhost/private addresses (unless `--allow-private` is explicit).
- Normalize `path`; reject `..`, absolute-path escapes, and control characters.
- `filename` allows only `[A-Za-z0-9._-]+` and must end in `.age`.
- Restrict `stream_id` character set/length.
- Show the decoded provider/endpoint/path/stream/label; ask for the passphrase only after the user confirms.
- Do not put the passphrase in the URL; if the passphrase must be transferred, use a separate secure channel.

### 9.5 Sharing and QR codes

- `rkp-mini-sync share` outputs the URI + QR code + copyable JSON.
- The share UI shows "storage location" and "decryption passphrase" separately; it states explicitly that the QR code does not contain the passphrase.
- The QR code encodes only the URI; never the passphrase/token.
- Custom schemes can be registered by malicious apps; prefer scanning/pasting inside the receiver rather than clicking from a browser.
- `label` is for display only; do not put device serial numbers, IMEI, email addresses, or other sensitive information in it.

---

## 10. Sender

### 10.1 Configuration

The sender adds a `[sync]` section to `config.toml` (or a separate `sync.toml`; reusing the main config is recommended):

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"                 # gist | webdav
stream_id = "st_01J..."
min_validity_days = 14
request_timeout_seconds = 30

# Gist
gist_id = ""
gist_filename = "rkp-sync.age"
gist_url = ""                     # optional: latest direct link for sharing
gist_token_file = "/data/misc/keystore/rkp-mini/sync/gist.token"  # 0600

# WebDAV
webdav_endpoint = ""
webdav_path = "/rkp/rkp-sync.age"
webdav_user = ""
webdav_password_file = "/data/misc/keystore/rkp-mini/sync/webdav.pass" # 0600

# Passphrase
password_mode = "secret-file"     # secret-file | prompt
password_file = "/data/misc/keystore/rkp-mini/sync/age.pass"      # 0600
```

- `risk_ack_version` is written by the UI; while it is unacknowledged, `enabled` must not be true.
- `password_mode = "secret-file"` is the default; the first setup must make the user confirm "the passphrase will be written to disk for automatic runs".
- `gist_token_file` / `webdav_password_file` / `password_file` are all `0600`.
- Do not write the token/passphrase into TOML values; write paths only.

### 10.2 CLI

```text
rkp-mini-sync init                  # initialize the stream/non-sensitive config
rkp-mini-sync push                  # read the keybox, encrypt and upload
rkp-mini-sync share                 # output URL/QR code (no passphrase)
rkp-mini-sync status [--json]       # remote/seq/hash/validity
rkp-mini-sync rotate-password       # re-encrypt the latest keybox with a new passphrase
rkp-mini-sync sync-now              # alias of push/entry point for automatic tasks
rkp-mini-sync forget                # clear local token/passphrase/state (keep the remote)
```

`rkp-mini-sync` does not handle `issue`; automatic tasks are orchestrated by `rkp-mini-issuer` + `rkp-mini-sync`.

### 10.3 push flow

```text
1. Read keybox.xml + meta.json
2. Validate:
   - file exists, 0600, within size limits
   - at least one ecdsa Key; SEC1 private key
   - chain of 2..8, leaf→root links
   - private key matches the leaf public key
   - DeviceID == rkp-mini-<sha256(pubkey)[:12]>
   - leaf not_after >= now + min_validity_days
3. Read the remote rkp-sync.age:
   - absent: base = local (first time seq=0/prev=null)
   - present: decrypt with the current passphrase, take seq/plaintext_sha256
   - decryption failure: stop, do not overwrite the remote
4. If the remote keybox_sha256 == local and seq is unchanged: no-op (unless --force)
5. Choose the base: base = remote.seq >= local.verified_seq ? remote : local
6. Build the CBOR:
   seq = base.seq + 1
   prev = base.plaintext_sha256 (null if there is no history at all)
   issued_at = now
   keybox_xml = local content
7. age armor encryption
8. Upload:
   Gist: PATCH /gists/{id}
   WebDAV: PUT tmp + MOVE / If-Match PUT
9. Read-back verification:
   download/decrypt, compare plaintext_sha256/seq/keybox_sha256
10. Update the local sync-state.json; log only seq/hash/provider
```

### 10.4 Concurrency

- Sender-local `sync.lock` (`tmp/sync.lock` or a dedicated directory), mutually excluding WebUI/CLI/scheduled tasks.
- Gist has no CAS: single writer; read back after writing, and warn if the content is not what you wrote.
- WebDAV with `If-Match` uses the ETag; 412 → re-read, seq+1, retry.
- Two senders writing the same stream at the same time is not a supported scenario; if multiple writers are needed, split the stream.

### 10.5 Automatic issuance + automatic push

> **2026-11 update**: automatic issuance is decoupled from sync. `auto_issue_days` is changed by
> `rkp-mini-issuer config set auto_issue_days N` (`rkp-mini-sync` no longer reads or writes it); the sender loop became the
> issuing module's `auto-loop.sh`: it wakes every 30 minutes and decides by wall clock whether it is due; a due round first issues
> when `auto_issue_days > 0` (sharing `tmp/issue.lock` with the WebUI), then runs `push --auto` when `check-auto` passes;
> when both are off the loop exits. The threshold semantics below are unchanged. On Qualcomm devices, locally issued RKP leaves are valid for only
> about 12–14 days, so the defaults were reduced accordingly: `auto_issue_days = 3` is recommended (the WebUI warns above 14),
> sender `min_validity_days = 3`, receiver `= 2`, and `rotation_lead_days = 4` (days rounded down; it must exceed 3 for a rotation
> to be deployed as soon as it arrives). The 14/7/30 values in the other examples are design-time numbers.

The existing `auto_issue_days` changes from "reserved field" to "remaining-days threshold":

```text
every day/every 12h:
  rkp-mini-issuer issue --if-expiring-days <auto_issue_days>
  rkp-mini-sync push
```

- `days_left > auto_issue_days`: do not issue.
- `days_left <= auto_issue_days`: issue a new keybox; push on success.
- Issuance failure: keep the old keybox, retry next round.
- push failure: the local keybox is already updated, the remote is stale; retry next round; the receiver keeps the old version.
- No "issue every day unconditionally"; that changes identity frequently and trips the receiver's identity gate.
- `issue` and `push` are separate steps; `rkp-mini-issuer` does not link `rkp-mini-sync`.

### 10.6 Sender guard

- Refuse to upload when: the keybox is missing/unreadable, the private key does not match the leaf, the chain structure is wrong, the leaf has too little validity left, or the passphrase file is missing/has wrong permissions.
- `--allow-expiring` is only for manual emergencies and is recorded in the log.
- Logs never contain the keybox, the private key, the age passphrase, the Gist token, or the WebDAV password.
- `share` states explicitly: the link can be public, the passphrase must be sent over a separate secure channel.

---

## 11. Receiver

### 11.1 Configuration

The receiver is a separate module (e.g. `rkp_mini_deployer`) configured as follows:

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 7
request_timeout_seconds = 30

# Gist (direct link preferred; no token needed)
gist_url = "https://gist.githubusercontent.com/.../raw/rkp-sync.age"
gist_id = ""
gist_filename = "rkp-sync.age"

# WebDAV
webdav_endpoint = ""
webdav_path = ""
webdav_user = ""
webdav_password_file = "/data/adb/rkp_sync/secrets/webdav.pass" # 0600

# age passphrase
password_mode = "secret-file"     # secret-file | prompt
password_file = "/data/adb/rkp_sync/secrets/age.pass"           # 0600

[deploy]
identity_change_policy = "scheduled"  # scheduled | always | never
rotation_lead_days = 30
auto_deploy = true

[[deploy.targets]]
id = "omk"
path = "/data/misc/keystore/omk/keybox.xml"
owner = "1017:1017"
mode = "0600"
reload = "auto"

# [[deploy.targets]]
# id = "teesim"
# path = "/data/adb/teesim/keybox.xml"
# owner = "0:0"
# mode = "0600"
# reload = "auto"
```

- `sync-state` lives at `<receiver>/sync/sync-state.json`.
- `deploy-state` lives at `<receiver>/deploy/deploy-state.json`.
- staging lives at `<receiver>/staging/`, `0700`.
- All secrets are `0600`, directories `0700`; they must not enter the module ZIP or cloud backups.

### 11.2 import / fetch

```text
rkp-mini-sync import 'rkpsync://...' # parse the location, show it for confirmation
rkp-mini-sync fetch [--json]         # download/decrypt/verify, write staging
rkp-mini-deployer plan [--json]      # read-only preview (identity gating)
rkp-mini-deployer apply [--json]     # deploy to the local targets (--confirm-first/--allow-identity-change)
rkp-mini-sync status [--json]        # sync + deploy state (read-only)
```

The receiver's fetch + deploy combination is driven by the module script (`action|run|auto` in
`modules/deployer/run.sh`); there is no top-level `run` subcommand: when `fetch` fails, it still runs
`rkp-mini-deployer apply --if-staged`, which re-checks enabled/risk/stream itself.

`import`:

1. Parse the URL/QR; validate schema/no secrets/HTTPS/path.
2. Show provider/endpoint/path/stream/label; the user confirms.
3. On first enable, show the risk disclosure (§13) and write `risk_ack_version`.
4. Prompt for the age passphrase; by default save it to `secret-file` (the user may switch to `prompt`).
5. Optional: test the connection (size only/HEAD; no need to decrypt first).
6. Do not apply the keybox automatically; `fetch` + `deploy` are required.

`fetch`:

1. Read the remote; conditional request (ETag/Last-Modified/API updated_at).
2. Size limits; age decryption.
3. CBOR schema/stream/seq/prev/time/leaf validation.
4. Strict keybox validation (§11.4).
5. Write staging + `sync-state`; output only sync-layer metadata, never read or compare `deploy-state`.
6. Do not modify target files.

`rkp-mini-deployer apply`:

1. Read staging/latest.
2. Read `deploy-state`; compare `keybox_sha256`:
   - same as deployed → idempotent, fix permissions/reload as needed;
   - different → go through the identity gate.
3. Deploy target by target; each target succeeds/fails independently.
4. Update `deploy-state`.
5. Do not modify `sync-state`.

### 11.3 Strict keybox validation

The receiver must implement strict validation independently; string containment is not enough:

- The parser disables DTDs/external entities (recommended: `roxmltree`; reject `<!DOCTYPE` / `<!ENTITY`).
- Size ≤ 128 KiB.
- Exactly one `<AndroidAttestation>`/`<Keybox>` structure (as produced by RKP-Mini).
- At least one `<Key>` with `algorithm="ecdsa"`; reject RSA-only (RKP-Mini never produces it).
- `PrivateKey` is a SEC1 `EC PRIVATE KEY`; parse the private key and derive the public key.
- Certificate chain of 2..8; leaf→root order; issuer/subject links correct.
- The leaf public key matches the private key.
- `DeviceID` == `rkp-mini-` + `sha256(public SEC1)[:12]`.
- Leaf `not_before <= now + skew`; `not_after >= now + min_validity_days`.
- `keybox_sha256` matches the manifest/payload.
- Certificate signature/root trust/revocation checks are optional hardening; v1 at minimum does structure, links, and key match, and labels in the UI "no root trust/revocation check".

### 11.4 Deploy adapters

Targets come only from local configuration; the URL/ciphertext cannot specify paths/commands.

| id | Path | owner / mode | reload | Notes |
|---|---|---|---|---|
| `omk` | `/data/misc/keystore/omk/keybox.xml` | `1017:1017` / `0600` | Module watcher (`reload = "auto"`); `restart` additionally creates `/data/adb/omk/restart.keymint` to restart keymint | Module `oh_my_keymint`; does not sync `config.toml`/`injector.toml`; warns about `retire_stale_keybox_bound_keys` |
| `teesim` | `/data/adb/teesim/<name>.xml`, default `keybox.xml` | `0:0` / `0600` (directory `0700`) | Module watcher | Module `teesim`; must match the keybox filename of a `config.json` profile and must end in `.xml`; the source supports EC-only, chain ≥2 |
| `tricky_store` | `/data/adb/tricky_store/keybox.xml` | `0:0` / `0600` | Module watcher | Module `tricky_store`: TrickyStore or TEESimulator-RS (same id, same path) |
| `custom` | Explicit local configuration | Explicit local configuration | Explicit local configuration | Must be opted into manually; delivering paths/commands remotely is forbidden |

`rkp-mini-deployer targets [--json]` lists the built-in targets in the table above read-only, and detects whether the corresponding module is installed (`/data/adb/modules/<module id>/module.prop` exists; `disable`/`remove` markers → `enabled = false`; a read failure is treated as not installed). `data.builtin[]`: `id`, `path`, `owner`, `mode`, `module { id, installed, enabled, name, version }`. The CLI's `add-target` is not restricted by the detection result.

TEESim notes:

- If a `config.json` profile references `foo.xml`, writing `keybox.xml` on the receiver has no effect; `deploy` should parse `config.json` and confirm the basename of the target path is referenced by at least one profile; otherwise report `target_unreferenced`.
- Do not modify `config.json` automatically.
- The README and the source disagree about RSA requirements; trust the target version's actual behavior.

OMK notes:

- OMK reads `/data/misc/keystore/omk/keybox.xml` with a fixed path; no configuration change is needed.
- With `retire_stale_keybox_bound_keys=true`, replacing the keybox deletes client keys bound to the old keybox; this must be warned about before deploying.
- `chown 1017:1017` must succeed; on failure report an error and do not continue.
- There is an OMK compatibility pre-check before writing (`crates/rkp-mini-deployer/src/omk.rs`): a handwritten scanner replicates the four regexes in `src/keybox.rs` of OMK 1.2.6 (`9069c6c`), `decode_pem`, and `common/src/crypto/ec.rs::import_sec1_private_key`, confirming that OMK will read exactly the same private key and certificate chain as the local validation (chain DER hash, DeviceID, SEC1 namedCurve); it is only stricter than OMK, never looser.
- Result: when the `omk` target is configured, `plan`'s `data.warnings[]` carries `omk_incompatible` at **every** gate; `apply` fails the `omk` target with `targets[].error.code = "omk_incompatible"` before touching that target's file (same repair path as a `same` deploy gate: no write, no permission change, no reload), while other targets continue as usual. The reason text is fixed and contains no keybox text, base64, or key bytes.
- Known residual: OMK also compares the leaf certificate's full SPKI DER with the SPKI BoringSSL derives from the private key; when the SPKI is encoded with a non-namedCurve `AlgorithmIdentifier`, the pre-check cannot detect it.
- Keyboxes issued before this change have private keys without SEC1 `parameters` and are judged incompatible; re-issue (`rkp-mini-issuer issue`) and then deploy. For how other targets treat that format see §4.3: only the `omk` target has a pre-check; `tricky_store` has none.

### 11.5 identity gate and rotation policy

Compare `deploy-state.deployed_keybox_sha256` with staging:

- Same → idempotent deploy; may fix permissions/trigger reload.
- Different → identity change:
  - `scheduled` (default): deploy automatically only when the old keybox has `keybox_not_after - now <= rotation_lead_days` or is already expired, and the new leaf's `not_after > the old not_after`; otherwise record `pending` and notify the user.
  - `always`: deploy any new identity automatically; explicit risk.
  - `never`: requires a manual `rkp-mini-deployer apply --allow-identity-change`.
- First deploy (no deploy-state): requires manual confirmation showing the old/new impact; even with `always` it is not silent.

The OMK warning must be shown/recorded before the actual write; the TEESim hint that existing keys will be re-attested must also be shown.

### 11.6 Atomic write

```text
tmp = target_dir/.<name>.tmp.<pid>
open O_CREAT|O_EXCL 0600
write + fsync
fchmod/fchown
rename(tmp, target)
fsync(target_dir)
restorecon (if needed)
read back target, verify sha256
write deploy-state (atomic)
```

- If any step fails: the target file keeps the old version; deploy-state records the error.
- Do not delete the old file before rename; do not empty the directory.
- Multiple targets: deploy one by one; one failure does not block the others, but status is recorded separately.
- Do not keep a plaintext `.prev` in the target directory; if rollback is needed, the user/deploy layer handles it separately (v1 offers no automatic rollback).

### 11.7 Automatic runs and secret-file

You chose `secret-file` as the default, so both sender and receiver can run unattended for long periods:

- Sender `age.pass` 0600; receiver `age.pass` 0600.
- Sender every day/every 12h: `rkp-mini-issuer issue --if-expiring-days N` + `rkp-mini-sync push`.
- Receiver every 6–12h: `sh modules/deployer/run.sh auto` (`check-auto --require-verified`,
  then `fetch --auto` + `apply --auto --if-staged`).
- Timer: `service.sh` waits for `sys.boot_completed` + network; background loop with `setsid/nohup` + lock + jitter.
- Android Doze/vendor power saving may freeze root shell loops; plain shell cannot guarantee strict timing. If strict timing is needed, the receiver should become an app + WorkManager.
- `secret-file` missing/wrong permissions: the automatic task stops and reports an error; never silently fall back to an empty passphrase or a prompt.
- Zeroize the passphrase as soon as possible after reading it; do not pass it to child process argv; use stdin/fd/inbox files when necessary (§19.2).

---

## 12. Binary split

### 12.1 Recommendation

**Split into a standalone `rkp-mini-sync`; a Cargo workspace + shared core is recommended.**

| Dimension | `rkp-mini-issuer` | `rkp-mini-sync` |
|---|---|---|
| Dependencies | KeyMint/vendor ABI, TA worker, RKP protocol | age, CBOR, HTTP, URL, keybox validation |
| Runs on | The issuing device | Sender + receiver (no RKP hardware needed) |
| Attack surface | High (vendor calls) | Medium (network/passphrase/files) |
| Lifecycle | Issuance/status | Long-term scheduled sync |
| Testing | Needs Android/TA | Fully testable on the host |
| Package size | Stays lean | Separate increment |

Stuffing sync into the single `rkp-mini-issuer` binary is not recommended: age/network/passphrase enlarge the issuing program's dependencies and attack surface together, and the receiver does not need KeyMint code at all.

### 12.2 Recommended layout

```text
crates/
  rkp-mini/          # existing issuance CLI; does not link sync code
  rkp-mini-sync/          # sync/deploy CLI
  rkp-mini-sync-core/     # age, CBOR, URL, state, strict keybox validation
```

Alternatively a separate repository `rkp-mini-sync` + publish `rkp-mini-sync-core`.

- The sender module contains `bin/rkp-mini-issuer` + `bin/rkp-mini-sync`.
- The receiver module contains only `bin/rkp-mini-sync` + receiver deploy adapters/WebUI.
- `rkp-mini-issuer` does not depend on `rkp-mini-sync-core`; the two are orchestrated through files/subprocesses.
- If changing the workspace is undesirable, start with a separate repository; do not use a second `src/bin` sharing all dependencies as the long-term solution.

### 12.3 Dependencies and constraints

Follow the repository's existing style:

- Synchronous blocking I/O; no async runtime, no `clap`, no `reqwest`.
- HTTP: reuse `ureq` (TLS).
- Encryption: `age` (passphrase/armor).
- Serialization: `ciborium`, `serde`, `serde_json`.
- Hashing/encoding: `sha2`, `hex`, `base64`.
- XML: `roxmltree` (pure parsing, no DTD/entities).
- Config: `toml`; time: `time`.
- Errors: `anyhow`; `zeroize` may be used for passphrase memory.
- The release profile matches the main project (`panic = "abort"`; do not rely on catch_unwind).

### 12.4 Packaging

- `build.sh` must cross-compile both binaries (if in the same repository) and inspect the ZIP.
- The sender ZIP root must contain `bin/rkp-mini-issuer`, `bin/rkp-mini-sync`, `module.prop`, hooks, and `webroot/` (if a WebUI is included).
- The receiver has its own ZIP/module; do not stuff receiver code into `rkp_mini_issuer`.
- `rkp-mini-sync` has its own `--version` and protocol version (`sync protocol v1`); do not hard-bind it to the module version.

---

## 13. Scope, UI, and cross-device risk

### 13.1 Repository scope change

This design requires changing `AGENTS.md` / `README.md`:

The original scope text:

```text
No ... remote private-key deployment.
```

becomes:

```text
Remote private-key deployment is opt-in and disabled by default. It encrypts the
keybox to user-controlled storage (Gist/WebDAV/public HTTPS) with an age
passphrase. Enabling it requires explicit risk acceptance. It does not guarantee
Play Integrity, and using a keybox on a different device is keybox cloning /
spoofing, not a legitimate RKP provisioning path.
```

Also add:

- Disabled by default; risk acceptance is required on first enable; it can be disabled in settings.
- The passphrase is written to disk (`secret-file`) by default for automatic runs; this is an explicitly accepted trade-off.
- OMK `config.toml`/`injector.toml` and TEESim `config.json` are not synced.
- The cross-device risk when the receiver is a new device.

### 13.2 Risk acceptance on first enable

The UI must display these and have the user tick each one:

- This keybox is the issuing device's RKP identity; using it on a new device is keybox cloning/impersonation.
- Play Integrity, Key Attestation, and banking/payment apps may refuse it; Google may revoke it.
- The private key is copied to the new device; compromising either device compromises both.
- The age passphrase is saved to a `0600` file to support automatic runs; device loss or root compromise leaks it.
- OMK's `retire_stale_keybox_bound_keys=true` may delete old keys during rotation.
- This is not legitimate cross-device RKP provisioning; the risk is borne by the user.

Write `risk_ack_version` and the timestamp; require re-confirmation when the version changes. Pre-checked boxes and hiding are forbidden.

### 13.3 Settings and disabling

- The settings page keeps an "enable/disable sync" switch.
- **Disable**: stop scheduled tasks, stop fetch/deploy; keep the deployed keybox so the device is not broken abruptly.
- **Disable and clear remote**: delete `age.pass`, the Gist token, the WebDAV password, the URL, and the sync/deploy state; optionally delete staging.
- **Remove the deployed keybox**: a separate operation with a second confirmation; it may leave OMK/TEESim without a keybox.
- Disabling is not revocation: the old keybox still exists; real revocation requires changing the passphrase + re-issuing.
- **2026-11 update (sender)**: automatic push has its own switch `[sync].auto_push` (default `true`; when off, the sender's `check-auto`
  fails with `auto_push_disabled`, stopping only automatic pushes, while manual `push` is unaffected); the sender page no longer offers "disable sync" (the CLI `disable` is kept).
  `remote-delete --yes` (sender only) first decrypts the cloud with the local passphrase and verifies the stream, then performs a conditional delete with the token it just read:
  Gist deletes the whole Gist and clears `gist_id`/`gist_url` (a Gist cannot exist without files; the next push creates a new one and the receiver must re-import),
  WebDAV `DELETE` + `If-Match` (412 → `remote_conflict`); `sync-state` is untouched and seq continues per §7.5.
  **Deleting the cloud file is not revocation**: receivers that already pulled keep the keybox. `forget --reset` (sender only) deletes the whole
  `[sync]` table after `forget` (keeping issuer-side fields), returning to uninitialized; it does not touch the cloud.
- **2026-11 update (both sides)**: `check-remote` checks the cloud read-only: it decrypts and verifies the stream, compares with the local state, and writes the result to
  `<root>/sync/remote-check.json` (`remote_check` in `status --json`), without writing the cloud, staging, `sync-state`, or `deploy-state`.
  `outcome`: `in_sync`, `remote_behind` (sender: the cloud keybox differs from this device and needs a push; receiver: the cloud seq is older than the verified one),
  `remote_ahead` (sender: the cloud seq is newer than the last push; receiver: there is an update to pull), `diverged` (same seq, different content), `missing`, `error`.

### 13.4 Cross-device use

The receiver is not the same device, therefore:

- RKP legitimacy is not promised; this is a keybox clone/impersonation deliberately chosen by the user.
- Each receiving device should get its own stream + its own passphrase; do not distribute one keybox to multiple devices.
- If legitimate multi-device use is needed later, run RKP issuance separately on each device (which requires that device's Qualcomm KeyMint support) instead of copying the keybox.

---

## 14. Failure matrix

| Scenario | Behavior |
|---|---|
| `age.pass` missing/wrong permissions | Automatic task stops; report path/permissions; never guess the passphrase |
| Gist direct link 404 | Keep the current keybox; prompt to re-import/update the link |
| Gist direct link returns stale cache | Wait/retry; the API read is available (still tokenless, watch the rate limit) |
| Gist token 401/403 | Sender stops; prompt to update the token |
| WebDAV 401/403 | Stop; prompt to update the app password |
| WebDAV 409/412/423 | Re-read the ETag; retry next round under a lock conflict |
| Public URL 401/403 | Explain that authentication is needed; switch to the WebDAV provider |
| Decryption failure | `decrypt_failed`; do not overwrite local, do not clear state |
| Invalid CBOR/XML | Reject; keep the current one |
| seq rollback/same seq with a different hash | Reject; warn |
| stream_id mismatch | Reject |
| Leaf below `min_validity_days` | Reject; the sender must re-issue |
| Identity change not allowed | Record `pending`; notify; do not write targets |
| OMK key retirement triggered | Deploy succeeds but keys may be deleted; warn beforehand in the UI |
| keybox passes local validation but OMK cannot read it | `omk_incompatible`; the `omk` target is not written (no OMK template overwrite), other targets proceed; prompt to re-issue or use a keybox OMK can parse |
| Target file write fails | Keep the old file; report path/errno/SELinux |
| reload fails | The file is already on disk; mark degraded; prompt for a manual restart/check the logs |
| Sync disabled | Stop tasks; keep the deployed keybox; do not delete automatically |
| Cross-device attestation failure | Cannot be detected reliably locally; the UI states it is a usage risk, not a sync error |

---

## 15. Security requirements (MUST)

1. Passphrase/token/WebDAV password/keybox/private key **MUST NOT** appear in URLs, QR codes, logs, error messages, WebUI text (except input fields, cleared immediately after submission), external process argv, or environment variables.
2. All secrets/state/staging **MUST** be `0600`; directories `0700`; target keybox permissions follow the adapter's requirements.
3. Passphrase input **MUST** go through stdin/TTY/a `0600` file/the §19.2 inbox; **MUST NOT** use `--password <plaintext>`.
4. Remote responses **MUST** have a size limit; nothing may be buffered without bound before decryption.
5. age decryption failure **MUST** reject application; "skip validation" is not allowed.
6. `seq/prev/stream_id/leaf expiry` **MUST** all be validated.
7. keybox XML parsing **MUST** disable DTDs/external entities; chain length/size **MUST** be limited.
8. The private key and leaf public key **MUST** match; `DeviceID` **MUST** agree with the public key derivation.
9. Deploy path/owner/mode/reload **MUST** come only from local configuration; the remote **MUST NOT** specify them.
10. Only https is allowed; http **MUST** require an explicit `--allow-insecure-http` and is for local debugging only.
11. URLs **MUST** reject userinfo, unknown fields, `..`, and private addresses (by default).
12. Writes **MUST** be atomic: same-directory temp + fsync + rename + fsync(dir).
13. A failed deploy **MUST** keep the old keybox; never delete before writing.
14. The Gist token **MUST** have minimal scope; WebDAV uses an app password; both are revocable.
15. `secret-file` device loss/root compromise is treated as a passphrase leak; change the passphrase + re-issue the keybox.
16. First enable **MUST** require risk acceptance; disabled by default; never enable silently.
17. Logs **MUST** record only seq/hash/provider/path/error codes, never content.

---

## 16. Implementation phases

| Phase | Content | Completion criterion |
|---|---|---|
| **P0** | `rkp-mini-sync-core`: CBOR envelope, age armor, URL parser, strict keybox validation, sync/deploy state; local directory transport | All host unit tests pass; no network/Android dependency |
| **P1** | Standalone `rkp-mini-sync` binary; sender Gist (token) + `push/share`; receiver Gist direct link (no token) + `import/fetch`; manual OMK deploy | Real Gist create/update/pull/decrypt; OMK reload; correct permissions |
| **P2** | WebDAV (custom filename, tmp+MOVE, If-Match) + public URL; TEESim/TrickyStore adapters; deploy-state/identity gate | Tested against Jianguoyun; TEESim `config.json` reference check; identity change blocked |
| **P3** | `secret-file` automatic runs; sender `auto_issue_days` threshold + push; receiver scheduled `run`; rotation policy `scheduled` | Runs automatically after reboot; OMK warning; Doze behavior recorded |
| **P4** | Passphrase rotation, risk UI/scope documentation, cross-device QA, packaging/versioning, fault injection | Acceptance checklist fully passes; docs/AGENTS/README updated |

Every phase must keep `rkp-mini-issuer`'s existing issuance/status contract unchanged; P0 builds the core first so network/passphrase code does not contaminate the issuance path.

---

## 17. Acceptance checklist

### 17.1 Unit/integration

- age passphrase encrypt/decrypt round-trip; wrong passphrase, tampering, truncation, and oversized input are all rejected.
- CBOR fields/size/encoding; `prev`/`seq`/`stream_id` validation.
- URL parser: rejects password/token/unknown fields/userinfo/private addresses/`..`; accepts all three providers.
- keybox validation: EC-only, chain >=2, key match, DeviceID derivation, DTD/XXE, oversized XML.
- state atomic writes; power-loss simulation; staging cleanup; sync/deploy separation (`sync-state` has no target/path).
- Gist: secret gist create/update; tokenless direct-link read on the receiver; cache/404/rate limit.
- WebDAV: custom filename; PUT+MOVE; If-Match; 401/403; quota.
- Public URL: unauthenticated read; SSRF rejection; redirect limits.
- identity gate: same hash idempotent; scheduled window; always/never; manual confirmation on first deploy.
- Automatic tasks: `secret-file` missing/wrong permissions; locking; retry; recovery after reboot.

### 17.2 Security

- No plaintext keybox/private key in remote buckets/gists/WebDAV.
- No passphrase/token in logs/errors/URLs/QR codes.
- No passphrase in `ps`/argv; temp files 0600 and unlinked promptly.
- `secret-file` 0600/directories 0700; correct SELinux context.
- URL parsing rejects secret fields; SSRF/XXE tests pass.
- Disabling sync does not automatically delete the deployed keybox.

### 17.3 Device

- OMK: watcher reload after the write; owner `1017:1017`; the `retire_stale_keybox_bound_keys` warning is visible.
- TEESim: `config.json` reference matches; watcher reload; EC-only measured on device.
- Cross-device: risk acceptance on first enable; attestation behavior after deploying on the new device recorded; failures are not reported as success.
- Automatic: tasks start after reboot; behavior under Doze/power saving recorded; manual `run` as a fallback.
- Old ciphertext/old keybox behavior after passphrase rotation is recorded.

---

## 18. Implementation freedom and boundaries

Do not change settled decisions; the following implementation details are free:

- workspace vs separate repository: a workspace is recommended; if the schedule is tight, start as a separate crate, but do not stuff it into the single `rkp-mini-issuer` binary.
- Gist direct link vs API resolution: direct link by default; the API as a tokenless fallback.
- WebDAV MOVE vs If-Match: choose by the target server's capabilities; keep a failure fallback.
- UI stack: KernelSU WebUI (§19), reusing `webui/`'s Vue + miuix-vue; the CLI keeps equivalent capability. Risk acceptance/disabling must exist in both.
- Whether to keep the most recent staging copy: allowed, but it must be 0600 and removable.
- Whether to add Ed25519: not in v1; if added later, public-key distribution must be designed separately and not mixed into this design.

## 19. WebUI revision (2026-11-10, settled)

The original v1 was CLI-only because "passing a passphrase through `ksu.exec` from the WebUI puts it in argv". Reading the KernelSU manager source showed that premise is false, so the WebUI becomes the primary entry point.

### 19.1 Confirmed facts (KernelSU `main`)

| Fact | Source | Impact |
|---|---|---|
| `exec(cmd[, opts], cb)` uses libsu: `createRootShell` builds `libksud.so debug su [-g]`, and the command is written into that shell's **stdin** via `newJob().add(cmd)` | `ui/webui/WebViewInterface.kt`, `ui/util/KsuCli.kt` | Command text never appears in any process argv; only **external commands** that the shell then `exec`s put arguments into argv |
| `opts.env` is prepended as `export k=v;`, which also goes through stdin | Same | Do not pass secrets via env: child processes inherit environment variables |
| `Shell.enableVerboseLogging = BuildConfig.DEBUG` | `KsuCli.kt` | A **debug** manager writes commands to logcat; a release build does not. This is a residual risk and goes into the risk disclosure |
| WebView origin `https://mui.kernelsu.org`, production CSP `connect-src 'none'` | `docs/WEBUI_DEVELOP.md` | A passphrase in the page cannot leave the device; it can only reach disk through `exec` |

### 19.2 Secret write (inbox)

```text
WebUI: hex = utf8ToHex(password)        // [0-9a-f]; no quoting/escaping/newline issues
exec:  umask 077; mkdir -p <inbox> 2>/dev/null; echo <hex> > <inbox>/<kind>.hex && \
       <bin> --json secret set <kind> --from-file <inbox>/<kind>.hex --hex
```

- Carry the secret using shell **builtins** only (`umask`, `echo`, redirection); the argv of external commands like `mkdir` and `rkp-mini-sync` contains only paths. hex makes `echo` behave the same in mksh/ash/bash; no here-doc (mksh writes temp files).
- inbox: `<data root>/sync/inbox/` on the sender, `/data/adb/rkp_sync/sync/inbox/` on the receiver, `0700`.
- `secret set --from-file`: opened with `O_NOFOLLOW`; must be a regular file owned by the current euid with no group/other bits; ≤ 8 KiB; `--hex` decoded (tolerating one trailing newline); non-empty UTF-8; written atomically to the configured secret path (0600). **Delete the inbox file whether it succeeds or fails**, and clear memory.
- The same mechanism is used for `rotate-password --from-file <f> --hex` (the new passphrase).
- Page: `type=password`, `autocomplete=off`; clear the input and the hex variable immediately after submission; never echo back, never read the stored secret file, only show "set/missing/bad permissions".

### 19.3 Non-interactive confirmation (bound to a version)

| Operation | CLI | Binding |
|---|---|---|
| Risk acceptance | `rkp-mini-sync --json risk` outputs `{version, items[], passphrase_path}`; `enable --accept-risk <version>` | `<version>` must equal the current `RISK_VERSION`; an old UI cannot "agree" on behalf of a newer disclosure |
| First import TOFU | `fetch` fails with `first_import_needs_confirmation`, and the error envelope carries `data` (seq/issued_at/device_id/leaf_not_after/keybox_sha256/plaintext_sha256); `fetch --accept-first <plaintext_sha256>` | Accepts only the exact version that was shown; otherwise `confirmation_mismatch` |
| First deploy / identity change | `rkp-mini-deployer plan --json` (read-only: gate, warnings, targets, old/new); `rkp-mini-deployer apply --confirm-first <keybox_sha256>` / `--allow-identity-change <keybox_sha256>` | Compares the staged sha inside `sync.lock`; if the background loop changed staging meanwhile, reject with `confirmation_mismatch` |

The sha argument accepts a full hex string or a prefix of ≥16 characters. The TTY interaction path is unchanged (y/N still works).

### 19.4 Config editing

- `rkp-mini-sync --json config show`: the effective config (no secret values at all, only secret paths and status).
- `rkp-mini-sync config set <key> <value>`: **whitelisted**, reloaded and validated after writing, rolled back on failure. Sender: `provider`, `label`, `min_validity_days`, `request_timeout_seconds`, `proxy`, `gist_id`, `gist_filename`, `webdav_endpoint`, `webdav_path`, `webdav_user`, `password_mode`, `auto_push`. Receiver: `label`, `min_validity_days`, `request_timeout_seconds`, `proxy`, `password_mode`, `identity_change_policy`, `rotation_lead_days`, `auto_deploy`. Location fields (receiver) are written only through `import`; `enabled`/`risk_ack_*` only through `enable/disable`; deploy targets only through `rkp-mini-deployer add-target/remove-target`.
- The `rkp-mini-issuer` WebUI's previous "config is read-only" decision now applies only to `rkp-mini-issuer`'s own fields; `[sync]` is modified through the commands above, and the top-level `auto_issue_days` through `rkp-mini-issuer config set auto_issue_days N` (the "automatic issuance settings" on the Issue page).

### 19.5 Pages

- **Sender** (new "Remote sync" card + bottom sheet in the `rkp_mini_issuer` WebUI): status (enabled, provider, location, seq, last push/error, local keybox expiry, whether the automatic loop is running); initialize; provider configuration; set the age passphrase / Gist token / WebDAV password; risk acceptance sheet (one checkbox per §13.2 item, none checked by default, submission only when all are checked, including the §19.1 debug manager residual risk); enable/disable; push now; share ("Share" with URI + a QR code rendered locally from `share --json`'s `qr` matrix + a "contains no passphrase" notice); rotate passphrase; clear local data; sync logs.
  **2026-11 revision**: the Sync tab follows the issuer status-page style: a status card (storage, sync time, auto-push; when something is wrong a warning bar shows the cloud check result,
  missing secret file/wrong permissions, an outdated risk disclosure, and the last error) which opens details on tap (location, sync ID, local/cloud keybox, last pushed seq);
  on entry it only reads the local cache, and "Refresh status" runs `check-remote` through the runner. Settings sheet: auto-push (switch + minimum remaining validity),
  cloud storage (including Gist token / WebDAV credentials), encryption ("Passphrase"), other (Label), danger zone (Delete cloud file, Reset encryption passphrase,
  Clear local data, Fully reset sync settings; each asks for confirmation first). Logs live under "More → Logs".
- **Receiver** (`rkp_mini_deployer` module `webroot/`, a second entry of the same Vue project): paste the import link → `import --check` previews the location → `import --yes` after confirmation; risk acceptance; passphrase/WebDAV password; add/remove targets (omk/teesim/tricky_store); policy settings; pull (the TOFU sheet shows the facts, then `--accept-first <sha>`); deploy (run `rkp-mini-deployer plan` first; on first deploy/identity change a sheet shows old→new, the OMK key deletion warning, TEESim re-attest, and "no root trust check", then executes with the sha); pending notice; run now; disable; clear; remove the deployed keybox (two-step confirmation); logs.
- The "Deploy targets" sheet first calls `rkp-mini-deployer targets`: only built-in targets whose `module.installed` is true can be added; targets whose module was not detected are shown as unselectable with the module id; when detection fails no add entry is offered at all. Disabled modules can still be selected, but the UI notes they will not reload until enabled.
- Long tasks (push, fetch, rotate-password) go through `webroot/runner/sync.sh <op> [non-secret args]`, the same `setsid` + status file + polling model as `issue.sh`; short commands (status/plan/config/secret/enable) are `exec`ed directly.
- After enabling, the page immediately starts the module's `sync-loop.sh` with `setsid` (the script has its own pid lock, so duplicate starts are harmless); after disabling, the loop exits at its next wake-up.
- The page never reads or displays the keybox, private key, passphrase, or token; `staging/`, `secrets/`, and `inbox/` never appear in any `cat`/`ls` command.

---

## Appendix A: Configuration examples

### A.1 Sender (excerpt)

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 14
request_timeout_seconds = 30

gist_id = ""
gist_filename = "rkp-sync.age"
gist_url = ""
gist_token_file = "/data/misc/keystore/rkp-mini/sync/gist.token"

webdav_endpoint = ""
webdav_path = "/rkp/rkp-sync.age"
webdav_user = ""
webdav_password_file = "/data/misc/keystore/rkp-mini/sync/webdav.pass"

password_mode = "secret-file"
password_file = "/data/misc/keystore/rkp-mini/sync/age.pass"
```

`auto_issue_days` stays at the top level of the main config; its meaning changes to "remaining-days threshold".

### A.2 Receiver (excerpt)

```toml
[sync]
enabled = false
risk_ack_version = 0
provider = "gist"
stream_id = "st_01J..."
min_validity_days = 7
request_timeout_seconds = 30

gist_url = "https://gist.githubusercontent.com/<owner>/<id>/raw/rkp-sync.age"
gist_id = ""
gist_filename = "rkp-sync.age"

webdav_endpoint = ""
webdav_path = ""
webdav_user = ""
webdav_password_file = "/data/adb/rkp_sync/secrets/webdav.pass"

password_mode = "secret-file"
password_file = "/data/adb/rkp_sync/secrets/age.pass"

[deploy]
identity_change_policy = "scheduled"
rotation_lead_days = 30
auto_deploy = true

[[deploy.targets]]
id = "omk"
path = "/data/misc/keystore/omk/keybox.xml"
owner = "1017:1017"
mode = "0600"
reload = "auto"
```

## Appendix B: CLI JSON contract

Unified output:

```json
{"ok":true,"data":{...}}
{"ok":false,"error":{"stage":"remote|decrypt|validate|state|deploy|config|generic","code":"...","message":"..."}}
```

- stdout carries exactly one envelope; diagnostics go to stderr.
- Exit codes:
  - `0` success
  - `2` usage/config/missing risk acceptance
  - `8` sync/network/decrypt/validation/state failure
  - `9` deploy/write/reload failure
  - `1` anything else

`push --json` data:

```json
{
  "provider": "gist",
  "stream_id": "st_...",
  "seq": 42,
  "ciphertext_sha256": "...",
  "url": "https://gist.githubusercontent.com/.../raw/rkp-sync.age",
  "uploaded_at": 1762750000
}
```

`fetch --json` data:

```json
{
  "provider": "gist",
  "stream_id": "st_...",
  "seq": 42,
  "plaintext_sha256": "...",
  "keybox_sha256": "...",
  "device_id": "rkp-mini-...",
  "leaf_not_after": 1794000000,
  "staged_path": "/data/adb/rkp_sync/staging/<sha>.xml",
  "previous_seq": 41
}
```

`rkp-mini-deployer apply --json` data:

```json
{
  "keybox_sha256": "...",
  "seq": 42,
  "targets": [
    {
      "id": "omk",
      "path": "/data/misc/keystore/omk/keybox.xml",
      "ok": true,
      "sha256": "...",
      "reload": "auto",
      "reload_status": "ok",
      "error": null
    }
  ]
}
```

## Appendix C: References

- age: <https://age-encryption.org/>
- GitHub Gist API: <https://docs.github.com/en/rest/gists>
- WebDAV (RFC 4918): <https://www.rfc-editor.org/rfc/rfc4918>
- OhMyKeymint: <https://github.com/qwq233/OhMyKeymint>
- TEESimulator: <https://github.com/JingMatrix/TEESimulator>
- TrickyStore: <https://github.com/5ec1cff/TrickyStore>
- Existing WebUI design: `docs/WEBUI_DEVELOP.md`
- Repository constraints: `AGENTS.md`

---

## Appendix D: Pre-implementation checks

The implementing agent should confirm before writing code:

- [ ] `AGENTS.md` and this document have been read; no server has been reintroduced.
- [ ] `sync-state` contains no target/path/owner/reload.
- [ ] No passphrase/token in URLs/QR codes/logs.
- [ ] The Gist receiver uses the direct link by default and does not require a token.
- [ ] `rkp-mini-sync` builds independently; `rkp-mini-issuer` does not link sync dependencies.
- [ ] First enable requires risk acceptance, is disabled by default, and can be disabled.
- [ ] The OMK `retire_stale_keybox_bound_keys` warning is implemented.
- [ ] The TEESim `config.json` keybox reference check is implemented.
- [ ] Permission/missing/rotation behavior is covered when automatic runs use `secret-file`.
- [ ] The cross-device risk text is written into the UI and README/AGENTS.
