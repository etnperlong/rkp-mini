# Repository Guidelines

## Project Overview

RKP Mini is a Rust CLI shipped as a KernelSU module, developed against a single Qualcomm reference device (arm64); other devices and firmware are untested, and no successful on-device issuance is recorded. It uses Qualcomm KeyMint Native V2 (`0x3122`) to obtain RKP certificates for a fresh userspace P-256 key and export a TrickyStore-compatible keybox.

Keep the original scope: Native V2 only; report unsupported firmware rather than adding V1/AIDL fallbacks. No TEE/boot-state spoofing, persistent daemon, automatic issuance, or remote private-key deployment. `auto_issue_days` is reserved, not a scheduler. Issuance does not promise Play Integrity success.

The module ships a KernelSU WebUI (`webui/` → `webroot/`) as a thin shell over the CLI's `--json` contract: it reads status, starts issuance and shows results, logs and read-only config. It never reads or renders keybox/private-key contents, edits configuration, or adds protocol paths. The Action button stays as the second entry point.

For protocol or scope changes, consult the original external design note (kept outside this repository). Treat current source as the implementation reference; the design's proposed layout and acceptance checklist are not evidence of completed device validation.

## Architecture & Data Flow

- `src/main.rs` dispatches the hand-written `src/cli.rs` commands: `status`, `issue`, `config init|show`, and internal `__ta-worker`.
- `src/rkp/mod.rs` orchestrates issuance: acknowledge → load configuration → check prerequisites → generate key → fetch challenge → encode request → invoke TA worker → decode and validate CSR → append fingerprint → request certificates → validate/order chain → write artifacts → update module description.
- `src/rkp/protocol.rs` owns CBOR/COSE and five TA frame layouts. Preserve challenge/public-key binding, request limits, and field `57` as a byte string containing a CBOR key array, not a direct array.
- `src/rkp/keymint.rs` isolates vendor ABI calls by re-executing the binary. Parent/worker exchange `request.bin` and `reply.bin` using the `RKP1` envelope; the parent enforces a timeout and silences child stdout/stderr. Keep unsafe vendor calls behind this boundary.
- `src/rkp/server.rs` performs blocking CBOR HTTP calls through `ureq`; `chain.rs` checks DER structure, issuer linkage, leaf key and validity; `keybox.rs` builds/inspects XML. Local validation does **not** cryptographically verify CSR or certificate signatures.
- `src/status.rs` performs local checks only: no network or TA calls. It writes status files; `--update-description` additionally rewrites `module.prop`. Issuance updates the description but does not itself refresh `status.json`.
- WebUI (`webui/src/`): `lib/ksu.ts` wraps `ksu.exec` with a 10 s client timeout; `lib/rkp.ts` holds every path constant, command, JSON type and Chinese label; `lib/runner.ts` launches `webroot/runner/issue.sh` detached (`setsid`, `nohup` fallback) and polls `tmp/issue.lock` / `issue.last.{code,json,err}`. After each finished task the page runs `status --json --update-description` once. Its `mkdir` lock only guards WebUI launches; Action/CLI issuance can still run concurrently.

## Key Directories

| Path | Purpose |
| --- | --- |
| `src/`, `src/rkp/` | CLI/config/status infrastructure and the issuance pipeline; one binary crate, no separate library |
| `module/` | KernelSU installer, Action, boot-status and uninstall hooks |
| `dist/module/`, `dist/*.zip` | Generated staging tree and installable ZIP; edit `module/` and `webui/`, not staged copies |
| `webui/` | Vue 3 + `miuix-vue` WebUI source; `public/runner/issue.sh` is the background issue runner; `src/dev/` is a dev-only fixture mock |
| `target/` | Generated host and Android build outputs |

Tests live beside implementation, not in the design's proposed `tests/` directory.

## Development Commands

Run from the repository root:

```sh
cargo build --locked                  # Host build
cargo run -- --help                   # Safe CLI smoke check
cargo run -- config show               # Effective configuration
cargo test --locked                    # Host unit tests
cargo test --locked rkp::protocol::tests # Focused protocol tests
cargo fmt --check                      # Standard formatting check
cargo clippy --all-targets -- -D warnings # Recommended lint check
rustup target add aarch64-linux-android
NDK=/path/to/android-ndk ./build.sh     # Android release build + WebUI + KernelSU ZIP
cd webui && bun install --frozen-lockfile && bun run build # vue-tsc + vite build only
cd webui && bun run dev                 # Browser preview against fixtures: ?mock=ready|expiring|expired|invalid|missing|blocked|status-exit|status-parse|status-timeout, ?runner=idle|running|overdue|success|failure|crashed|stale, ?outcome=success|failure
```

`build.sh` configures the NDK linker and `ring` C compiler variables, targets Android API 26, builds the WebUI with Bun (frozen lockfile), copies `webui/dist/` to `dist/module/webroot/`, and checks the ELF/ZIP with `file`, `readelf` and `unzip`. Output: `dist/rkp_mini-v<Cargo version>.zip`. The ZIP root must contain `module.prop`, the shell hooks, `bin/rkp-mini` and `webroot/index.html`, without a wrapper directory; the script fails if any is missing.

On a rooted target after installation:

```sh
su -c '/data/adb/modules/rkp_mini/bin/rkp-mini status --json'
su -c '/data/adb/modules/rkp_mini/bin/rkp-mini issue --acknowledge --json'
```

The second command performs real network/TA issuance and creates new credentials; use it only for an intentional device test. Host `status` cannot satisfy the compile-time Android arm64 prerequisite and still attempts status-file writes.

## Code Conventions & Common Patterns

- Use standard Rust formatting, `snake_case` functions/modules, `UpperCamelCase` types and `SCREAMING_SNAKE_CASE` constants. Keep module documentation and `SAFETY` explanations around unsafe operations.
- Keep execution synchronous and command-scoped. Pass `&Config`, paths, keys and byte slices explicitly; pure encoders/parsers are separate from HTTP, process and filesystem boundaries. There is no async runtime, DI container or shared service state.
- Return `anyhow::Result` internally; use `error::err(Stage, code, message)` / `AppError` for stage-specific failures. Preserve machine-readable codes, raw `ta_status` and diagnostic `head_hex`. Exit codes: 1 generic, 2 usage/config, 3 prerequisites, 4 HTTP, 5 TA/CSR, 6 chain/keybox, 7 writes.
- Keep `--json` stdout to one result envelope; send diagnostics to stderr. Description/log updates are best-effort and must not turn successful issuance into failure. Existing module descriptions are Chinese; CLI/help is English.
- Configuration precedence is CLI > TOML > defaults. Only `issue` accepts server/output/fingerprint overrides; TOML is validated before overrides. Missing files use defaults; unknown keys warn. Persist custom `output_dir` in TOML if subsequent `status` must inspect it.
- Use Unix seconds and UTC. Reuse `util::atomic_write` and `ensure_dir_0700`; enforce `0600` credential/data files and `0700` private directories. Installer `mkdir -p` does not enforce `0700` on output/log directories before first use. Keys use SEC1 `EC PRIVATE KEY`, not PKCS#8; certificates are leaf→root.
- Preserve the old keybox on failure. Current writes are individually atomic, **not a multi-file transaction**: chain/key/meta files precede `keybox.xml`, with a best-effort `keybox.xml.prev` backup. Do not describe the whole output set as transactional or clear the output directory before issuance.
- Treat private keys and keyboxes as credentials: never print, log, upload or commit them. Avoid logging complete CSRs. Keep fresh key/challenge generation for every issuance attempt.

## Important Files

- `src/config.rs`: defaults, validation and commented config template. Default config is `/data/misc/keystore/rkp-mini/config.toml`; default output is its sibling `output/`. Logs, worker temporary files and `status.{json,txt}` stay under the fixed data root even with a custom output directory.
- `src/error.rs`: shared error/exit contract; `src/util.rs`: atomic writes, permissions, UTC, hashing and XML escaping.
- `src/test_support.rs`: shared generated certificate/key fixtures, compiled only for tests.
- `Cargo.toml`, `Cargo.lock`, `build.sh`: dependencies, release profile and cross-build/package behavior. Keep Cargo version and `module/module.prop` version/versionCode synchronized; packaging does not update module metadata.
- `module/action.sh`: foreground `exec issue --acknowledge`, preserving the exit code. `service.sh`: background one-shot local status refresh only. `customize.sh`: arm64 gate, permissions, config initialization and non-fatal status refresh. `uninstall.sh`: deliberately retains runtime credentials/data.
- Installer permission caveat: `customize.sh` resets files to `0644` and restores execution only for the binary, Action and service hooks; verify uninstall invocation when changing hook permissions.
- `README.md`: installation and firmware failure guidance. Its bare design-document link is external, and its stated default NDK version is not enforced by `build.sh`.
- `webui/src/lib/rkp.ts` mirrors the CLI JSON contract (status/issue envelopes, blocker codes, stages, exit codes). Changing `--json` output, blockers or stages requires updating it and `src/dev/fixtures/`, then `bun run build`.

## Runtime/Tooling Preferences

Use Cargo/Rust 2021 and the committed lockfile. No Rust toolchain/MSRV pin is declared. Android packaging assumes an x86_64 Linux host, Bash, Android NDK, Bun, `zip`, `unzip`, `file` and `readelf`. Set `NDK` explicitly for reproducibility; otherwise the script selects the last matching directory under `~/Android/Sdk/ndk/`, not a pinned version.

WebUI stack: Vue 3 + TypeScript + Vite, Bun for install/scripts, committed `webui/bun.lock`. `miuix-vue` is pinned to exactly `0.1.1`; its published type entry is broken, so `tsconfig.json` maps `miuix-vue` and `miuix-vue/icons` via `paths` (TypeScript 6, no `baseUrl`). Use Miuix tokens from `miuix-vue/style.css`; app-owned tokens (`--app-warning`, `--app-sheet-group`) live in `src/styles/app.css`. KernelSU's WebView is transparent and has no SPA history (back closes the WebUI), so the page paints its own background, uses one page plus bottom sheets with explicit close buttons, and loads nothing remote (production CSP sets `connect-src 'none'`). Shell commands only interpolate constant paths and log names matching `issue-<digits>.log`.

Keep the small synchronous stack: hand-written CLI parsing, `ureq` with Rust TLS, `ciborium`, `p256` and `x509-cert`. Do not introduce `clap`, an async runtime, `reqwest`, or a runtime OpenSSL executable dependency. Module hooks use `/system/bin/sh`, not Bash. Device prerequisites include root, Qualcomm hardware, `/vendor/lib64/libkeymasterdeviceutils.so`, `/dev/smcinvoke` and a fingerprint. Report SELinux/path failures rather than silently relocating data.

## Testing & QA

Use Rust's built-in `#[test]` framework in inline `#[cfg(test)] mod tests`. Reuse `rcgen` chains from `src/test_support.rs` and `tempfile` for isolated files; keep host tests offline and independent of root/vendor libraries. Existing tests cover config, frame/CSR parsing and binding, server response parsing/HTTP error mapping, chain/keybox handling, worker reply framing and description states.

No coverage percentage, coverage tooling or CI workflow is configured. Add behavioral regression coverage for changed boundaries and failure paths; parser tests do not establish end-to-end HTTP, worker timeout, output rollback or installed-module correctness.

Reference-device firmware acceptance remains a device-only go/no-go check; the repository contains no recorded successful device result. Record actual KeyMint version, TA status and issuance outcome when tested. Host tests and a valid AArch64 ZIP cannot substitute for this. Diagnose `-18`, `-40`, `-100` and HTTP `444` explicitly rather than changing protocols. Device QA must check JSON/errors, key-to-leaf binding, leaf→root ordering, `0700`/`0600` permissions, old-keybox preservation, Action exit status and boot refresh without issuance. Release uses `panic = "abort"`; worker `catch_unwind` is not release panic recovery.

The WebUI has no automated tests: `bun run build` (vue-tsc) plus the dev-mock scenarios cover host checks. Device-only WebUI QA: the Open button renders without a transparent/white background, status matches `status --json`, blockers disable issuance, issuance survives closing and reopening the WebUI, a double tap launches one task, and failures show stage/code/`ta_status` with the old keybox intact.
