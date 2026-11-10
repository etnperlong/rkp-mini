# RKP Mini

Minimal KernelSU modules that issue an Android RKP keybox on **Qualcomm** devices
by driving the on-device KeyMint TA as a CSR signing oracle (Native V2, vendor
command `0x3122`). Target: a single Qualcomm reference device (arm64); other devices and firmware are untested.

> Scope: **only** Native V2 (`0x3122`). No V1, no AIDL V2, no TEE/boot-state spoofing.
> See the original external design note.
>
> Remote private-key deployment (`rkp-mini-sync`) is opt-in and disabled by default. It
> encrypts the keybox to user-controlled storage (Gist/WebDAV/public HTTPS) with an age
> passphrase. Enabling it requires explicit risk acceptance. It does not guarantee Play
> Integrity, and using a keybox on a different device is keybox cloning / spoofing, not a
> legitimate RKP provisioning path.

Three commands, three jobs — they never call each other:

| Command | Job |
|---|---|
| `rkp-mini-issuer` | obtain a keybox from the KeyMint TA, inspect local status |
| `rkp-mini-sync` | push the keybox to / fetch it from encrypted remote storage |
| `rkp-mini-deployer` | write an already-verified staged keybox to local targets |

Two modules use them: **RKP-Mini Issuer** (`rkp_mini_issuer`, ships issuer + sync) and
**RKP-Mini Deployer** (`rkp_mini_deployer`, ships sync + deployer).

## What it does

- `rkp-mini-issuer status` — read-only prereq + keybox check, updates module description.
- `rkp-mini-issuer issue --acknowledge` — generate P-256 key → fetch RKP challenge → sign
  CSR via KeyMint `0x3122` → `signCertificates` → validate chain → write keybox.
- `rkp-mini-sync` — sender: `push`, `share`, `rotate-password`; receiver: `import`, `fetch`.
- `rkp-mini-deployer` — `plan`, `apply`, `targets`, `add-target`, `remove-target`, `remove`.
- KernelSU WebUI — issuer page: status, prerequisite checks, one-tap issuance with
  result/failure diagnostics, certificate details, issue logs, read-only config and the
  remote-sync (sender) section. Deployer page: import, first-use confirmation, deployment
  preview/approval, target management and the combined fetch+deploy result.

## Build

```sh
./build.sh
```

Produces `dist/rkp_mini_issuer-v0.2.0.zip` and `dist/rkp_mini_deployer-v0.2.0.zip`
(binaries, hooks and `webroot/`), both installable from the KernelSU manager. Every
`rkp-mini-*` dependency of a package is compiled from the same workspace: the two ZIPs ship
a byte-identical `rkp-mini-sync`.

Requires the Android NDK (`$NDK`, default `~/Android/Sdk/ndk/30.0.16248370`),
`rustup target add aarch64-linux-android` and [Bun](https://bun.sh) for the WebUI.
WebUI-only iteration: `cd webui && bun run dev:issuer` / `bun run dev:deployer` preview
each page in a browser against fixtures.

## Install & use

1. KernelSU manager → install `dist/rkp_mini_issuer-v0.2.0.zip` → reboot (or reopen the module
   page). The installer refuses to run while the legacy `rkp_mini` module is still installed:
   uninstall it first (its data at `/data/misc/keystore/rkp-mini` is kept and reused).
2. Tap **Open** on the module card for the WebUI: check status, then **签发新证书**.
   Issuance runs in the background; closing the WebUI does not interrupt it, and reopening
   shows the result. Alternatively tap **Action**, or run:

```sh
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge'
```

3. Artifacts land in `/data/misc/keystore/rkp-mini/output` (`keybox.xml`, `cert-chain.pem`,
   `cert-chain.der`, `private-key.pem`, `meta.json`). Copy `keybox.xml` into your keystore
   module yourself, or use the opt-in remote sync / deployer module below.

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

## Remote keybox sync and local deployment (opt-in)

Design: `docs/REMOTE_SYNC_DEVELOP.md`. No server, accounts or remote revocation: the keybox
is sealed into one ASCII-armored age file (`rkp-sync.age`, scrypt passphrase) together with a
CBOR manifest (`stream_id`, monotonic `seq`, `prev` hash, keybox digests). The cloud only ever
sees ciphertext; links and QR codes carry the location only, never the passphrase or a token.

- **Sender** (`rkp_mini_issuer` module, ships `bin/rkp-mini-sync`): uploads to a secret GitHub
  Gist (PAT with `gist` scope) or WebDAV (app password; PUT temp + MOVE, `If-Match` fallback).
- **Receiver** (`rkp_mini_deployer` module): `rkp-mini-sync import`/`fetch` read a Gist raw link
  without a token, WebDAV, or a public HTTPS file; they decrypt, enforce
  `stream_id`/`seq`/`prev`/expiry and strict keybox checks (no DTD, EC-only, chain 2..8 leaf→root,
  key/leaf match, DeviceID derivation) and stage the keybox (`0600`). `rkp-mini-deployer apply`
  then writes it atomically to locally configured targets: OhMyKeymint
  (`/data/misc/keystore/omk/keybox.xml`, `1017:1017`; skipped with `omk_incompatible` when a
  precheck mirroring OMK's own scanner predicts OMK would reject the keybox and replace it with
  its bundled template), TEESimulator (a `.xml` file in
  `/data/adb/teesim/`, must be referenced by `config.json`), TrickyStore or TEESimulator-RS
  (both read `/data/adb/tricky_store/keybox.xml`), or a custom opt-in path. Each of these
  modules watches its keybox directory and reloads the replaced file without a reboot.
  `rkp-mini-deployer targets` lists the built-in targets and whether the owning module is
  installed; the WebUI only offers targets whose module was detected. Tool configs (OMK
  `config.toml` / `injector.toml`, TEESim `config.json`) are never synced.
- Certificate signatures, root trust and revocation are **not** verified.

**WebUI (recommended).** Sender: issuer module → Open → “远程同步” card: initialize, choose
Gist/WebDAV, set the age passphrase and token/app password, tick every risk item to enable,
push, then share the link/QR. Receiver: deployer module → Open: paste the link, review the
location, accept the risks, enter the passphrase received over a separate channel, add a
deployment target, run the task (confirm the first version's seq/device/expiry, review the
plan and deploy with the sha of the keybox you just saw). Secrets typed into the pages are
written by shell builtins into a 0600 inbox file that `rkp-mini-sync` consumes and deletes;
they never appear in process arguments, URLs or logs. A *debug* build of the KernelSU manager
logs shell commands to logcat — use a release build.

**CLI** (root shell; the same operations). Sender, `SYNC` = `/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-sync`:

```sh
SYNC --role sender --config /data/misc/keystore/rkp-mini/config.toml init --provider gist
SYNC --role sender secret set age            # passphrase from TTY (no echo) → sync/age.pass 0600
SYNC --role sender secret set gist-token     # or: secret set webdav-password
SYNC --role sender enable                    # shows the risk statement; type I ACCEPT
                                             # (non-interactive: review `risk`, then
                                             #  `enable --accept-risk <version>`)
SYNC --role sender push                      # first push creates the secret gist
SYNC --role sender share                     # import link + QR (no password inside)
```

Receiver, `SYNC` = `/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-sync` and
`DEPLOY` = `/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer`
(append `--config /data/adb/rkp_sync/config.toml --role receiver` to both):

```sh
SYNC import 'rkpsync://v1/import?p=…'         # review location, accept risk, enter passphrase
DEPLOY targets                               # which target modules are installed
DEPLOY add-target omk                        # or teesim / tricky_store
SYNC fetch                                   # first version: trust-on-first-use confirmation
                                             # (non-interactive: --accept-first <plaintext_sha256>)
DEPLOY plan                                  # preview
DEPLOY apply --confirm-first <keybox_sha256> # or --allow-identity-change for a rotation
```

The module Action (and the WebUI task) runs `fetch` and then `apply --if-staged`, so a failed
fetch still repairs a damaged target from staging that was already verified; a first
deployment or an identity change outside the policy stays `pending` until it is approved.

Unattended runs (`password_mode = "secret-file"`, the default) start at boot when
`rkp-mini-sync check-auto` passes: the sender every ~12 h runs `rkp-mini-issuer issue
--if-expiring-days <auto_issue_days>` (when `auto_issue_days > 0`) then `sync push --auto`; the
receiver every ~8 h runs `sync check-auto --require-verified`, then `sync fetch --auto` and
`deployer apply --auto --if-staged`. Doze/vendor power saving may delay these loops. Logs:
`logs/sync.log` under each data root (seq/hash/provider/error codes only).

Identity changes on the receiver follow `[deploy].identity_change_policy`: `scheduled`
(default) deploys a new keybox automatically only within `rotation_lead_days` of the old
leaf's expiry and when the new leaf outlives it; otherwise it is held as `pending`. OMK's
`retire_stale_keybox_bound_keys` (default true) deletes client keys bound to a replaced keybox.

Disable with `sync disable` (stops loops, keeps deployed keyboxes); `sync forget` deletes local
secrets/state; `deployer remove --yes` deletes deployed keyboxes. Uninstalling either module
keeps its data root. Disabling is not revocation: change the passphrase
(`sync rotate-password`) **and** re-issue the keybox. Old Gist revisions still decrypt with the
old passphrase. Treat a lost device or root compromise on either side as a passphrase and key
leak.

### Migrating from the old single-module layout

The old `rkp_mini` (sender) and `rkp_sync` (receiver) modules use the same data roots. Disable
and uninstall the old module, keep `/data/adb/rkp_sync` (the old receiver uninstaller deletes
it — back it up first), then install the new module; the new installers refuse to run while the
legacy module of the same role is present.

## Extension points (not implemented)

- Ed25519-signed manifests (v1 relies on the age passphrase alone).

## License

MIT. Portions modelled on Duck-ToolBox (MIT).
