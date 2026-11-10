# RKP Mini

Minimal KernelSU module that issues an Android RKP keybox on **Qualcomm** devices
by driving the on-device KeyMint TA as a CSR signing oracle (Native V2, vendor
command `0x3122`). Target: a single Qualcomm reference device (arm64); other devices and firmware are untested.

> Scope: **only** Native V2 (`0x3122`). No V1, no AIDL V2, no TEE/boot-state spoofing.
> See the original external design note.
>
> Remote private-key deployment (`rkp-sync`) is opt-in and disabled by default. It
> encrypts the keybox to user-controlled storage (Gist/WebDAV/public HTTPS) with an age
> passphrase. Enabling it requires explicit risk acceptance. It does not guarantee Play
> Integrity, and using a keybox on a different device is keybox cloning / spoofing, not a
> legitimate RKP provisioning path.

## What it does

- `rkp-mini status` — read-only prereq + keybox check, updates module description.
- `rkp-mini issue --acknowledge` — generate P-256 key → fetch RKP challenge → sign
  CSR via KeyMint `0x3122` → `signCertificates` → validate chain → write keybox.
- KernelSU WebUI — status, prerequisite checks, one-tap issuance with result/failure
  diagnostics, certificate details, issue logs and read-only config.

## Build

```sh
./build.sh
```

Produces `dist/rkp_mini-v0.1.0.zip` (binary, hooks and `webroot/`), installable from the KernelSU manager.

Requires the Android NDK (`$NDK`, default `~/Android/Sdk/ndk/30.0.16248370`),
`rustup target add aarch64-linux-android` and [Bun](https://bun.sh) for the WebUI.
WebUI-only iteration: `cd webui && bun run dev` previews it in a browser against fixtures.

## Install & use

1. KernelSU manager → install `dist/rkp_mini-v0.1.0.zip` → reboot (or reopen module page).
2. Tap **Open** on the module card for the WebUI: check status, then **签发新证书**.
   Issuance runs in the background; closing the WebUI does not interrupt it, and reopening
   shows the result. Alternatively tap **Action**, or run:

```sh
su -c '/data/adb/modules/rkp_mini/bin/rkp-mini issue --acknowledge'
```

3. Artifacts land in `/data/misc/keystore/rkp-mini/output` (`keybox.xml`, `cert-chain.pem`,
   `cert-chain.der`, `private-key.pem`, `meta.json`). Copy `keybox.xml` into your keystore
   module (e.g. TrickyStore) yourself, or use the opt-in remote sync below.

When issuance fails, the WebUI result sheet shows the stage, error code, `ta_status` and
stderr; full logs live in `/data/misc/keystore/rkp-mini/logs/issue-<unix>.log` (also
viewable under WebUI → 设置 → 签发日志). A failed run leaves the existing `keybox.xml` in place.

## go/no-go (reference device)

KeyMint 3 firmware may reject `0x3122`:

- `-100` UNIMPLEMENTED — TA lacks the command.
- `-18` INCOMPATIBLE_KEY_FORMAT — TA wants MacedPublicKey; Native V2 unavailable.
- `-40` INVALID_TAG — vendor command/frame mismatch.
- HTTP `444` from `signCertificates` — device not registered.

On any of these the module stops with a clear error; it never falls back to V1/AIDL.

## Remote keybox sync (`rkp-sync`, opt-in)

Design: `docs/REMOTE_SYNC_DEVELOP.md`. No server, accounts or remote revocation: the keybox
is sealed into one ASCII-armored age file (`rkp-sync.age`, scrypt passphrase) together with a
CBOR manifest (`stream_id`, monotonic `seq`, `prev` hash, keybox digests). The cloud only ever
sees ciphertext; links and QR codes carry the location only, never the passphrase or a token.

- **Sender** (`rkp_mini` module, ships `bin/rkp-sync`): uploads to a secret GitHub Gist
  (PAT with `gist` scope) or WebDAV (app password; PUT temp + MOVE, `If-Match` fallback).
- **Receiver** (separate `rkp_sync` module, `dist/rkp_sync-v*.zip`, contains only `rkp-sync` and its WebUI):
  reads a Gist raw link without a token, WebDAV, or a public HTTPS file; decrypts, enforces
  `stream_id`/`seq`/`prev`/expiry and strict keybox checks (no DTD, EC-only, chain 2..8 leaf→root,
  key/leaf match, DeviceID derivation), stages the keybox (`0600`), then a separate deploy
  layer writes it atomically to locally configured targets: OhMyKeymint
  (`/data/misc/keystore/omk/keybox.xml`, `1017:1017`), TEESimulator (`/data/adb/teesim/`, must be
  referenced by `config.json`), TrickyStore, or a custom path. Tool configs (OMK `config.toml` /
  `injector.toml`, TEESim `config.json`) are never synced.
- Certificate signatures, root trust and revocation are **not** verified.

**WebUI (recommended).** Sender: rkp_mini module → Open → “远程同步” card: initialize, choose
Gist/WebDAV, set the age passphrase and token/app password, tick every risk item to enable,
push, then share the link/QR. Receiver: rkp_sync module → Open: paste the link, review the
location, accept the risks, enter the passphrase received over a separate channel, add a
deployment target, fetch (confirm the first version's seq/device/expiry), deploy (confirm the
first deployment). Secrets typed into the pages are written by shell builtins into a 0600 inbox
file that `rkp-sync` consumes and deletes; they never appear in process arguments, URLs or logs.
A *debug* build of the KernelSU manager logs shell commands to logcat — use a release build.

**CLI** (root shell; the same operations). Sender, `rkp-sync` = `/data/adb/modules/rkp_mini/bin/rkp-sync`:

```sh
rkp-sync --role sender init --provider gist   # adds a disabled [sync] table to config.toml
rkp-sync secret set age                       # passphrase from TTY (no echo) → sync/age.pass 0600
rkp-sync secret set gist-token                # or: secret set webdav-password
rkp-sync enable                               # shows the risk statement; type I ACCEPT
                                              # (non-interactive: review `rkp-sync risk`, then
                                              #  `rkp-sync enable --accept-risk <version>`)
rkp-sync push                                 # first push creates the secret gist
rkp-sync share                                # import link + QR (no password inside)
```

Receiver, `rkp-sync` = `/data/adb/modules/rkp_sync/bin/rkp-sync`:

```sh
rkp-sync import 'rkpsync://v1/import?p=…'     # review location, accept risk, enter passphrase
rkp-sync deploy add-target omk                # or teesim / tricky_store
rkp-sync fetch                                # first version: trust-on-first-use confirmation
                                              # (non-interactive: --accept-first <plaintext_sha256>)
rkp-sync deploy plan                          # preview; then deploy apply [--confirm-first <keybox_sha256>]
```

Unattended runs (`password_mode = "secret-file"`, the default) start at boot when
`rkp-sync check-auto` passes: the sender every ~12 h runs `rkp-mini issue --if-expiring-days
<auto_issue_days>` (when `auto_issue_days > 0`) then `rkp-sync push --auto`; the receiver every
~8 h runs `rkp-sync run --auto`. Doze/vendor power saving may delay these loops. Logs:
`logs/sync.log` under each data root (seq/hash/provider/error codes only).

Identity changes on the receiver follow `[deploy].identity_change_policy`: `scheduled`
(default) deploys a new keybox automatically only within `rotation_lead_days` of the old
leaf's expiry and when the new leaf outlives it; otherwise it is held as `pending`. OMK's
`retire_stale_keybox_bound_keys` (default true) deletes client keys bound to a replaced keybox.

Disable with `rkp-sync disable` (stops loops, keeps deployed keyboxes); `rkp-sync forget`
deletes local secrets/state; `rkp-sync deploy remove --yes` deletes deployed keyboxes.
Disabling is not revocation: change the passphrase (`rkp-sync rotate-password`) **and**
re-issue the keybox. Old Gist revisions still decrypt with the old passphrase. Treat a lost
device or root compromise on either side as a passphrase and key leak.

## Extension points (not implemented)

- Ed25519-signed manifests (v1 relies on the age passphrase alone).

## License

MIT. Portions modelled on Duck-ToolBox (MIT).
