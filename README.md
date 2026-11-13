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
- `rkp-mini-issuer remove --yes` — delete the issued keybox, its `.prev` backup, private key, chain
  and `meta.json` from the output directory (refused while an issuance holds `tmp/issue.lock`).
- `rkp-mini-sync` — sender: `push`, `share`, `rotate-password`, `remote-delete`; receiver: `import`,
  `fetch`; both: `check-remote` (read-only update check).
- `rkp-mini-deployer` — `plan`, `apply`, `targets`, `add-target`, `remove-target`, `remove`.
- KernelSU WebUI — issuer page: status, prerequisite checks, one-tap issuance with
  result/failure diagnostics, certificate details, issue logs, read-only config and the
  remote-sync (sender) section. Deployer page: import, first-use confirmation, deployment
  preview/approval, target management and the combined fetch+deploy result.

## Build

```sh
./build.sh
```

Produces `dist/rkp_mini_issuer-v0.4.0.zip` and `dist/rkp_mini_deployer-v0.4.0.zip`
(binaries, hooks and `webroot/`), both installable from the KernelSU manager. Every
`rkp-mini-*` dependency of a package is compiled from the same workspace: the two ZIPs ship
a byte-identical `rkp-mini-sync`.

Requires the Android NDK (`$NDK`, default `~/Android/Sdk/ndk/30.0.16248370`),
`rustup target add aarch64-linux-android` and [Bun](https://bun.sh) for the WebUI.
WebUI-only iteration: `cd webui && bun run dev:issuer` / `bun run dev:deployer` preview
each page in a browser against fixtures.

## Install & use

1. KernelSU manager → install `dist/rkp_mini_issuer-v0.4.0.zip` → reboot (or reopen the module
   page). The installer refuses to run while the legacy `rkp_mini` module is still installed:
   uninstall it first (its data at `/data/misc/keystore/rkp-mini` is kept and reused).
2. Tap **Open** on the module card for the WebUI: check the 状态 tab, then 签发 → **立即签发**.
   Issuance runs in the background; closing the WebUI does not interrupt it, and reopening
   shows the result. From a root shell:

```sh
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge'
```

3. Artifacts land in `/data/misc/keystore/rkp-mini/output` (`keybox.xml`, `cert-chain.pem`,
   `cert-chain.der`, `private-key.pem`, `meta.json`). Copy `keybox.xml` into your keystore
   module yourself, or use the opt-in remote sync / deployer module below.

When issuance fails, the WebUI result sheet shows the stage, error code, `ta_status` and
stderr; full logs live in `/data/misc/keystore/rkp-mini/logs/issue-<unix>.log` (also
viewable under WebUI → 更多 → 日志). A failed run leaves the existing `keybox.xml` in place.

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

The WebUI's 立即部署 runs `fetch` and then `apply --if-staged`, so a failed fetch still repairs a
damaged target from staging that was already verified; a first deployment or an identity change
outside the policy stays `pending` until it is approved. `sh run.sh action` in the module directory
does the same in a root shell.

Auto-issue does not need sync: `rkp-mini-issuer config set auto_issue_days 3` (or WebUI → 签发 →
自动签发设置; `0` turns it off) makes the issuer module's `auto-loop.sh` run `rkp-mini-issuer issue
--if-expiring-days 3` about every 12 h, so a new keybox is issued only when the current leaf has
≤ 3 days left. Native Qualcomm RKP leaves are valid for only ~12–14 days, so keep N small: N ≥ 12
re-issues on every check (the WebUI warns above 14). The same loop runs `sync push --auto` right after while `rkp-mini-sync check-auto`
passes (sync enabled, `password_mode = "secret-file"`, the default). The receiver loop runs every
~8 h only while sync is enabled: `sync check-auto --require-verified`, then `sync fetch --auto` and
`deployer apply --auto --if-staged`. Doze/vendor power saving may delay these loops. Logs:
`logs/auto.log` (sender) and `logs/sync.log` (receiver) under each data root (seq/hash/provider/error
codes only); the last auto-issue check is kept in `tmp/auto-issue.last.*`.

The **Action** button on either module card runs one round of that module's loop right now
(issuer: auto-issue if due, then auto-push; deployer: the verified gate, `fetch --auto`,
`apply --auto --if-staged`). The round's lines appear in the Action console and are appended to
the same log the WebUI shows (更多 → 日志 → 后台日志). It is not a forced issuance: with
`auto_issue_days = 0` the issuer round skips issuing; use 立即签发 for that.
Action's prompts and step summaries follow the language last synchronized from the WebUI, using
the owning binary's embedded catalog. Shared loop log messages use the same language; JSON results,
machine codes, exit behavior and raw binary diagnostics remain unchanged. Before the first language
synchronization, or if the saved language is invalid, these prompts default to Simplified Chinese.

The module card description follows the language selected in WebUI → More → Appearance → Language.
Opening the WebUI or changing its language saves the resolved language for subsequent native status
updates, including background issuance; refresh the KernelSU module list to see the new description.
The deployer translates its existing introduction, not a new live status summary. With “System” selected,
system-language changes are picked up the next time the WebUI opens. Before the first synchronization,
the module description remains Chinese; raw diagnostic details retain their original language.
Descriptions are rendered by the owning binary from its embedded `rust-i18n` catalog, independently
of the WebUI's `vue-i18n` messages. The WebUI shares only the active language code and calls the owning
binary's `update-description` command. The issuer refresh checks local status without issuing a keybox;
the deployer refreshes its introduction without initializing or enabling sync. If native synchronization
fails, the WebUI keeps the selected language and reports the error on a manual language change.

Identity changes on the receiver follow `[deploy].identity_change_policy`: `scheduled`
(default) deploys a new keybox automatically only within `rotation_lead_days` (default 4) of the old
leaf's expiry and when the new leaf outlives it; otherwise it is held as `pending`. OMK's
`retire_stale_keybox_bound_keys` (default true) deletes client keys bound to a replaced keybox.

Disable with `sync disable` (stops loops, keeps deployed keyboxes); on the sender
`sync config set auto_push false` stops only the automatic push. `sync forget` deletes local
secrets/state (`--reset` on the sender also drops the `[sync]` table); `sync remote-delete --yes`
deletes the sender's remote file (a whole Gist); `deployer remove --yes` deletes deployed keyboxes.
Uninstalling either module keeps its data root. Neither disabling nor deleting the remote file is
revocation: receivers keep what they fetched. Change the passphrase
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
