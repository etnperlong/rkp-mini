# Usage guide

**English** | [简体中文](USAGE_ZH.md)

Beyond the [README](../README.md): the command line, background automation, revocation and
migration. Everything here runs in a root shell on the device. The protocol and threat model
are in [`REMOTE_SYNC_DEVELOP.md`](REMOTE_SYNC_DEVELOP.md).

## Commands

Three independent binaries; they never call each other.

| Command | Job | Shipped in |
|---|---|---|
| `rkp-mini-issuer` | Obtain a keybox from the KeyMint TA, inspect local status | Issuer |
| `rkp-mini-sync` | Push the keybox to / fetch it from encrypted remote storage | Issuer, Deployer |
| `rkp-mini-deployer` | Write an already-verified staged keybox to local targets | Deployer |

Binaries live in `/data/adb/modules/<module id>/bin/`. Most commands take `--json` for a machine-readable result.

### Issuer

```sh
ISSUER=/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer
$ISSUER status                          # local prerequisite + keybox check (no network, no TA)
$ISSUER issue --acknowledge             # real issuance: new key, network, TA
$ISSUER config set auto_issue_days 3    # auto-issue when ≤ 3 days remain; 0 = off
$ISSUER remove --yes                    # delete the issued keybox, key, chain and metadata
```

Output: `/data/misc/keystore/rkp-mini/output/` (`keybox.xml`, `cert-chain.pem`, `cert-chain.der`,
`private-key.pem`, `meta.json`). Logs: `/data/misc/keystore/rkp-mini/logs/issue-<unix>.log`.

### Sync: sender

`SYNC` = `/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-sync`

```sh
SYNC --role sender --config /data/misc/keystore/rkp-mini/config.toml init --provider gist
SYNC --role sender secret set age            # passphrase from the TTY (no echo), stored 0600
SYNC --role sender secret set gist-token     # or: secret set webdav-password
SYNC --role sender enable                    # shows the risk statement; type I ACCEPT
                                             # (non-interactive: review `risk`, then
                                             #  `enable --accept-risk <version>`)
SYNC --role sender push                      # the first push creates the secret gist
SYNC --role sender share                     # import link + QR (no passphrase inside)
```

The Gist token is a GitHub PAT with only the `gist` scope. For WebDAV use a separately
revocable app password.

### Sync and deploy: receiver

`SYNC` = `/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-sync`,
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

Other commands: `check-remote` (both roles; read-only update check), `rotate-password` and
`remote-delete` (sender), `remove-target` and `remove` (deployer).

## Deployment targets

| Target | Path | Notes |
|---|---|---|
| `omk` (OhMyKeymint) | `/data/misc/keystore/omk/keybox.xml` | Skipped with `omk_incompatible` when OMK's own scanner would reject the keybox |
| `teesim` (TEESimulator) | a `.xml` file in `/data/adb/teesim/` | Must be referenced by TEESimulator's `config.json` |
| `tricky_store` (TrickyStore, TEESimulator-RS) | `/data/adb/tricky_store/keybox.xml` | |
| custom | any path you configure | Opt-in |

The WebUI offers a built-in target only when its module is installed. These modules watch
their keybox file and should pick up a replacement without a reboot; reboot if a new keybox
does not take effect. Tool configs (OMK `config.toml` / `injector.toml`, TEESimulator
`config.json`) are never synced.

## Background automation

- **Issuer**: about every 12 h, issues a new keybox when the current one has ≤ `auto_issue_days`
  left, then pushes it if sync is enabled. Keep the value small: leaves are valid for only
  ~12–14 days, so 12 or more re-issues on every round.
- **Deployer**: about every 8 h while sync is enabled, fetches and deploys.
- **Action** button on a module card: runs one such round now and shows its log. It is not a
  forced issuance; with auto-issue off the Issuer round skips issuing (use **Issue now**).
- Doze and vendor power saving may delay the loops. Logs: `logs/auto.log` (Issuer) and
  `logs/sync.log` (Deployer) under each data root, also in WebUI → **More → Logs**.

**Deploy now** in the WebUI fetches and then applies whatever is staged, so a failed fetch
still repairs a damaged target from a keybox that was already verified. A first deployment,
or an identity change outside the policy, stays pending until you approve it.

Identity changes follow `[deploy].identity_change_policy`. The default `scheduled` deploys a
new keybox automatically only within `rotation_lead_days` (default 4) of the old leaf's
expiry and when the new leaf outlives it. OMK's `retire_stale_keybox_bound_keys` (default
true) deletes client keys bound to a replaced keybox.

## Language

Module card descriptions and Action output follow the language chosen in WebUI →
**More → Language**; refresh the KernelSU module list to see the change. Before the WebUI is
first opened they are in Simplified Chinese. Raw diagnostics keep their original language.

## Secrets

Secrets typed into the WebUI go into a `0600` inbox file that `rkp-mini-sync` consumes and
deletes; they never appear in process arguments, URLs or logs. A *debug* build of the
KernelSU manager logs shell commands to logcat: use a release build.

## Disable, delete, revoke

| Goal | Command |
|---|---|
| Stop sync, keep deployed keyboxes | `sync disable` |
| Stop only the automatic push (sender) | `sync config set auto_push false` |
| Delete local secrets and state | `sync forget` (`--reset` on the sender also drops the `[sync]` table) |
| Delete the remote file (the whole Gist) | `sync remote-delete --yes` |
| Delete deployed keyboxes | `deployer remove --yes` |

Uninstalling a module keeps its data root (`/data/misc/keystore/rkp-mini`, `/data/adb/rkp_sync`).

None of these is revocation: receivers keep what they fetched, and old Gist revisions still
decrypt with the old passphrase. To revoke, run `sync rotate-password` **and** issue a new
keybox. Treat a lost device or a root compromise on either side as a passphrase and key leak.

## Migrating from the old single-module layout

The old `rkp_mini` (sender) and `rkp_sync` (receiver) modules use the same data roots, and
the new installers refuse to run while the old module of the same role is present.

1. Back up `/data/adb/rkp_sync`: the old receiver's uninstaller deletes it.
2. Disable and uninstall the old module.
3. Restore the backup if needed, then install the new module.
