# Repository Guidelines

## Project Overview

RKP Mini is a Rust CLI suite shipped as two KernelSU modules, developed against a single Qualcomm reference device (arm64); other devices and firmware are untested, and no successful on-device issuance is recorded. It uses Qualcomm KeyMint Native V2 (`0x3122`) to obtain RKP certificates for a fresh userspace P-256 key and export a TrickyStore-compatible keybox.

Keep the original scope: Native V2 only; report unsupported firmware rather than adding V1/AIDL fallbacks. No TEE/boot-state spoofing. Issuance does not promise Play Integrity success.

Three commands, no shared `main` and no cross-invocation:

|Binary|Scope|
|---|---|
|`rkp-mini-issuer`|`status`, `issue` (with `--if-expiring-days N`), `config init\|show`, internal `__ta-worker`|
|`rkp-mini-sync`|`init`, `risk`, `enable`, `disable`, `forget`, `secret set`, `config show\|set`, `status`, `check-auto [--require-verified]`, `push`, `share`, `rotate-password`, `import`, `fetch`|
|`rkp-mini-deployer`|`plan`, `apply`, `add-target`, `remove-target`, `remove` (local only)|

The commands never call each other and never link each other; users combine them in their shell, and the module scripts combine them for the WebUI/Action/loop. Old names (`rkp-mini`, `rkp-sync`, the `sync-now` alias, `run`, and the `deploy …` subcommands) are gone: they are usage errors, not aliases.

Two modules: `rkp_mini_issuer` (`modules/issuer`, ships issuer + sync + issuer page) and `rkp_mini_deployer` (`modules/deployer`, ships sync + deployer + combo runner + deployer page). Each installer aborts when the same-role legacy module (`rkp_mini` / `rkp_sync`) is still installed; data roots and paths are unchanged, so a cutover keeps credentials and anti-replay state. The legacy `rkp_sync` uninstaller deletes `/data/adb/rkp_sync`, so the manual cutover backs that root up first.

Remote private-key deployment is opt-in and disabled by default. It encrypts the keybox to user-controlled storage (Gist/WebDAV/public HTTPS) with an age passphrase. Enabling it requires explicit risk acceptance. It does not guarantee Play Integrity, and using a keybox on a different device is keybox cloning / spoofing, not a legitimate RKP provisioning path. The settled design — decisions, threat model, formats, acceptance list — is `docs/REMOTE_SYNC_DEVELOP.md`; read it before touching `rkp-mini-sync` and do not re-litigate its decisions. Unattended loops run only while sync is enabled; `auto_issue_days` is a remaining-days threshold used by the sender loop, and `rkp-mini-issuer` itself never schedules.

The module ships a KernelSU WebUI (`webui/` → `webroot/`) as a thin shell over the CLIs' `--json` contracts: it reads status, starts issuance and shows results, logs and read-only issuer config, and manages remote sync and local deployment. Neither page reads or renders keybox/private-key contents or stored secrets, edits the issuer's own config keys, or adds protocol paths; sync settings change only through `rkp-mini-sync config set` (allow-list), target edits only through `rkp-mini-deployer add-target|remove-target`. The Action button stays as the second entry point.

For protocol or scope changes, consult the original external design note (kept outside this repository). Treat current source as the implementation reference; the design's proposed layout and acceptance checklist are not evidence of completed device validation.

## Architecture & Data Flow

Cargo workspace with six crates and a strictly acyclic dependency graph:

```text
rkp-mini-issuer            (binary, self-contained: no rkp-mini-* dependency)
rkp-mini-core              ← rkp-mini-runtime
rkp-mini-core              ← rkp-mini-sync-core
rkp-mini-core + runtime + sync-core ← rkp-mini-sync      (binary)
rkp-mini-core + runtime    ← rkp-mini-deployer           (binary)
```

- `crates/rkp-mini-core`: local, network-free foundation — `error` (stage/code/exit model), `fsutil` (atomic 0600 writes), `keybox` (strict XML validation, no DTD), `locator` (`rkpsync://v1/import` + URL policy), `state` (`sync-state.json`, `deploy-state.json`, `staging/latest.json`), `confirmation` (`sha_confirms`/`confirmation_mismatch`), and the `PROTOCOL_VERSION`/`REMOTE_FILENAME`/limits/`sha256`/`valid_stream_id`/`DAY` constants. Real P-256 fixtures live here behind the non-default `test-support` feature. It must not depend on `age`, an HTTP client or vendor ABI.
- `crates/rkp-mini-sync-core`: `envelope` (age scrypt + armor), `manifest` (canonical CBOR), `protocol` (push base selection, receiver replay/rollback), `secret` (0600 secret/inbox files), `transport`; re-exports `age::secrecy`.
- `crates/rkp-mini-runtime`: `config` (TOML, roles, templates, comment-preserving edits, target policies), `targets` (builtin/custom target parsing only), `context` (`Global`/`Ctx`), `output` (`Output`, one envelope per command, exit mapping), `lock` (the shared non-blocking data lock), `cli` (`Flags`, `sha_arg`), `tty` (plain prompts), `util` (clock, UTC date, loop-pid check). No network, no `age`, no secret reading.
- `crates/rkp-mini-issuer`: `src/rkp/` orchestrates issuance (acknowledge → config → prerequisites → key → challenge → request → TA worker → CSR → certificates → chain → artifacts → description). `protocol.rs` owns CBOR/COSE and the five TA frame layouts: preserve challenge/public-key binding, request limits, and field `57` as a byte string containing a CBOR key array. `keymint.rs` isolates vendor ABI calls by re-executing the binary (`RKP1` envelope, parent timeout, silenced child); keep unsafe vendor calls behind that boundary. `server.rs` does blocking CBOR HTTP via `ureq`; `chain.rs` checks DER structure/linkage/leaf key/validity; `keybox.rs` builds/inspects XML. Local validation does **not** cryptographically verify CSR or certificate signatures. `status.rs` is local-only and can rewrite `module.prop`.
- `crates/rkp-mini-sync`: `app` (HTTP client, age passphrase, provider credentials, sync-state bookkeeping), `http` (manual redirects, DNS-level private-address guard, 256 KiB cap), `remote` (Gist/WebDAV/read-only URL), `sender`, `receiver` (import + fetch only), `common`, `cli`, `tty` (echo-off secret reader over `runtime::tty`).
- `crates/rkp-mini-deployer`: `cli`, `commands` (plan/apply/add-target/remove-target/remove, the skip rules, sha-bound approvals), `deploy` (identity gate, atomic target write with SELinux label preservation, per-target report, reload, remove). Local only: no HTTP, no age, no sync state writes.

Failures: `rkp-mini-issuer` keeps its own error contract (stage/code/`ta_status`/`head_hex`, exit 1/2/3/4/5/6/7). `rkp-mini-sync` and `rkp-mini-deployer` share `SyncError` (sync: 0 ok, 2 usage/config/risk, 8 sync/network/decrypt/validate/state, 1 other; deployer: 0 ok, 2 usage/config/approval, 9 deploy, 1 other).

Data flow for sync: `rkp-mini-sync push` seals `output/keybox.xml` into one armored age file; `rkp-mini-sync fetch` verifies `stream_id`/`seq`/`prev`/expiry/keybox structure and writes `staging/latest.json` + the staged XML; `rkp-mini-deployer apply` re-validates the staged bytes, evaluates the identity gate, writes each locally configured target, and records `deploy-state.json`. `fetch` never reads deploy state; `apply` never reads sync state.

Module scripts:

- `modules/issuer`: `customize.sh` (legacy-module gate, arm64 gate, permissions, config init, non-fatal status), `service.sh` (status refresh; starts `sync-loop.sh` only when sender `check-auto` passes), `action.sh` (foreground `issue --acknowledge`), `sync-loop.sh` (12 h: optional `issue --if-expiring-days`, then `push --auto`), `uninstall.sh` (keeps data), `webroot/runner/issue.sh`, `webroot/runner/sync.sh` (sender `push`/`rotate` only).
- `modules/deployer`: `customize.sh`, `service.sh`, `action.sh` (`exec sh run.sh action`), `sync-loop.sh` (8 h: `sh run.sh auto`, then logs each step from the auto sidecars), `uninstall.sh` (keeps `/data/adb/rkp_sync` and deployed keyboxes), `webroot/runner/sync.sh` (receiver `fetch [sha]`/`run`), and the combo runner:
  - `run.sh` fixes the binaries/data root and accepts only `action` | `run` | `auto`.
  - `lib/run.sh` exposes `run_receiver_steps MODE SYNC_BIN DEPLOY_BIN CONFIG RESULT_BASE` and `publish_run_result RESULT_BASE OVERALL`.
  - `action` runs fetch then `apply --if-staged` in the foreground (stdio inherited, prompts intact) and still runs apply after a failed fetch; `run` (WebUI) does the same with `--json`; `auto` runs `check-auto --require-verified` first and stops on failure, then `fetch --auto` and `apply --auto --if-staged`.
  - Result files in `<data root>/tmp`: `<base>.{preflight,fetch,deploy}.{json,err,code}`, `<base>.last.op`, `<base>.last.err`, `<base>.last.steps`, `<base>.last.code` (`<base>` = `sync-task` for WebUI, `sync-auto` for the loop; action writes none). Old results are deleted in a fixed order before the binary check; `last.steps` is an eight-section snapshot separated by `<<<RKP-SEP>>>` (overall, fetch code/JSON/stderr, deploy code/JSON/stderr, orchestration stderr, tails capped at 16 KiB), renamed into place before `last.code` is written. Shell never parses or synthesises JSON, uses no jq/python, and never falls back to a binary on `PATH`.

WebUI (`webui/src/`): one Bun package, two explicit apps. `entry.ts` resolves the app from `--mode issuer|deployer` only. `src/apps/issuer/` (issuer page + sender sync section) and `src/apps/deployer/` (deployer page) each have `main.ts`, `App.vue`, `components/`, `lib/`, `dev/` and a `config.ts` exporting `appConfig` (`moduleId`, `moduleDir`, `dataRoot`, `syncRole`). `src/shared/` holds `components/` (FoldBlock, StatusPill, RiskSheet, SecretSheet, SecretInput), `lib/` (`command.ts` errors/paths/JSON-line parsing, `ksu.ts` bridge, `runner.ts` `useTaskRunner`, `sync.ts` types + management commands, format/theme/debug/clipboard), and `styles/app.css`. Aliases: `@app` → the mode's `main.ts`, `@app-config` → its `config.ts`, `@app-mock` → its `dev/mock.ts`; `ksu.ts` imports the mock only inside `import.meta.env.DEV`. Deploy execution (`plan`/`apply`/`add-target`/`remove-target`/`remove`) lives in `apps/deployer/lib/deploy.ts` and calls `bin/rkp-mini-deployer --role receiver --config … --json`; `apps/deployer/lib/run.ts` parses `sync-task.last.steps` (requires `last.op == run`, exactly 8 sections, per-section JSON validated, empty code+envelope = not executed). Vite ships no shell: `publicDir: false`, no runner plugins, `base: './'`, ES2020, production CSP with `connect-src 'none'`.

WebUI secrets (design §19.2): `ksu.exec` sends the command over the root shell's stdin, so only external commands' argv are visible. `shared/lib/sync.ts` hex-encodes a secret and writes it with builtins (`umask 077; echo <hex> > <inbox>/<kind>.hex`), then `rkp-mini-sync secret set … --from-file … --hex` consumes and deletes it. Never pass secrets as arguments, env, here-docs or localStorage. Debug builds of the KernelSU manager log shell commands; that residual risk is a risk-statement item. Approvals are bound to what was shown: `enable --accept-risk <version>`, `fetch --accept-first <plaintext sha>`, `apply --confirm-first/--allow-identity-change <keybox sha>`.

## Key Directories

|Path|Purpose|
|---|---|
|`crates/rkp-mini-issuer/src/`, `…/src/rkp/`|Issuer CLI/config/status and the issuance pipeline|
|`crates/rkp-mini-core/`, `crates/rkp-mini-runtime/`|Network-free foundation and shared local runtime|
|`crates/rkp-mini-sync-core/`, `crates/rkp-mini-sync/`|Sync library (age/CBOR/protocol/transport) and sync CLI|
|`crates/rkp-mini-deployer/`|Local deployment CLI (identity gate + atomic target writes)|
|`modules/issuer/`|Issuer module: hooks, loop and the issuer WebUI runner|
|`modules/deployer/`|Deployer module: hooks, loop, `run.sh`+`lib/run.sh`, deployer runner; never the issuer|
|`dist/module-issuer/`, `dist/module-deployer/`, `dist/*.zip`|Generated staging trees and installable ZIPs; edit `modules*/` and `webui/`, not staged copies|
|`webui/`|Vue 3 + `miuix-vue` source for both pages; `dist/issuer/`, `dist/deployer/` outputs|
|`target/`|Generated host and Android build outputs|

Tests live beside implementation, not in a `tests/` directory.

## Development Commands

Run from the repository root:

```sh
cargo build --locked                  # Host build (workspace)
cargo run -p rkp-mini-issuer -- --help        # Safe CLI smoke check
cargo run -p rkp-mini-issuer -- config show   # Effective configuration
cargo run -p rkp-mini-sync -- --help          # Sync CLI smoke check
cargo run -p rkp-mini-deployer -- --help      # Deployer CLI smoke check
cargo test --locked                   # Host unit tests (all crates)
cargo test --locked -p rkp-mini-sync-core protocol::tests   # Focused protocol tests
cargo fmt --check                     # Standard formatting check
cargo clippy --locked -p rkp-mini-core -p rkp-mini-runtime -p rkp-mini-sync-core \
  -p rkp-mini-sync -p rkp-mini-deployer --all-targets -- -D warnings
rustup target add aarch64-linux-android
NDK=/path/to/android-ndk ./build.sh   # Android release build + WebUI + KernelSU ZIPs
cd webui && bun install --frozen-lockfile && bun run build   # vue-tsc + both vite builds
cd webui && bun run dev:issuer          # Issuer preview against fixtures
cd webui && bun run dev:deployer        # Deployer preview against fixtures
```

Issuer dev scenarios: `?mock=ready|expiring|expired|invalid|missing|blocked|status-exit|status-parse|status-timeout`, `?runner=idle|running|overdue|success|failure|crashed|stale`, `?outcome=success|failure`; sender sync scenarios are described in `apps/issuer/dev/mockSyncSender.ts`. Deployer scenarios: `apps/deployer/dev/mockReceiver.ts` (including the combined-run sidecar scenarios).

`build.sh` configures the NDK linker and `ring` C compiler variables, targets Android API 26, builds `rkp-mini-issuer`, `rkp-mini-sync` and `rkp-mini-deployer` with the committed lockfile, builds both WebUI pages with Bun (frozen lockfile), copies `modules/issuer` + `webui/dist/issuer` into `dist/module-issuer` and `modules/deployer` + `webui/dist/deployer` into `dist/module-deployer`, and checks the ELFs/ZIPs with `file`, `readelf` and `unzip`. Outputs: `dist/rkp_mini_issuer-v<issuer version>.zip` (root: `module.prop`, hooks, `sync-loop.sh`, `bin/rkp-mini-issuer`, `bin/rkp-mini-sync`, `webroot/index.html`, `webroot/runner/{issue,sync}.sh`) and `dist/rkp_mini_deployer-v<deployer version>.zip` (root: `module.prop`, hooks, `sync-loop.sh`, `run.sh`, `lib/run.sh`, `bin/rkp-mini-sync`, `bin/rkp-mini-deployer`, `webroot/index.html`, `webroot/runner/sync.sh`). No wrapper directory; the script fails when an entry is missing or extra, when the two ZIPs ship different `rkp-mini-sync` binaries, or when `module.prop` id/version disagrees with the owning crate.

On a rooted target after installation:

```sh
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer status --json'
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge --json'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer --role receiver --json plan'
```

The second command performs real network/TA issuance and creates new credentials; use it only for an intentional device test. Host `status` cannot satisfy the compile-time Android arm64 prerequisite and still attempts status-file writes.

## Code Conventions & Common Patterns

- Use standard Rust formatting, `snake_case` functions/modules, `UpperCamelCase` types and `SCREAMING_SNAKE_CASE` constants. Keep module documentation and `SAFETY` explanations around unsafe operations.
- Keep execution synchronous and command-scoped. Pass `&Config`, paths, keys and byte slices explicitly; pure encoders/parsers are separate from HTTP, process and filesystem boundaries. There is no async runtime, DI container or shared service state.
- Keep the dependency direction: the issuer depends on no `rkp-mini-*` crate, the deployer on `core`+`runtime` only, and `core`/`runtime` on neither `age`, `ureq` nor `libc`-ABI vendor code. Verify with `cargo tree -p rkp-mini-deployer -e normal`.
- Return `anyhow::Result` internally; use `error::err(Stage, code, message)` / `AppError` for stage-specific failures. Preserve machine-readable codes, raw `ta_status` and diagnostic `head_hex`. `rkp-mini-sync` and `rkp-mini-deployer` reuse `runtime::output::{print_error, emit}` so each `--json` command prints exactly one envelope and the `main` owns `process::exit`.
- Configuration precedence is CLI > TOML > defaults. Only `issue` accepts server/output/fingerprint overrides; TOML is validated before overrides. Missing files use defaults; unknown keys warn. Persist custom `output_dir` in TOML if subsequent `status` must inspect it. The deployer accepts only `--config`, `--role receiver` and `--json`; a sender role is `wrong_role`.
- Use Unix seconds and UTC. Reuse `fsutil::atomic_write` and `ensure_dir_0700`; enforce `0600` credential/data files and `0700` private directories. Installer `mkdir -p` does not enforce `0700` on output/log directories before first use. Keys use SEC1 `EC PRIVATE KEY`, not PKCS#8; certificates are leaf→root.
- Preserve the old keybox on failure. Current writes are individually atomic, **not** a multi-file transaction: chain/key/meta files precede `keybox.xml`, with a best-effort `keybox.xml.prev` backup. Do not describe the whole output set as transactional or clear the output directory before issuance. Staging and target writes are separate steps: the combination is never a transaction, and `apply` re-validates the staged bytes and the approval digest itself.
- `apply --if-staged` skips **only** when `staging/latest.json` is genuinely absent (a stat error other than `NotFound` is reported, and a corrupt file keeps its original error); `--auto` returns a successful no-op when `[deploy].auto_deploy` is false and takes precedence over `--if-staged`. Neither flag approves anything: enabled/risk/stream checks run before both, and an identity change still needs `--allow-identity-change <sha>`.
- Treat private keys and keyboxes as credentials: never print, log, upload or commit them. Avoid logging complete CSRs. Keep fresh key/challenge generation for every issuance attempt.
- Sync secrets (age passphrase, Gist token, WebDAV password) enter only via TTY (echo off), stdin or `0600` secret files; there is no CLI flag that takes one, and TOML holds paths only. URLs, QR codes, logs, errors, argv and env carry location/seq/hash/code only.
- Deployment targets, owners, modes and reload actions come only from the receiver's local config; nothing from the remote side may select a path or command. Deploy failures keep the old target file (temp + fsync + rename + dir fsync, read-back hash).
- Shell composed flows must stay JSON-free: no parsing, no synthesis, no jq/python, no `fetch && apply`. Result sidecars are written atomically, and the completion code is published last.
- Keep the three binaries non-recursive: the deployer never invokes sync, and no command shells out to another RKP-Mini binary. The issuer's `__ta-worker` re-executes itself only.

## Important Files

- `crates/rkp-mini-issuer/src/config.rs`: defaults, validation and commented config template. Default config is `/data/misc/keystore/rkp-mini/config.toml`; default output is its sibling `output/`; logs, worker temp files and `status.{json,txt}` stay under the fixed data root even with a custom output directory.
- `crates/rkp-mini-runtime/src/config.rs`: the sync/deploy TOML (roles, sender/receiver templates, `[sync]`/`[deploy]`, secret paths, `edit_table`/`add_target`/`remove_target`, `lock_path`). The sender config is the shared `rkp-mini` file (`output_dir`, `auto_issue_days`, `[sync]`); the receiver root is `/data/adb/rkp_sync/` (`sync/`, `deploy/`, `staging/`, `secrets/`).
- `crates/rkp-mini-runtime/src/targets.rs`: builtin/custom target resolution (paths, `uid:gid`, modes, reload specs, duplicate detection). `crates/rkp-mini-deployer/src/deploy.rs`: identity gate, atomic writes, SELinux label copy, TEESim profile check, reload, remove. `…/src/commands.rs`: plan/apply/add-target/remove-target/remove plus the skip rules.
- `crates/rkp-mini-core/src/{confirmation,state,keybox}.rs`: the approval digest rules, the three state files (no target/path fields in `sync-state.json`) and strict keybox validation shared by fetch and apply.
- `Cargo.toml` (workspace + release profile; dev-profile `opt-level = 3` for scrypt/ChaCha so host tests stay fast), `crates/*/Cargo.toml`, `Cargo.lock`, `build.sh`: dependencies and cross-build/package behavior. Keep each binary crate version and its module's `module.prop` version/versionCode synchronized (`rkp-mini-issuer` ↔ `modules/issuer/`, `rkp-mini-deployer` ↔ `modules/deployer/`); `rkp-mini-sync` has its own `--version` and is not a module version source.
- `modules/deployer/lib/run.sh`: the only orchestration layer; the sidecar/publish order and the exit-code priority (deploy, then fetch, then 0) live here.
- Installer permission caveat: `customize.sh` resets files to `0644` and restores execution only for the binaries and listed hooks; add new hooks there and verify uninstall invocation when changing hook permissions. `lib/run.sh` stays `0644` (it is sourced).
- `README.md`: installation and firmware failure guidance. Its bare design-document link is external, and its stated default NDK version is not enforced by `build.sh`.
- `webui/src/shared/lib/sync.ts` mirrors `rkp-mini-sync`'s JSON contract; `apps/deployer/lib/deploy.ts` mirrors `rkp-mini-deployer`'s; `apps/deployer/lib/run.ts` mirrors the combo sidecar snapshot. Changing `--json` output, codes, stages or sidecars requires updating them and the dev mocks/fixtures, then `bun run build`.

## Runtime/Tooling Preferences

Use Cargo/Rust 2021 and the committed lockfile. No Rust toolchain/MSRV pin is declared. Android packaging assumes an x86_64 Linux host, Bash, Android NDK, Bun, `zip`, `unzip`, `file` and `readelf`. Set `NDK` explicitly for reproducibility; otherwise the script selects the last matching directory under `~/Android/Sdk/ndk/`, not a pinned version.

WebUI stack: Vue 3 + TypeScript + Vite, Bun for install/scripts, committed `webui/bun.lock`. `miuix-vue` is pinned to exactly `0.1.1`; its published type entry is broken, so `tsconfig.json` maps `miuix-vue` and `miuix-vue/icons` via `paths` (TypeScript 6, no `baseUrl`). Use Miuix tokens from `miuix-vue/style.css`; app-owned tokens (`--app-warning`, `--app-sheet-group`) live in `src/shared/styles/app.css`. KernelSU's WebView is transparent and has no SPA history (back closes the WebUI), so each page paints its own background, uses one page plus bottom sheets with explicit close buttons, and loads nothing remote (production CSP sets `connect-src 'none'`). Shell commands only interpolate constant paths and log names matching `issue-<digits>.log`.

Keep the small synchronous stack: hand-written CLI parsing, `ureq` with Rust TLS, `ciborium`, `p256` and `x509-cert`. Do not introduce `clap`, an async runtime, `reqwest`, or a runtime OpenSSL executable dependency. Module hooks use `/system/bin/sh`, not Bash.

Device prerequisites include root, Qualcomm hardware, `/vendor/lib64/libkeymasterdeviceutils.so`, `/dev/smcinvoke` and a fingerprint. Report SELinux/path failures rather than silently relocating data.

## Testing & QA

Use Rust's built-in `#[test]` framework in inline `#[cfg(test)] mod tests`. Reuse the shared fixtures: `rkp-mini-core`'s `test_support` (behind the `test-support` feature) provides `TestKeybox`; `rkp-mini-issuer` keeps its own `test_support.rs`; use `tempfile` for isolated files. Keep host tests offline and independent of root/vendor libraries. Sync tests run the full push/fetch/replay protocol over `DirTransport` and HTTP providers against in-process `TcpListener` servers with `--allow-insecure-http/--allow-private` policy; nothing contacts GitHub or a real WebDAV server.

Existing coverage includes config/templates, frame/CSR parsing and binding, server response parsing/HTTP error mapping, chain/keybox handling, worker reply framing, description states, protocol replay rules, secret/permission handling, target resolution, the deploy identity gate and per-target failures, the `apply --if-staged`/`--auto` skip and error precedence, stale sha approvals, the shared lock, and `check-auto --require-verified`.

No coverage percentage, coverage tooling or CI workflow is configured. Add behavioral regression coverage for changed boundaries and failure paths; parser tests do not establish end-to-end HTTP, worker timeout, output rollback or installed-module correctness.

Reference-device firmware acceptance remains a device-only go/no-go check; the repository contains no recorded successful device result. Record actual KeyMint version, TA status and issuance outcome when tested. Host tests and a valid AArch64 ZIP cannot substitute for this. Diagnose `-18`, `-40`, `-100` and HTTP `444` explicitly rather than changing protocols. Device QA must check JSON/errors, key-to-leaf binding, leaf→root ordering, `0700`/`0600` permissions, old-keybox preservation, Action exit status and boot refresh without issuance. Release uses `panic = "abort"`; worker `catch_unwind` is not release panic recovery.

Sync device QA (not yet recorded): real Gist create/update and token-less raw read, a real WebDAV provider (e.g. Jianguoyun), OMK watcher reload with owner `1017:1017`, TEESimulator `config.json` reference and EC-only acceptance, SELinux labels on deployed files, loops after reboot and under Doze, and attestation behaviour on the receiving device.

Combination-script QA: the Action path keeps its final exit code (deploy failures win over fetch failures), a failed fetch still repairs a damaged target from already-verified staging, `auto` stops before any network access when `check-auto --require-verified` fails, `auto_deploy = false` changes nothing, and a missing binary yields exit 2 with a diagnostic in `last.err` instead of an eternal spinner.

The WebUI has no automated tests: `bun run build` (vue-tsc) plus the dev-mock scenarios cover host checks. Device-only WebUI QA: the Open button renders without a transparent/white background, status matches `status --json`, blockers disable issuance, issuance survives closing and reopening the WebUI, a double tap launches one task, and failures show stage/code/`ta_status` with the old keybox intact. Deployer page additionally: the combined run shows the fetch and deploy halves separately (including a failed fetch with a successful repair), a missing step envelope reads as an abnormal exit rather than success, TOFU/deploy dialogs show the facts before any write, `auto` never overwrites the last WebUI result, secrets entered in the page land as 0600 files with no `inbox/*.hex` left behind and never appear in `ps`/logcat (release manager), risk items start unchecked, and closing the page does not interrupt push/fetch/run tasks.
