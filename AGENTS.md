# Repository Guidelines

## Project Overview

RKP-Mini is a Rust CLI suite shipped as two KernelSU modules, developed against a single Qualcomm reference device (arm64); other devices and firmware are untested, and no successful on-device issuance is recorded. It uses Qualcomm KeyMint Native V2 (`0x3122`) to obtain RKP certificates for a fresh userspace P-256 key and export a TrickyStore-compatible keybox; an opt-in sync layer moves that keybox, encrypted, to another device.

Scope is fixed: Native V2 only (report unsupported firmware instead of adding V1/AIDL fallbacks), no TEE/boot-state spoofing, no promise of Play Integrity success.

Three independent commands. They never link or invoke each other; module scripts and users compose them.

|Binary|Commands|
|---|---|
|`rkp-mini-issuer`|`status`, `issue` (`--if-expiring-days N`), `config init\|show`, internal `__ta-worker`|
|`rkp-mini-sync`|`init`, `risk`, `enable`, `disable`, `forget`, `secret set`, `config show\|set`, `status`, `check-auto [--require-verified]`, `push`, `share`, `rotate-password`, `import`, `fetch`|
|`rkp-mini-deployer`|`plan`, `apply`, `targets`, `add-target`, `remove-target`, `remove` (local only)|

Removed names are usage errors (exit 2), not aliases: binaries `rkp-mini`/`rkp-sync`, `sync-now`, `run`, and every `deploy …` subcommand.

Two modules: `rkp_mini_issuer` (`modules/issuer`: issuer + sync + issuer page) and `rkp_mini_deployer` (`modules/deployer`: sync + deployer + combo runner + deployer page). Data identifiers deliberately keep their old spelling: sender root `/data/misc/keystore/rkp-mini`, receiver root `/data/adb/rkp_sync`, remote file `rkp-sync.age`, `rkpsync://v1/import`, DeviceID prefix `rkp-mini-`, TOML/JSON roles `sender`/`receiver`. Rename commands, never these.

Remote private-key deployment is opt-in, disabled by default, and gated on explicit risk acceptance. Using a keybox on another device is keybox cloning/spoofing, not legitimate RKP provisioning. Unattended loops run only while sync is enabled; `rkp-mini-issuer` itself never schedules.

Design records — read before changing the area, and treat their decisions as settled:

- `docs/REMOTE_SYNC_DEVELOP.md`: sync protocol, threat model, formats, risk statement (§13), WebUI secrets/approvals (§19). Required before touching `rkp-mini-sync*` or `rkp-mini-deployer`.
- `docs/WEBUI_DEVELOP.md`: WebUI design record and KernelSU WebView facts. Required before touching `webui/`.
- The original external design note (outside the repo): issuance protocol/scope. Its proposed layout and acceptance checklist are not evidence of device validation; current source is the implementation reference.

## Architecture & Data Flow

Six crates, acyclic:

```text
rkp-mini-issuer   (bin)  no rkp-mini-* dependency; the only crate with vendor ABI (libloading)
rkp-mini-core     (lib)  no age, no ureq, no libc
rkp-mini-runtime  (lib)  ← core            (libc for flock/isatty/kill/getuid)
rkp-mini-sync-core(lib)  ← core            (age, ciborium)
rkp-mini-sync     (bin)  ← core + runtime + sync-core   (ureq, qrcode)
rkp-mini-deployer (bin)  ← core + runtime  (no HTTP, no age)
```

- `rkp-mini-core`: `error` (`Stage`, `SyncError`, `Stage::exit_code`), `fsutil` (`atomic_write`, `ensure_private_dir`), `keybox` (strict XML validation, no DTD), `locator` (`rkpsync://` + URL/SSRF policy), `state` (`sync-state.json`, `deploy-state.json`, `staging/latest.json`), `confirmation` (`sha_confirms`, `confirmation_mismatch`), protocol constants. `test_support` (`TestKeybox`) sits behind the non-default `test-support` feature.
- `rkp-mini-sync-core`: `envelope` (age scrypt + armor), `manifest` (canonical CBOR; decode re-encodes and requires byte equality), `protocol` (push base selection, receiver seq/prev/rollback rules over a `Transport`), `secret` (0600 secret/inbox files), `transport`.
- `rkp-mini-runtime`: `config` (TOML, roles, templates, comment-preserving `edit_table`/`add_target`/`remove_target`, `lock_path`), `targets` (`BUILTINS` table, `resolve_targets`: parsing only, no I/O on targets), `context` (`Global`, `Ctx`), `output` (`Output`, `print_error`, `emit` returns the exit code), `lock` (shared non-blocking `flock`), `cli` (`Flags`, `sha_arg`), `tty` (plain prompts), `util`.
- `rkp-mini-issuer` (`src/rkp/mod.rs`): acknowledge → config → prerequisites → key → challenge → request → TA worker → CSR → certificates → chain → artifacts → module description. `protocol.rs` owns CBOR/COSE and the five TA frame layouts: preserve challenge/public-key binding, request limits, and field `57` as a byte string containing a CBOR key array. `keymint.rs` re-executes the binary as `__ta-worker` (`RKP1` envelope via `request.bin`/`reply.bin`, parent timeout, silenced child); unsafe vendor calls stay behind that boundary. `server.rs` is blocking CBOR over `ureq`; `chain.rs` checks DER structure/linkage/leaf key/validity. Nothing verifies CSR or certificate signatures cryptographically. `status.rs` is local-only (no network, no TA).
- `rkp-mini-sync`: `app` (HTTP client, passphrase, credentials, sync-state error recording), `http` (manual same-host redirects, DNS-level private-address guard, 256 KiB cap), `remote` (Gist API/raw, WebDAV PUT tmp+MOVE, read-only URL), `sender`, `receiver` (import + fetch only), `common` (init/risk/enable/forget/config/status/check-auto), `tty` (echo-off secret reader).
- `rkp-mini-deployer`: `commands` (plan/apply/targets/add-target/remove-target/remove, skip rules, sha-bound approvals), `deploy` (identity gate, temp + fsync + rename + dir fsync write preserving the SELinux label, read-back hash, reload, remove), `omk` (pre-write mirror of OhMyKeymint's regex keybox scanner; `omk_incompatible` skips that target), `modules` (read-only detection of the root module behind each builtin target: `/data/adb/modules/<id>/module.prop`, `disable`/`remove` markers; fails closed).

Data flow: `sync push` seals `output/keybox.xml` into one armored age file → `sync fetch` verifies `stream_id`/`seq`/`prev`/expiry/keybox structure and writes `staging/latest.json` + `staging/<sha>.xml` → `deployer apply` re-validates the staged bytes, evaluates the identity gate, writes each locally configured target, records `deploy-state.json`. `fetch` never reads deploy state; `plan`/`apply`/`remove` never read sync state; sync `status` reads both without locking. Mutating commands (`push`, `rotate-password`, `import`, `fetch`, `forget`, `apply`, `remove`, `add-target`, `remove-target`) take the non-blocking `<root>/sync/sync.lock`; contention is `locked` (exit 8), never a wait.

Exit contracts (`--json` prints exactly one envelope; only each `main` calls `process::exit`):

|Binary|Codes|
|---|---|
|issuer (own `error.rs`)|1 generic, 2 config/usage, 3 prerequisites, 4 HTTP, 5 TA/CSR, 6 chain/keybox, 7 writes. `status` always exits 0 (blockers are data). `--json` is honoured only by `status` and `issue`|
|sync|0, 2 config/usage/risk/`wrong_role`, 8 remote/decrypt/validate/state, 1 other|
|deployer|0 (including `data.skipped` and `data.pending`), 2 config/usage/`wrong_role`/missing `--yes`, 8 state (`nothing_staged`, `staging_corrupt`, `locked`, `confirmation_mismatch`), 9 a target failed (envelope carries `data`), 1 other|

Module scripts (`/system/bin/sh`):

- Both modules: `customize.sh` and `service.sh` stop when the legacy same-role module (`rkp_mini` / `rkp_sync`) exists under `/data/adb/modules` or `modules_update`. `service.sh` starts `sync-loop.sh` only when `rkp-mini-sync check-auto` passes. `uninstall.sh` keeps the data root. Loops hold a pid file (`<root>/tmp/sync-loop.pid`) and log to `<root>/logs/sync.log` (256 KiB, one `.1` generation).
- Issuer: `action.sh` = foreground `issue --acknowledge`; loop ≈12 h: optional `issue --if-expiring-days <auto_issue_days>`, then `push --auto`.
- Deployer: `action.sh` = `exec sh run.sh action`; loop ≈8 h: `sh run.sh auto`, then appends each step sidecar to the log.
- `webroot/runner/{issue,sync}.sh` are the WebUI's background tasks: `mkdir` lock `<root>/tmp/<name>.lock` (contention → exit 9), outputs `<name>.last.{json,err,op}`, `<name>.last.code` written last. Issuer `sync.sh` accepts `push`/`rotate`; deployer `sync.sh` accepts `fetch [sha]`/`run`. Roles are fixed per script.

Combo runner (`modules/deployer/run.sh` + sourced `lib/run.sh`), the only orchestration layer:

- `run.sh action|run|auto`; anything else exits 2. Binaries come from the module directory only.
- `action`: foreground fetch, then `apply --if-staged` even when fetch failed; stdio inherited, no sidecars. `run` (WebUI): same with `--json`. `auto`: `check-auto --require-verified` first, stop on failure; then `fetch --auto`, `apply --auto --if-staged`.
- Exit priority: preflight failure, else deploy non-zero, else fetch non-zero, else 0.
- Sidecars under `<root>/tmp/<base>` (`sync-task` for the WebUI, `sync-auto` for the loop): `.preflight|.fetch|.deploy.{json,err,code}`, `.last.{op,err,steps,code}`. Old results are deleted in a fixed order before the binary check. `publish_run_result` writes `last.steps` (eight sections split by `<<<RKP-SEP>>>`: overall, fetch code/JSON/stderr, deploy code/JSON/stderr, orchestration stderr; stderr tails capped at 16 KiB; no preflight section), renames it into place, then writes `last.code`. A failed publish exits 1 with no `last.code`: readers treat a missing code as "unknown".

WebUI (`webui/`, one Bun package, two apps):

- `vite.config.ts` accepts only `--mode issuer|deployer` (`resolveApp` throws otherwise, so bare `vite`, `vite build` and `bun run preview` fail), exposes it as `virtual:app-mode`, and aliases `@app` → `src/apps/<app>/main.ts`, `@app-config` → `config.ts`, `@app-mock` → `dev/mock.ts`. Output `dist/<app>/`. `publicDir: false`: Vite ships no shell; runners live in `modules/*/webroot/runner`.
- `src/apps/issuer` (issue page + sender sync section) and `src/apps/deployer` each own `App.vue`, `components/`, `lib/`, `dev/`, and `config.ts` (`appConfig`: `moduleId`, `moduleDir`, `dataRoot`, `syncRole`). `src/shared` (`components/`, `lib/`, `styles/app.css`) never imports from `apps`; its only coupling is `@app-config` and a DEV-only dynamic `@app-mock` import in `shared/lib/ksu.ts`.
- `shared/lib/ksu.ts` wraps `ksu.exec`; its 10 s timeout stops waiting but never kills the command, so long work goes through `shared/lib/runner.ts` (`useTaskRunner`: detached `setsid`/`nohup` launch, polls the runner files above).
- Contract mirrors: `apps/issuer/lib/rkp.ts` ↔ issuer JSON; `shared/lib/sync.ts` ↔ sync JSON and management commands; `apps/deployer/lib/deploy.ts` ↔ deployer (`bin/rkp-mini-deployer --role receiver --config … --json`); `apps/deployer/lib/run.ts` ↔ `sync-task.last.steps` (requires `last.op == run`, exactly 8 sections, validates each envelope; empty code + envelope = step not executed).
- The deployer page branches on `result.op === 'run'` before touching an envelope: a combined run publishes none.

Secrets from the WebUI (design §19.2): `ksu.exec` pipes the command to the root shell's stdin, so only external commands' argv are visible. `shared/lib/sync.ts` hex-encodes the secret and writes it with builtins (`umask 077; mkdir -p <inbox>; echo <hex> > <inbox>/<kind>.hex`), then `rkp-mini-sync secret set <kind> --from-file … --hex` consumes and deletes it.

## Key Directories

|Path|Purpose|
|---|---|
|`crates/rkp-mini-issuer/src/`, `…/src/rkp/`|Issuer CLI/config/status and the issuance pipeline|
|`crates/rkp-mini-core/`, `crates/rkp-mini-runtime/`|Network-free foundation; shared config/lock/output/targets|
|`crates/rkp-mini-sync-core/`, `crates/rkp-mini-sync/`|Sync library and CLI (both roles)|
|`crates/rkp-mini-deployer/`|Local deployment CLI|
|`modules/issuer/`, `modules/deployer/`|Module sources: hooks, loops, WebUI runners, combo runner|
|`webui/src/apps/{issuer,deployer}/`, `webui/src/shared/`|The two pages and their common layer; dev mocks and fixtures under each app's `dev/`|
|`dist/module-issuer/`, `dist/module-deployer/`, `dist/*.zip`, `webui/dist/`, `target/`|Generated; edit `modules/` and `webui/src/` instead|

## Development Commands

Run from the repository root:

```sh
cargo build --locked
cargo test --locked                                         # all crates, offline
cargo test --locked -p rkp-mini-sync-core protocol::tests   # focused example
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo run -p rkp-mini-issuer -- --help      # likewise -p rkp-mini-sync, -p rkp-mini-deployer
cargo tree --locked -p rkp-mini-deployer -e normal          # dependency-direction check
cd webui && bun install --frozen-lockfile && bun run build  # vue-tsc + both vite builds
cd webui && bun run dev:issuer                              # or dev:deployer
NDK=/path/to/android-ndk ./build.sh                         # arm64 release + both ZIPs
```

- `build.sh` builds the three binaries for `aarch64-linux-android` (API 26), runs `bun run build`, stages `modules/<role>` + `webui/dist/<role>` into `dist/module-<role>`, and zips `dist/rkp_mini_issuer-v<issuer version>.zip` and `dist/rkp_mini_deployer-v<deployer version>.zip`. It verifies AArch64 ELFs, a fixed required-entry and forbidden-entry list per ZIP, no wrapper directory, byte-identical `bin/rkp-mini-sync` in both ZIPs, and `module.prop` `id`/`version` against the owning crate. The version compare is literal: `module.prop` needs `version=v<crate version>`. `versionCode` is unchecked. Without `NDK` it picks the highest directory under `~/Android/Sdk/ndk/`.
- Dev previews run against fixtures when no KernelSU bridge exists. Scenario query parameters:

|App|Parameters|
|---|---|
|issuer (`dev/mock.ts`, `dev/mockSyncSender.ts`)|`mock=ready\|expiring\|expired\|invalid\|missing\|blocked\|status-exit\|status-parse\|status-timeout`, `runner=idle\|running\|overdue\|success\|failure\|crashed\|stale`, `outcome=success\|failure`, `sync=uninit\|disabled\|ready\|error\|nosecret`, `syncpush=ok\|noop\|fail`|
|deployer (`dev/mock.ts`)|`sync=new\|imported\|ready\|pending_first\|pending_identity\|error`, `fetch=tofu\|ok\|fail\|mismatch`, `deploy=fail\|omk_incompatible`, `run=ok\|fetch_fail\|missing_envelope\|skipped\|stale`, `modules=<comma list of omk\|teesim\|tricky_store, each optionally :disabled>\|none\|fail` (default `omk,tricky_store`)|

The deployer mock persists its runner files in `localStorage` (`rkp.mock.receiver.runner`) so a reload reproduces a finished task; clear storage for a clean scenario.

On a rooted target:

```sh
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer status --json'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer --role receiver --json plan'
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge --json'
```

The last one performs real network/TA issuance and creates new credentials: intentional device tests only. Host `status` cannot satisfy the Android arm64 prerequisite and still attempts status-file writes.

## Code Conventions & Common Patterns

- Execution is synchronous and command-scoped: pass `&Config`, paths, keys and byte slices explicitly; pure encoders/parsers stay apart from HTTP, process and filesystem boundaries. No async runtime, DI container or shared service state.
- CLI parsing is hand-written per binary (`cli.rs`). No flag takes a secret value; the sync parser rejects secret-looking flags.
- Errors: `anyhow::Result` internally. Sync/deployer raise `rkp_mini_core::error::err(Stage, code, message)`; the `Stage` decides the exit code, so pick the stage for the contract above. Issuer uses its own `error::err`/`AppError` and preserves raw `ta_status` and `head_hex`. Machine-readable codes are API: the WebUI maps them to labels.
- Atomic writes and private dirs: `rkp_mini_core::fsutil::{atomic_write, ensure_private_dir}` in sync/deployer/runtime; the issuer keeps its own `util::{atomic_write, ensure_dir_0700}`. Credential/data files are `0600`, private directories `0700`. Time is Unix seconds, UTC.
- Role resolution (`runtime::config`): explicit `--config`/`--role`, else whichever default config exists; both existing is `role_ambiguous`. `sync init` requires `--role`. A sender config rejects `[deploy]` (the file is shared with the issuer, which ignores `[sync]`). Scripts and the WebUI always pass `--role` and `--config`.
- Issuer config precedence is CLI > TOML > defaults; only `issue` takes server/output/fingerprint overrides. Persist a custom `output_dir` in TOML if later `status` calls must see it.
- Approvals bind to what was shown: `enable --accept-risk <version>`, `fetch --accept-first <plaintext sha>`, `apply --confirm-first|--allow-identity-change <keybox sha>` (16–64 lowercase hex; checked under the lock against the current staging → `confirmation_mismatch`). Composition never approves anything.
- `apply` order: lock → enabled/risk/stream → `--auto` skip when `[deploy].auto_deploy = false` → `--if-staged` skip only when `staging/latest.json` is genuinely absent (other stat errors and corrupt files keep their error) → gate. Manual `apply` ignores `auto_deploy`. A first deploy or blocked identity change without its flag returns exit 0 with `data.pending`.
- Issuer output writes are individually atomic, not one transaction: chain/key/meta precede `keybox.xml`, with a best-effort `keybox.xml.prev`. The old keybox survives a failure. Fetch and apply are likewise separate steps.
- Deployment targets, owners, modes and reload actions come only from the receiver's local config; remote input selects neither a path nor a command.
- Secrets (age passphrase, Gist token, WebDAV password) enter via TTY (echo off), stdin, a `0600` file, or the WebUI inbox file; TOML holds paths only. URLs, QR codes, logs, errors, argv and env carry location/seq/hash/code only. Private keys, keyboxes and full CSRs stay out of stdout, logs and commits. Every issuance attempt generates a fresh key and challenge.
- Shell composition stays JSON-blind: `lib/run.sh` copies envelopes verbatim, records numeric exit codes, and runs both steps unconditionally (never `fetch && apply`). POSIX `sh` only; a new hook needs a `set_perm … 0755` line in that module's `customize.sh` (it resets everything to `0644`; `lib/run.sh` stays `0644` because it is sourced).
- WebUI: one page plus bottom sheets with explicit close buttons (the WebView has no history; back closes the UI), self-painted background, nothing remote (build-only CSP with `connect-src 'none'`). Icons miuix-vue lacks are path data in `shared/lib/icons.ts`, rendered with `shared/components/SvgIcon.vue` (`MiuixIcon` cannot take them: its glyphs are flipped). Shell strings interpolate only constant paths and validated arguments. Pages never render keybox/private-key contents or stored secrets. Type-check target is ES2022 but the bundle target is ES2020: stay within ES2020 runtime APIs.
- Keep each binary crate version equal to its module's `module.prop` (`rkp-mini-issuer` ↔ `modules/issuer`, `rkp-mini-deployer` ↔ `modules/deployer`); `rkp-mini-sync` has its own `--version`.

## Important Files

- `crates/rkp-mini-issuer/src/config.rs`: issuer defaults and template. Config `/data/misc/keystore/rkp-mini/config.toml`, output `output/` beside it; logs (`logs/issue-<unix>.log`), worker temp files and `status.{json,txt}` stay under the fixed data root even with a custom output directory.
- `crates/rkp-mini-runtime/src/config.rs`: sync/deploy TOML, both role templates, `RISK_VERSION`, receiver layout under `/data/adb/rkp_sync/` (`sync/`, `deploy/`, `staging/`, `secrets/`, `tmp/`, `logs/`). Bump `RISK_VERSION` whenever a risk item changes, together with the WebUI fixtures.
- `crates/rkp-mini-runtime/src/targets.rs`: `BUILTINS` — builtin targets with fixed paths/owners and the module that reads each keybox: `omk` → module `oh_my_keymint`, `teesim` → `teesim` (JingMatrix TEESimulator; any `*.xml` directly inside `/data/adb/teesim`), `tricky_store` → `tricky_store` (TrickyStore and TEESimulator-RS share the module id and `/data/adb/tricky_store/keybox.xml`); plus `Custom`. Hot reload is each module's own inotify watcher reacting to the atomic rename; the temp name must never end in `.xml`. `targets --json` exposes the table with module detection; the deployer page offers a builtin target only when `module.installed` is true (the CLI `add-target` stays unrestricted).
- `crates/rkp-mini-deployer/src/{commands,deploy}.rs`: skip rules, approvals, identity gate, per-target report.
- `crates/rkp-mini-deployer/src/omk.rs`: hand-written mirror of OhMyKeymint's regex keybox scanner (OMK 1.2.6, `9069c6c`: `src/keybox.rs` + `common/src/crypto/ec.rs`) at a pinned revision; it must only be stricter than OMK, so re-sync it whenever OMK's scanner changes. `crates/rkp-mini-issuer/src/rkp/keybox.rs` writes the SEC1 key with the secp256r1 named-curve `parameters` because OMK refuses a key without it; `rkp-mini-core`'s `test_support` fixture mirrors that encoding.
- `crates/rkp-mini-core/src/error.rs`: stage → exit code table shared by sync and deployer.
- `modules/deployer/lib/run.sh`: step order, sidecar names, publish order, exit priority. `webui/src/apps/deployer/lib/run.ts` and `apps/deployer/dev/mock.ts` must change with it.
- `webui/vite.config.ts`, `webui/src/entry.ts`, `webui/src/env.d.ts`: mode validation, aliases and their type declarations.
- `build.sh`: packaging lists. A new module file must be added to its copy list and `check_zip` list or it will not ship.
- `README.md`: user-facing install/CLI guide; "Migrating from the old single-module layout" is the cutover note (the legacy `rkp_sync` uninstaller deletes `/data/adb/rkp_sync`, so back it up first).

Changing any `--json` field, error code, stage, exit code or sidecar means updating the matching WebUI mirror, its dev mock/fixtures, then `bun run build`.

## Runtime/Tooling Preferences

- Rust 2021, committed `Cargo.lock`, always `--locked`. No toolchain/MSRV pin, no `rustfmt.toml`/`clippy.toml`, no CI.
- Keep the small synchronous stack: `ureq` 2 with rustls, `ciborium`, `p256`, `x509-cert`, `age` (armor), `toml`/`toml_edit`, `roxmltree`. Adding `clap`, an async runtime, `reqwest` or an OpenSSL dependency is out of scope.
- Root `Cargo.toml` sets `opt-level = 3` for the scrypt/ChaCha dependency chain in the dev profile so host tests stay fast; release is `opt-level = "z"`, LTO, `panic = "abort"` (the worker's `catch_unwind` is not release panic recovery).
- WebUI: Bun for install and scripts (`webui/bun.lock`, frozen), Vue 3, Vite 8, TypeScript 6, `miuix-vue` pinned to exactly `0.1.1`. Its published type entry is broken, so `tsconfig.json` maps `miuix-vue` and `miuix-vue/icons` through `paths` without `baseUrl`. Use Miuix tokens; app-owned tokens live in `src/shared/styles/app.css`.
- Packaging host: x86_64 Linux with Bash, Android NDK, Bun, `zip`, `unzip`, `file`, `readelf`, `sha256sum`. Only NDK and Bun are pre-checked.
- Device prerequisites: root, Qualcomm hardware, `/vendor/lib64/libkeymasterdeviceutils.so`, `/dev/smcinvoke`, a fingerprint. Report SELinux/path failures; do not relocate data to work around them.

## Testing & QA

- Rust tests are inline `#[cfg(test)] mod tests`; there is no `tests/` directory. Host-only, offline, Unix-only, independent of root and vendor libraries.
- Fixtures: `rkp_mini_core::test_support::TestKeybox::new(leaf_days)` mints a fresh CA + leaf relative to the current clock; `rkp-mini-sync-core` and `rkp-mini-deployer` enable it through the `test-support` dev-dependency feature. The issuer has its own `src/test_support.rs` (`test_chain()`). Use `tempfile` for files.
- Network code is tested against in-process loopback `TcpListener` servers and `DirTransport`, with a permissive `UrlPolicy { allow_insecure_http, allow_private }` built directly in the test; nothing contacts GitHub or a real WebDAV server.
- Permission tests return early when run as root; run the suite as a normal user.
- Covered: issuer config, frames/CSR binding, server parsing and HTTP error mapping, chain/keybox (including the SEC1 named-curve key encoding OhMyKeymint requires), worker reply framing, description states; core keybox/locator/state/confirmation; protocol replay rules; secret-file permissions; config templates and target resolution; the lock primitive; deploy identity gate, per-target failure, the OhMyKeymint scanner precheck (`omk_incompatible`, deploy and repair paths), `--if-staged`/`--auto` precedence, stale approvals; builtin-target module detection; `check-auto --require-verified`.
- Not covered by any automated test: `rkp-mini-sync` command flows (`sender`, `receiver`, `app`), lock acquisition by the commands, the module shell scripts, and the WebUI. Verify those with a throwaway harness: real host binaries plus a sourced `modules/deployer/lib/run.sh` against temp configs, and `bun run build` plus the dev-mock scenarios in a browser.
- Add behavioural regression tests for changed boundaries and failure paths; skip tests that pin wording or wiring.
- Device-only, none recorded in the repository: reference-device issuance go/no-go (record KeyMint version, TA status, outcome; diagnose `-18`, `-40`, `-100`, HTTP `444` explicitly instead of changing protocols), module install/Action/boot loops, both modules side by side, the legacy cutover, real Gist/WebDAV round trips, OMK reload with owner `1017:1017`, TEESimulator `config.json` reference and EC-only acceptance, SELinux labels on deployed files, loops under Doze. Host tests and a valid AArch64 ZIP do not substitute for these; report them as unverified.
- Watcher-based reload of all four target modules (OhMyKeymint, TEESimulator, TEESimulator-RS, TrickyStore) was established by reading upstream source (`docs/REMOTE_SYNC_DEVELOP.md` §4.2–§4.3), not on a device: report it as unverified until a deployed keybox is observed to take effect without a reboot.
- Device WebUI checks: Open renders without a transparent/white background, status matches `status --json`, blockers disable issuance, tasks survive closing and reopening the page, a double tap launches one task, failures show stage/code/`ta_status` with the old keybox intact, entered secrets land as `0600` files with no `inbox/*.hex` left behind, risk items start unchecked, TOFU/deploy sheets show the facts before any write, and the loop's `sync-auto` results never replace the page's `sync-task` result.
