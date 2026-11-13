# RKP Mini WebUI Development Plan

[简体中文](WEBUI_DEVELOP_ZH.md) | **English**

> English translation of the Chinese original ([WEBUI_DEVELOP_ZH.md](WEBUI_DEVELOP_ZH.md)), which is authoritative where the two differ.

> Status: **implemented** (after the 2026-11 three-command split). This document is kept as a design record; the implementation status below is authoritative.
> Reader: a new agent session taking over maintenance.
> Read first: the repository root `AGENTS.md` (architecture, constraints, and verification commands), then this document.
>
> **Implementation status (differences from the design draft below)**
>
> - Two standalone pages: `webui/src/apps/issuer/` (issuer plus sender sync) and `webui/src/apps/deployer/` (deployer); shared code lives in `webui/src/shared/`. `entry.ts` accepts only an explicit `--mode issuer|deployer`.
> - Three binaries: `rkp-mini-issuer`, `rkp-mini-sync` (both modules share the same build artifact), and `rkp-mini-deployer` (local deploy only). Module ids are `rkp_mini_issuer` / `rkp_mini_deployer`; the data roots are still `/data/misc/keystore/rkp-mini` and `/data/adb/rkp_sync`.
> - Page invocation: `bin/rkp-mini-sync --role <sender|receiver> --config <path> --json <cmd>` and `bin/rkp-mini-deployer --role receiver --config <path> --json <cmd>` (the `deploy` subcommand was removed).
> - Combined tasks (fetch + apply) are orchestrated by the module script `modules/deployer/run.sh`; the result comes back as the eight-section snapshot in `tmp/sync-task.last.steps`. The page no longer parses the Rust-side `run` envelope.
> - Background scripts moved into the module source tree (`modules/*/webroot/runner/`); Vite builds only the pages (`publicDir: false`).
> - Build artifacts are `webui/dist/issuer/` and `webui/dist/deployer/`; the ZIPs are `dist/rkp_mini_issuer-v*.zip` and `dist/rkp_mini_deployer-v*.zip`.
> - The issuer page has four tabs, Status / Issue / Sync / More, pinned to the bottom with `MiuixNavigationBar`; the current tab is component state only, it writes no URL or history, and the back key still closes the WebUI. The former settings sheet became the More page (`MoreTab.vue`). Status cards follow the KernelSU Miuix home screen `StatusCard` (`HomeMiuix.kt`) coloring: a background tinted by status plus a 110px Material Rounded icon clipped at the bottom-right corner. The deployer likewise has three tabs, Status / Deploy / More (see the end of §7.1).
> - The command names `rkp-mini` / `rkp-sync`, the directories `dist/`, `dist-receiver/`, and the module ids `rkp_mini` / `rkp_sync` below are historical design-stage names; replace them with the section above.

---

## 0. In one sentence

Implement the KernelSU WebUI under `webui/` with **Vue 3 + TypeScript + Vite + Bun + `miuix-vue`**; build artifacts go into the module `webroot/`. Through `ksu.exec` the page calls `rkp-mini-issuer` / `rkp-mini-sync` / `rkp-mini-deployer` to read status and to trigger issuance, sync, and deploy. Issuance and combined tasks all run in a **background process detached from the page**; results are written to files and the page polls them.

---

## 1. Decisions already made (do not argue them again)

| Decision | Conclusion | Rationale |
|---|---|---|
| Framework | Vue 3.5 + TS | `miuix-vue` is an existing Miuix port, zero porting cost |
| Component library | `miuix-vue@0.1.1` (locked exactly) | Tokens and styles taken 1:1 from Miuix `Colors.kt` / `TextStyles.kt` |
| Build | Vite 8 + `@vitejs/plugin-vue`, Bun installs packages and runs scripts | Measured to work, small output |
| Styling | Use the library's `miuix-vue/style.css` plus a little application CSS | No Tailwind, no other UI library |
| Types | tsconfig `paths` fallback (see appendix B) | The upstream 0.1.1 type entry path is broken |
| Routing | None. Single page (the issuer has bottom tabs that only switch component state) + `MiuixBottomSheet` / `MiuixDialog` | The KernelSU back key has no SPA history, so a flat structure is the most robust |
| Issuance execution | Background script + state files + polling | `ksu.exec` has no timeout and page teardown loses the callback |
| Config | Read-only in v1 (shows the raw TOML), no editing | Avoid corrupting `config.toml` |
| Action | Keep `action.sh` | The KernelSU Action and Open buttons coexist, see `ModuleMiuix.kt` |
| Certificate details | Show only fields already in `status.json` | Expanding the full chain needs new binary commands; not in v1 |

---

## 2. Verified facts (do not investigate them again)

Everything below was measured in the 2026-11-09 session or confirmed by reading the source.

### 2.1 Frontend build (measured)

The test page contains 12 `miuix-vue` components, 2 icons, and the theme, built with `bun install && bunx vite build` (Vue 3.5 / Vite 8.3.7 / Bun 1.3.14):

| Artifact | Raw | gzip |
|---|---|---|
| JS | 240.39 kB | **83.89 kB** |
| CSS (whole library stylesheet) | 35.78 kB | **5.72 kB** |
| index.html | 0.41 kB | 0.28 kB |

- The icon entry `miuix-vue/icons` has a 1.3 MB source file, but **tree-shaking works** (only 2 icons are imported, the bundle does not grow).
- `miuix-vue.css` contains **no** `url()`, `@import`, or `http(s)://` → fully offline, no remote fonts or images.
- The production output contains **no** `eval(` / `new Function` → the CSP can be just `script-src 'self'`.
- Hard dependencies: `vue ^3.4`, `motion-v ^2` (peer). `motion-v` pulls in `framer-motion` and `@vueuse/core`; that is the main source of the 84 kB gzip and is the cost of the Miuix spring feel.

### 2.2 Upstream defect (0.1.1)

`package.json` has `types` and `exports["."].types` pointing at `./dist/index.d.ts`, which does not exist in the published package; the real path is `dist/src/index.d.ts` (likewise `icons` resolves to `dist/src/icons/extended/index.d.ts`). A plain `tsc` reports:

```
error TS7016: Could not find a declaration file for module 'miuix-vue'
```

The fix is in appendix B; **`tsc` was measured at exit 0**.

### 2.3 KernelSU WebUI facts (confirmed by reading the manager source)

| Fact | Impact |
|---|---|
| The page URL is `https://mui.kernelsu.org/index.html`; `webroot/` is mapped by `WebViewAssetLoader` plus root reads | Same-origin HTTPS, so ES modules / fetch / Cache API work; relative-path assets work normally |
| `webView.setBackgroundColor(Color.TRANSPARENT)` | **The page must paint its own background**, otherwise the manager's background shows through |
| `allowFileAccess = false`, `domStorageEnabled = true` | `localStorage` works; local files cannot be read (use `exec`/`io`) |
| `textZoom` follows the system font scale | Font sizes written in px/rem scale as well; accessibility is naturally better than canvas |
| With no page history the back key closes the WebUI directly | Do not design navigation that needs "back one level"; sheets provide their own close button |
| `ksu.exec(cmd, optsJson, cbName)` is callback-based, **with no timeout** | Long tasks cannot wait through it; short commands need their own timeout |
| `dispose()` closes only the shell the manager itself holds | Background processes must detach with `setsid`, or they may be killed together with the manager |
| The Open button appears only when the module has `webroot/index.html`; the Action button coexists with it | Keep both entry points |
| The WebView background is transparent and `WebUIScreen` adds inset padding | Do not call `enableEdgeToEdge(true)`; lay out with the default padding |

### 2.4 Existing binary contract (confirmed by reading the source)

- Module id is fixed at `rkp_mini`, binary `/data/adb/modules/rkp_mini/bin/rkp-mini`.
- Data root `DATA_DIR = /data/misc/keystore/rkp-mini`; config `config.toml`; default output `output/keybox.xml`; logs `logs/issue-<unix>.log`; temp directory `tmp/`.
- `status` performs local checks only and writes `status.json` (0600) and `status.txt`; `--update-description` additionally rewrites `module.prop`.
- **A successful `issue` does not refresh `status.json` and does not change the description**; on failure the description is replaced with an error summary.
- `issue` generates a new P-256 key each time; on failure the old keybox is kept; it is not a multi-file transaction.
- `issue` exits with code 2 when `--acknowledge` is missing.

---

## 3. Goals and non-goals

### Goals (v1)

1. Open the WebUI from the KernelSU module card and see the current keybox status and the device prerequisites.
2. After ticking the acknowledgement, trigger one real issuance and show stages, elapsed time, logs, and the result.
3. After a successful issuance, refresh the status and the module description; on failure, explain the stage, error code, and `ta_status`.
4. Show certificate details (chain length, leaf subject, validity, key match, fingerprint/path) with copy support.
5. Show `config.toml`, recent logs, and version information read-only.
6. Package into `dist/rkp_mini-vX.Y.Z.zip` with `module.prop`, scripts, `bin/`, and `webroot/` at the ZIP root.

### Non-goals (explicitly out of scope for v1)

- Editing `config.toml`, changing server/output/fingerprint.
- Showing `keybox.xml`, private keys, full certificate chain contents, or exporting credentials.
- Scheduled/automatic issuance (`auto_issue_days` is still a reserved field).
- Any promise about Play Integrity results.
- V1/AIDL protocol fallback, a daemon, remote private-key delivery.
- Switching module version, uninstalling, or rebooting from the WebUI.

---

## 4. Runtime contract

### 4.1 Path constants (collected in one place in the frontend)

> Current implementation: paths come from each app's `config.ts` (`appConfig`), see `webui/src/apps/*/config.ts`.

```ts
// Issuer (webui/src/apps/issuer/config.ts)
const MODULE_ID = 'rkp_mini_issuer'
const MODULE_DIR = '/data/adb/modules/rkp_mini_issuer'  // prefer moduleInfo().moduleDir
const BIN = `${MODULE_DIR}/bin/rkp-mini-issuer`
const SYNC_BIN = `${MODULE_DIR}/bin/rkp-mini-sync`
const DATA = '/data/misc/keystore/rkp-mini'

// Deployer (webui/src/apps/deployer/config.ts)
//   MODULE_ID = 'rkp_mini_deployer', MODULE_DIR = '/data/adb/modules/rkp_mini_deployer'
//   DEPLOY_BIN = `${MODULE_DIR}/bin/rkp-mini-deployer`, DATA = '/data/adb/rkp_sync'

const TMP = `${DATA}/tmp`
const CONFIG = `${DATA}/config.toml`
const STATUS_JSON = `${DATA}/status.json`
const LOGS = `${DATA}/logs`
```

`output_dir` may have been changed by the user, so **do not hard-code the output path**; always show `status.keybox.path`.

### 4.2 Commands and output

| Command | Purpose | stdout |
|---|---|---|
| `${BIN} status --json` | Read status | Single line `{"ok":true,"data":{...}}` |
| `${BIN} status --json --update-description` | Refresh status and sync the module description | Same as above |
| `${BIN} issue --acknowledge --json` | Issuance (called by the runner only) | Success `{"ok":true,"data":{...}}`; failure `{"ok":false,"error":{...}}` |
| `${BIN} --version` | Version | `rkp-mini-issuer 0.2.0` |
| `cat ${CONFIG}` | Read-only config | TOML text |
| `ls -1t ${LOGS}` | Log list | File names, newest first |
| `tail -n 200 ${LOGS}/<name>` | Log content | Text |

Parsing rule: **parse stdout only**; take the last non-empty line and `JSON.parse` it. stderr is shown for diagnostics only. The `ctx` prefix (`cd`/`export`) is assembled by the bridge, so callers must not add another `cd`.

`status` `data`:

```json
{
  "generated_at": 1762700000,
  "device": {
    "android_arm64": true, "root": true, "qualcomm": true,
    "library_present": true, "device_present": true, "fingerprint": "..."
  },
  "blockers": [],
  "keybox": {
    "path": "/data/misc/keystore/rkp-mini/output/keybox.xml",
    "exists": true, "valid": true, "certificates": 3,
    "leaf_subject": "CN=...", "not_after": 1791000000,
    "not_after_utc": "2027-02-04 00:00:00 UTC",
    "days_left": 87, "key_match": true, "error": null
  },
  "description": "✅ RKP 2027-02-04（87天）"
}
```

(The `description` value above is a runtime string localized by the native side; the English variant of the same format is `✅ RKP 2027-02-04 (87 days)`. The Chinese literal `（87天）` means "87 days".)

`issue` success `data`: `keybox`, `chain_certificates`, `leaf_subject`, `leaf_not_after`, `leaf_not_after_utc`, `device_id`, `csr_len`, `frame_layout`, `ta_status`.

`issue` failure `error`: `stage`, `code`, `message`, optionally `ta_status`, `head_hex`.

blocker values (raw strings; the UI maps them to localized text):

`android_arm64_required`, `root_required`, `qualcomm_required`, `soc_unknown`, `soc_conflicting`, `keymint_library_missing`, `smcinvoke_missing`, `fingerprint_missing`.

### 4.3 Exit codes

| code | Stage | UI text |
|---|---|---|
| 0 | success | Issued |
| 1 | generic | Unknown error |
| 2 | config | Config/usage error |
| 3 | prereq | Prerequisites not met |
| 4 | challenge / server | Network or server failure (HTTP) |
| 5 | ta / csr | Device TA signature or CSR validation failed |
| 6 | chain | Certificate chain validation failed |
| 7 | write | Failed to write files |

`stage` mapping: `config` Config, `prereq` Prerequisites, `challenge` Get challenge, `server` Certificate service, `ta` TA signature, `csr` CSR validation, `chain` Certificate chain, `write` Write, `generic` Other.

### 4.4 Issuance execution (core design)

**Do not** wait for `issue` with `exec`. Use a static script started in the background:

`webui/public/runner/issue.sh` (after the build it lives at `webroot/runner/issue.sh`, 0644, read and executed with `sh`):

```sh
#!/system/bin/sh
# rkp-mini WebUI issue runner. Started by the WebUI as root, detached from the page lifecycle.
set -u
DATA=/data/misc/keystore/rkp-mini
TMP=$DATA/tmp
BIN=/data/adb/modules/rkp_mini/bin/rkp-mini

mkdir -p "$TMP" || exit 9
mkdir "$TMP/issue.lock" 2>/dev/null || exit 9   # a task is already running
trap 'rmdir "$TMP/issue.lock" 2>/dev/null' EXIT

rm -f "$TMP/issue.last.json" "$TMP/issue.last.err" "$TMP/issue.last.code"
"$BIN" issue --acknowledge --json >"$TMP/issue.last.json" 2>"$TMP/issue.last.err"
code=$?
printf '%s\n' "$code" >"$TMP/issue.last.code"
```

Starting it (one `exec` that returns immediately):

```ts
const cmd =
  `setsid sh '${MODULE_DIR}/webroot/runner/issue.sh' </dev/null >/dev/null 2>&1 &`
await exec(cmd)
```

If the device has no `setsid` (`command -v setsid` is empty), fall back to `nohup sh '…/issue.sh' </dev/null >/dev/null 2>&1 &` and record the branch taken in the debug log.

State machine (every page mount and every poll runs the same checks):

| Observation | Conclusion |
|---|---|
| `tmp/issue.lock` exists, no `issue.last.code` | Task running → keep polling |
| `tmp/issue.last.code` exists | Task finished → read json/err, show the result |
| Neither exists | Idle |
| Lock exists but mtime > 10 min and no code | Stale lock → offer "Clear lock" (`rmdir`) |

Polling: every 600 ms, dropping to 1 s after 10 s; after a 120 s upper bound, show "result unknown, check the logs". Read files only, never kill the process (there is no cancel capability, so do not pretend there is one). Detect a stale lock by comparing `stat -c %Y "$TMP/issue.lock"` with the current time.

Restoring on re-entry: while the result files still exist, show a "last issuance result" card until the user closes it.

### 4.5 Refresh order

After the task ends, **whatever the outcome**, run once:

```
${BIN} status --json --update-description
```

Reason: `issue` writes only the keybox and the description, not `status.json`; and on success the description does not change by itself either. Re-render the status card after the refresh. Do not run `status` concurrently while an issuance is running.

---

## 5. Repository change list

### Added

```
webui/
  package.json
  bun.lock                  # committed
  vite.config.ts
  tsconfig.json
  index.html
  public/runner/issue.sh
  src/main.ts
  src/App.vue
  src/env.d.ts
  src/styles/app.css
  src/lib/ksu.ts            # bridge wrapper (timeout, mock)
  src/lib/rkp.ts            # commands + JSON types + parsing
  src/lib/runner.ts         # start/poll/restore
  src/lib/format.ts         # dates, day counts, fingerprint truncation
  src/components/StatusCard.vue
  src/components/CheckCard.vue
  src/components/IssueSheet.vue
  src/components/ResultSheet.vue
  src/components/DetailSheet.vue
  src/components/SettingsSheet.vue
  src/dev/mockKsu.ts        # dev only: fixture-driven
  src/dev/fixtures/*.json
docs/WEBUI_DEVELOP.md       # this document
```

### Modified

| File | Change |
|---|---|
| `build.sh` | Build the webui, copy `webui/dist/.` to `dist/module/webroot/`, fail when missing |
| `.gitignore` | Add `/webui/node_modules`, `/webui/dist` |
| `AGENTS.md` | Change "No ... WebUI" to include the WebUI, and record the build prerequisites (Bun/Vite) |
| `README.md` | Add one line to the install notes: "Open" in the module card enters the WebUI |
| `module/customize.sh` | **No permission logic changes**; KernelSU handles `webroot` permissions and the SELinux context itself |

`module/` stays pure source and holds no build artifacts; `dist/module/` is the staging directory.

---

## 6. Build and packaging

### 6.1 `webui/package.json`

```json
{
  "name": "rkp-mini-webui",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "kernelsu": "3.3.0",
    "miuix-vue": "0.1.1",
    "motion-v": "^2.11.1",
    "vue": "^3.5.35"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^6.0.7",
    "typescript": "~6.0.3",
    "vite": "^8.0.16",
    "vue-tsc": "^3.3.3"
  }
}
```

### 6.2 `webui/vite.config.ts`

```ts
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  base: './',                        // KernelSU loads with relative paths
  build: {
    outDir: 'dist',
    target: 'es2020',
    cssCodeSplit: false,             // single CSS file, easier to confirm the MIME type
  },
})
```

### 6.3 `build.sh`

Insert before `--- Package ---`:

```sh
# --- WebUI --------------------------------------------------------------------
command -v bun >/dev/null 2>&1 || { echo "! bun is required to build the WebUI" >&2; exit 1; }
echo "- Building WebUI"
( cd webui && bun install --frozen-lockfile && bun run build )
[ -f webui/dist/index.html ] || { echo "! webui build produced no index.html" >&2; exit 1; }
```

Add to the packaging section:

```sh
mkdir -p dist/module/webroot
cp -R webui/dist/. dist/module/webroot/
```

The ZIP still uses `( cd dist/module && zip -rq ... . )`, which includes `webroot/` automatically. Suggested addition to the verification section:

```sh
unzip -l "$ZIP" | grep -q 'webroot/index.html' || { echo "! webroot missing" >&2; exit 1; }
```

(`grep` is used only inside scripts and is not affected by the repository tool policy.)

### 6.4 Page skeleton constraints

- Add a CSP to `index.html`:
  `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'none'`
  (styles must keep `'unsafe-inline'`: Vue/motion write inline styles.)
- `html, body, #app { height: 100%; background: var(--m-color-background); }` because the WebView background is transparent.
- `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">`.
- Do not pull in any remote font, image, or CDN.

---

## 7. UI specification

### 7.1 Structure

Issuer: top bar, four tabs (one `MiuixScrollArea` each, switched with `v-show` so each keeps its scroll position), and bottom navigation; no router:

```
MiuixTopAppBar(title="RKP-Mini · Issuer")
Status  StatusCard (colored by status + corner icon; keeps the large remaining-days figure; the top-right status is the refresh entry point, showing "Issuing {elapsed}" while issuing; read/refresh failures go into a warning bar),
        AutoStatusRow (tap to jump to the Issue / Sync tab; auto-issue: issuer status auto_issue.days > 0 and auto-loop.sh running;
        auto-push: sync status enabled + risk_ack_current + auto_push + loop_running), CheckCard
Issue   IssueStatusCard (same style as SyncStatusCard: valid until / days remaining / auto-issue; warning bar when expiring soon/expired/invalid;
        the issuance task and last result live inside the card; the top-right status is the refresh entry point, showing "Issuing {elapsed}"; tap → DetailSheet),
        reasons issuance is unavailable, "Issue now", "Issue settings", disclaimer footnote
Sync    SyncStatusCard (storage / sync time / auto-push; warning bar on anomalies; tap → SyncDetailSheet; the top-right status is the refresh entry point,
        tapping it runs `check-remote` through the runner `check`, and while the task runs the same spot shows "Checking/Pushing {elapsed}"), "Push now" or "Enable", "Share", "Sync settings"
More    MoreTab: Appearance, logs entry, read-only config, About (re-read on every switch)
MiuixNavigationBar  Status / Issue / Sync / More
MiuixSnackbarHost   (mounted once; shifted above the navigation bar)
IssueSheet / ResultSheet / DetailSheet / IssueSettingsSheet / LogsSheet / sync-related sheets (global, layered above the tabs)
```

IssueSettingsSheet ("Issue settings"): auto-issue switch + lead days (1–365, default 3; above 14 shows a frequent-issuance warning, because RKP certificates on this device are valid for only about 12–14 days) → `rkp-mini-issuer config set auto_issue_days N --json`; when enabled, start `auto-loop.sh` with `setsid` (the pid lock makes repeated starts harmless). "Last automatic check" reads `tmp/auto-issue.last.{at,code,json,err}` directly (`code` is written last; when missing, no check has completed). The danger zone item "Delete local certificate" runs `rkp-mini-issuer remove --yes --json` after a MiuixDialog confirmation (disabled while an issuance is running or when there is no local certificate).

Entering the sync tab shows only the cache (`status --json`, including the last `remote_check`) and does not go online; only tapping the top-right status performs a network check. The top-right of the three status cards shares `shared/components/CardStateButton.vue` (status text + refresh icon, spinning while busy). With press feedback enabled, `MiuixCard` calls `setPointerCapture` on pointerdown, after which every click lands on the card itself: buttons inside a card **must** use `@pointerdown.stop`, otherwise they cannot be tapped and the card click fires instead.

SenderSyncSheet: auto-push (`auto_push` switch + minimum remaining validity), cloud storage (including the Gist token / WebDAV credential entries), encryption ("Passphrase", with no mention of age and no display of whether one is set), other (label), save, danger zone (red; delete cloud file / clear local data / fully reset sync settings each ask for a MiuixDialog confirmation first; reset encryption passphrase opens SenderRotateDialog directly, explaining the consequences and asking for the new passphrase twice). A missing secret or bad permissions is reported only in the status card warning bar.

Secret entry is always a dialog (`shared/components/SecretDialog.vue`, no bottom sheet any more) and the input has a placeholder only. The sender sets its own passphrase, enters it twice, and gets a strength hint; the receiver enters it once, and after saving, if a sync stream is already imported, the runner immediately runs `check` (`check-remote`) to test decryption: on success it shows "Decryption succeeded. {label}", on failure "Passphrase check failed: {detail}". The Gist token / WebDAV password are entered once on both ends.

Data and background tasks (`useIssueRunner`, `useSyncRunner`, status reads) stay in `App.vue`; switching tabs does not re-read. All logs live in LogsSheet ("More → Logs", re-read on every open): issuance logs `logs/issue-*.log`, background log `logs/auto.log` (auto-loop.sh: auto-issue + auto-push), and the in-memory page debug log; the sync settings sheet no longer shows logs. "View issuance logs" in the result sheet opens LogsSheet directly at the latest issuance log.

Deployer: the same tab shell (`MiuixNavigationBar` pinned to the bottom, tabs are component state only), three tabs, no top bar buttons:

```
MiuixTopAppBar(title="RKP-Mini · Deployer")
Status  DeployedCard "Current certificate" (deployed keybox: coloring + corner icon + large remaining-days figure; top-right refresh; tap → DeployedDetailSheet),
        AutoStatusRow (auto-pull / auto-deploy, tap to jump to "Deploy"), ModulesCard (lists installed supported modules only: enabled/disabled, whether it is a deploy target, last result)
Deploy  DeployStatusCard "Remote deploy" (storage / sync time / auto-deploy; the warning bar is tappable: new version, pending confirmation → DeploySheet,
        missing passphrase, stale risk, last error; top-right = runner `check` (check-remote, check only, no pull); last task inside the card),
        "Deploy now" (runner `run` = fetch + apply --if-staged) or "Enable", "Deploy settings"
More    MoreTab: Appearance, logs (shared LogsSheet: background log logs/sync.log + debug log), read-only config, About
```

On both ends "More → Appearance" uses `MiuixDropdownPreference` dropdowns for theme and language. The theme keeps follow system / light / dark and persists through `localStorage['rkp.theme']`.

DeploySettingsSheet: auto-deploy (`auto_deploy` + minimum remaining validity + identity change policy + rotation lead days), deploy targets (add/remove; only installed modules can be added; TEESimulator can name the `.xml` file under `/data/adb/teesim`, default `keybox.xml`, and renaming = remove-target + add-target --path), cloud storage (sync link → ImportSheet, WebDAV password, proxy), encryption ("Passphrase"), other (label), save, danger zone (remove the deployed keybox = `rkp-mini-deployer remove --yes`; clear local data = `forget --yes [--staging]`, which also clears the imported link). The page provides no "Disable sync". First import: `run`/`fetch` reports `first_import_needs_confirmation` → TofuSheet → after a successful `fetch <sha>` open DeploySheet directly. A successful manual deploy shows "Run complete"; when targets really were written it adds "Reboot the phone to fully reload the keybox", and it says nothing when there is no new version.

### 7.2 Component mapping (export names verified in `miuix-vue`)

| Purpose | Component |
|---|---|
| Top bar | `MiuixTopAppBar` |
| Group container | `MiuixCard`, `MiuixSmallTitle` |
| Text | `MiuixText` (`type`: `title4` / `body1` / `body2` / `footnote1`; `color` accepts a CSS variable) |
| Primary/secondary button | `MiuixButton` (`type="primary"` / default), `MiuixIconButton` |
| List row | `MiuixBasicComponent` (`title` / `summary` / `summaryColor` / `clickable`) |
| Switch/checkbox | `MiuixSwitch`, `MiuixCheckbox` |
| Sheet | `MiuixBottomSheet`, `MiuixDialog` (`v-model` + `title`/`summary`) |
| Feedback | `MiuixSnackbarHost` + `showSnackbar`, `MiuixProgressIndicator` |
| Divider/scroll | `MiuixDivider`, `MiuixScrollArea` |
| Icon | `MiuixIcon` + named icons (`miuix-vue/icons` or built-ins such as `IconCheck`) |

**There is no `Badge`**: the status pill is written by hand (`border-radius: 9999px`, `tertiaryContainer` background + `onTertiaryContainer` text), about 10 lines of CSS.

Component props are defined by `node_modules/miuix-vue/dist/src/**/*.d.ts`; do not write them from memory.

### 7.3 Status mapping

| Condition | Pill | Color |
|---|---|---|
| `keybox.exists && keybox.valid && days_left > 3` | Valid | `tertiaryContainer` background / `onTertiaryContainer` text |
| `valid && days_left <= 3` (`EXPIRING_DAYS`; RKP certificates on this device last only about 12–14 days) | Expiring soon | application-level `--app-warning` (Miuix has no warning token) |
| `days_left < 0` (or `not_after <= now`) | Expired | `error` |
| `exists && !valid` | Invalid (with `keybox.error`) | `error` |
| `!exists` | Not issued | `onSurfaceVariantSummary` |

Evaluate in table order, first match wins (`not_after <= now` takes precedence over `valid`).

Primary button disabled when: `blockers.length > 0` or a task is running. When disabled, show the first blocker's localized hint above the button.

### 7.4 Token subset (from `miuix-vue/style.css`; do not redefine)

| Variable | Light | Dark |
|---|---|---|
| `--m-color-primary` | `#3482ff` | `#277af7` |
| `--m-color-background` | `#ffffff` | `#242424` |
| `--m-color-on-background` | `#000000` | `#e6ffffff` |
| `--m-color-surface-container` | `#ffffff` | `#242424` |
| `--m-color-secondary-variant` | `#f0f0f0` | `#434343` |
| `--m-color-on-surface-variant-summary` | `#99000000` | `#80ffffff` |
| `--m-color-on-surface-variant-actions` | `#66000000` | `#66ffffff` |
| `--m-color-outline` | `#d9d9d9` | `#404040` |
| `--m-color-divider-line` | `#e0e0e0` | `#393939` |
| `--m-color-error` | `#e94634` | `#f12522` |
| `--m-color-tertiary-container` | `#eaf2ff` | `#2b3b54` |
| `--m-color-on-tertiary-container` | `#3482ff` | `#4788ff` |
| `--m-color-window-dimming` | `rgba(0,0,0,.3)` | `rgba(0,0,0,.6)` |
| `--m-radius-md` | `16px` | same |
| `--m-text-title4-size` | `18px` | same |
| `--m-text-body1-size` | `16px` | same |
| `--m-text-footnote1-size` | `13px` | same |

Application-level additions: `--app-warning: #f5a623` (light) / `#ffb74d` (dark), used for "expiring soon".

### 7.5 Interaction details

- Theme: on startup `setThemeMode(localStorage['rkp.theme'] ?? 'system')`; the library **does not persist** it, so store `system|light|dark` yourself.
- Copy: `navigator.clipboard.writeText`, falling back on failure to a hidden textarea + `execCommand('copy')`; call `showSnackbar` on success.
- Touch feedback comes from the library's `::after` alpha overlay; do not add extra `hover` styles.
- A sheet must have its own close button and close on backdrop tap; do not rely on the system back key.
- Warn before leaving the page while a task is running; on re-entry restore per 4.4.
- All long text (`head_hex`, logs) goes into collapsible areas, collapsed by default.

### 7.6 Internationalization (i18n)

- Framework: `vue-i18n@11` (Composition mode, `legacy: false`; `vite.config.ts` disables the legacy API and production devtools). Messages are JIT-compiled to an AST and interpreted at runtime; the production bundle was measured to contain no `eval`/`new Function`, so the CSP is unchanged.
- Languages: `zh-CN` (source language), `zh-TW`, `en`, `ja`, `ko`, `de`, `fr`, `it`. Three message trees: `src/shared/locales/` (keys `shared.*`), `src/apps/issuer/locales/` (`issuer.*`), `src/apps/deployer/locales/` (`deployer.*`); each app bundle loads only `shared` plus its own tree on demand, and every language is a separate lazy-loaded chunk.
- Default language: match `navigator.languages` in order (`zh-TW/HK/MO/Hant` → Traditional, other `zh*` → Simplified, anything else by primary language prefix), falling back to `en`. "More → Appearance → Language" allows a manual choice stored in `localStorage['rkp.locale']`; choosing "Follow system" deletes that key. A missing key falls back to `zh-CN`; if the language bundle fails to load, the page stays on `zh-CN` and does not go blank.
- API (`src/shared/lib/i18n.ts`): `t(key, params?, count?)` has typed key names, and `vue-tsc` reports unknown keys; `tCode(key)` is for keys built from machine codes (`shared.sync.code.<code>`) and returns `undefined` when the source language has no such key, in which case the caller shows the raw code; `MessageKey` is for constant tables; `currentLocale()` is for `Intl` (`formatAgo` uses `Intl.RelativeTimeFormat`).
- Conventions: non-source language files declare `satisfies typeof zhCN`, so a missing or extra key fails `vue-tsc`. Do not call `t()` at module top level (language switches must take effect immediately); one key per whole sentence with named parameters, never concatenated translation fragments; count parameters are always named `{n}` and passed via `t(key, count)`, and `en/de/fr/it` may use the `singular | plural` form (French uses the singular for 0 and 1). In messages `{ } @ $ |` are syntax characters; write a literal one as `{'|'}`.
- The module card description follows the language chosen in the WebUI: at page start and on every language switch, one of the eight actually enabled languages is written atomically to `ui-language` (0600) in the respective data root, instead of trying to read `localStorage` from the native process. Choosing "Follow system" records the current resolution; if the system language changes after the page closes, it is only synced the next time the WebUI opens. All issuer status descriptions read that file when generated, falling back to `zh-CN` when it is missing or invalid; while issuing, only the language file is updated, the description is not refreshed concurrently, and it is updated by the normal write at the end of issuance or by a later page refresh. The deployer translates only its existing static introduction and adds no dynamic status. The KernelSU module list may need a refresh to show the change. A failed native sync does not roll back or block the Vue language switch; a manual switch shows the error, and at startup it is recorded in the debug log.
- Native descriptions use `rust-i18n`, with language packs at `crates/rkp-mini-{issuer,deployer}/locales/<locale>.json`, embedded at compile time with no runtime pack loading. Vue and Rust neither import, convert, nor copy each other's packs; they share only the language codes above. Native calls pass the locale explicitly and never change the global language; the remaining-days count uses only a lightweight `one/other` choice (English/German/Italian use `one` for 1, French uses `one` for 0 and 1, Chinese/Japanese/Korean are unchanged). New native text changes only the eight JSON files of the owning crate, verifying that keys and placeholders agree; no Vue pack changes are required.
- Description refresh is handled by the owning binary's `update-description`: the issuer reads the issuance config and re-checks local status, writes the description only (no status snapshot), and exits 7 on a write failure; the existing `status --update-description` still keeps the status command's exit-0 contract. The deployer reads no sync config and requires no initialization or risk approval, updates the static introduction only, and exits 1 on a write failure. Both atomically update their own `module.prop` and keep the remaining fields and permissions. The WebUI does not assemble or translate module descriptions and does not write `module.prop` directly.
- Human-readable messages from Action and the module loops follow `ui-language` as well: scripts only ask the owning binary's internal `__action-message KEY [VALUE]` for fixed messages, and all eight translations live in the owning crate's `action.*` packs, with no separate shell/Vue translation table. They cover the run banner, round start/end, per-step exit summaries, skip reasons, and lock conflicts; shared round and loop logs use the same messages. Message parameters are inserted literally and are never executed or reinterpreted; an unknown message or a wrong parameter count is a usage error (exit 2). JSON, machine codes, raw diagnostics, and the existing task exit semantics are unchanged. `status.description` is localized by the issuer's native layer and the WebUI does not translate it again; `error.message`, mocks/fixtures under `dev/`, and raw debug logs are not translated. The risk statement comes only from `rkp-mini-sync risk` (two variants, `zh`/`en`): a Chinese interface shows `zh` with `en` appended, other interfaces show only `en`, and the WebUI never translates it separately.
- New text: add it to the matching `zh-CN.ts` first, then add the same key to the other 7 language files; a passing `bun run build` means the key names agree.

---

## 8. Bridge wrapper

`src/lib/ksu.ts`:

```ts
import { exec as ksuExec } from 'kernelsu'
import { mockExec } from '../dev/mockKsu'

export interface ExecResult { errno: number; stdout: string; stderr: string }

const TIMEOUT_MS = 10_000

export async function exec(command: string): Promise<ExecResult> {
  if (import.meta.env.DEV && !('ksu' in window)) return mockExec(command)
  const timeout = new Promise<never>((_, reject) =>
    setTimeout(() => reject(new Error(`exec timeout: ${command}`)), TIMEOUT_MS),
  )
  return Promise.race([ksuExec(command), timeout])
}
```

Key points:

- `kernelsu@3.3.0` uses ESM syntax but the package has no `"type": "module"`, so it **works only inside a bundler** (Vite/esbuild are fine); do not import it directly in Node.
- The package checks `typeof ksu !== "undefined"` at module load, so importing it in a normal browser does not throw, but `exec` rejects; the dev mock must short-circuit before the call.
- `errno` is the process exit code.
- The timeout only abandons the wait, it **does not** kill the background command; it does not apply to issuance (issuance goes through the runner).

---

## 9. Security and credentials

- Never read, print, transfer, or render `keybox.xml`, `key.pem`, or full CSR contents.
- Use only fields from `status.json` and the `issue` envelope; `keybox.path` is displayed as a string only.
- Provide no export/download/share.
- Logs may contain paths and error messages and may be shown; still avoid whole CSRs.
- Never interpolate user input into shell commands in the page; all command paths are constants.
- No remote resources (CSP + offline requirement).

---

## 10. Phased implementation plan

### Phase 0 — Spike (do it first, it decides the rest)

1. A minimal page in `webui/`: `MiuixButton` plus one button calling `exec('id -u')`, with the result shown on the page.
2. After building, push to the device:
   `adb push webui/dist/. /sdcard/webroot/ && adb shell su -c 'cp -R /sdcard/webroot/. /data/adb/modules/rkp_mini/webroot/'`
3. The KernelSU module card shows "Open", the page renders, dark mode follows the system, and `id -u` returns `0`.

**Acceptance**: the page starts, `ksu.exec` works, Chinese renders, no remote requests.

### Phase 1 — Skeleton and design system

`package.json` / `vite.config.ts` / `tsconfig.json` / theme / status pill / `StatusCard` / `CheckCard`, all fixture-driven.

**Acceptance**: `bun run dev` covers the 5 states in the browser; `bun run build` passes `vue-tsc`.

### Phase 2 — Status reading and refresh

`src/lib/rkp.ts`: `readStatus()`, `refreshStatus()`; read on mount, refresh by pull-to-refresh/button; each error state (read failure, JSON parse failure, non-zero exit) has its own UI.

**Acceptance**: on the device the status matches `su -c 'rkp-mini status'`; blockers are shown correctly.

### Phase 3 — Issuance flow

`runner/issue.sh`, `runner.ts` (start, poll, restore), confirmation sheet, progress, result sheet, failure diagnostics.

**Acceptance**:
- The button is disabled before ticking; after ticking it starts and the page shows "in progress".
- Leaving the WebUI and coming back still shows in-progress/completed.
- Success: the status card updates and the `module.prop` description updates.
- Failure (network unplugged, TA unreachable): stage/code/ta_status are shown and the old keybox is still there.
- Two quick taps start only one task (the lock works).

### Phase 4 — Details and settings

Certificate details sheet, fingerprint copy, log list and viewer, read-only TOML, version information.

**Acceptance**: copy works; logs are read-only; no private key content appears.

### Phase 5 — Packaging and device acceptance

`build.sh` integration, ZIP verification, `AGENTS.md`/`README` updates, one full run on a real device (see §11).

**Acceptance**: `dist/rkp_mini-vX.Y.Z.zip` has `webroot/index.html` at the root; after installation both the Open and Action entry points work.

### Optional Phase 6 — Stage progress

Add `--progress` to the binary: at each `issue` step write a single line to stderr,
`rkp-progress:{"id":"ta","state":"start"}`, and have the page tail `issue.last.err` to map it into a step bar.
**It does not change the stdout contract**. Not in v1; use "elapsed time + log tail" for now.

---

## 11. Testing and acceptance

### 11.1 Host

```sh
cd webui
bun install --frozen-lockfile
bun run build          # vue-tsc + vite build
bun run dev            # browser + mock ksu (fixture)
```

- Coverage: ready, blocked, running, success, failure, unknown result, stale lock.
- Check the output: `ls -l dist/assets`, one CSS file and one JS file, total gzip < 150 kB.
- `grep -R "eval(\|new Function" dist` should be empty.

### 11.2 Device checklist

| Item | Expectation |
|---|---|
| Open the WebUI | Page renders, no blank screen, background does not show through |
| Dark mode | Follows the system; a manual switch survives a reopen |
| Enlarged system font | Text grows with it, no clipping |
| `status --json` | Matches the CLI output |
| blocker | Primary button disabled with the reason visible |
| Issuance success | Status and description update, remaining days correct |
| Issuance failure | Stage/error code/`ta_status` visible, old keybox not replaced |
| Leave midway and re-enter | Result is not lost |
| Repeated issuance taps | Only one task runs |
| Back key | Closes the WebUI instead of going back one screen (by design) |
| `unzip -l` | ZIP root contains `module.prop`, `bin/rkp-mini`, `webroot/index.html` |

---

## 12. Risks and known defects

| Risk | Notes | Handling |
|---|---|---|
| `miuix-vue` is 0.1.x | The API may change | Lock 0.1.1 exactly; read the changelog and run `vue-tsc` before upgrading |
| Broken type entry | See §2.2 | The `paths` from appendix B, verified |
| `exec` has no timeout | Cannot be canceled once stuck | 10 s frontend timeout; issuance never waits through `exec` |
| Manager killed | Background tasks may be interrupted | `setsid` detach + file state; "unknown result" is visible on re-entry |
| Concurrent issuance | The Action button can run `issue` too | The WebUI lock only blocks the WebUI; a real fix needs locking in the binary (later) |
| WebView version differences | Old managers may lack `ksu` or use a different origin | Probe `window.ksu` at startup and show a compatibility hint when it is missing |
| Text not selectable | None (DOM rendering is actually better) | — |
| Package size | About 90 kB gzip added | Acceptable, no optimization needed |

---

## 13. Documentation sync (required during implementation)

- `AGENTS.md`: delete or rewrite "No TEE/boot-state spoofing, WebUI, ...", and add the `webui/` directory, the Bun build prerequisite, and the `build.sh` behavior. Otherwise the next agent is misled by the old constraints.
- `README.md`: how to open the WebUI after installation, and where to look when something fails.
- `module/module.prop`: keep the version/versionCode in sync with `Cargo.toml`; the `webui/package.json` version does not participate in the module version.

---

## Appendix A: Command cheat sheet

```sh
# Host (WebUI)
cd webui && bun install --frozen-lockfile
cd webui && bun run build          # vue-tsc + dist/issuer + dist/deployer
cd webui && bun run dev:issuer     # issuer preview (fixture)
cd webui && bun run dev:deployer   # deployer preview (fixture)

# Rust side
cargo fmt --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo test --workspace --locked

# Packaging
NDK=/path/to/ndk ./build.sh
unzip -l dist/rkp_mini_issuer-v0.4.0.zip
unzip -l dist/rkp_mini_deployer-v0.4.0.zip

# Device
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer status --json'
su -c '/data/adb/modules/rkp_mini_issuer/bin/rkp-mini-issuer issue --acknowledge --json'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-sync --role receiver --json status'
su -c '/data/adb/modules/rkp_mini_deployer/bin/rkp-mini-deployer --role receiver --json plan'

# Iterate without reinstalling the module
adb push webui/dist/issuer/. /sdcard/webroot/
adb shell su -c 'cp -R /sdcard/webroot/. /data/adb/modules/rkp_mini_issuer/webroot/'
```

---

## Appendix B: Type fallback (verified)

`webui/tsconfig.json`:

```json
{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "target": "es2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "types": ["vite/client"],
    "paths": {
      "miuix-vue": ["./node_modules/miuix-vue/dist/src/index.d.ts"],
      "miuix-vue/icons": ["./node_modules/miuix-vue/dist/src/icons/extended/index.d.ts"]
    }
  },
  "include": ["src/**/*.ts", "src/**/*.vue"]
}
```

- TypeScript 6 removed `baseUrl`, so `paths` resolve relative to the tsconfig; do not add `baseUrl` back.
- `src/env.d.ts`:

```ts
/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}
```

---

## Appendix C: Minimal page skeleton

```vue
<script setup lang="ts">
import { onMounted, ref } from 'vue'
import {
  MiuixButton, MiuixCard, MiuixText, MiuixSmallTitle,
  MiuixSnackbarHost, setThemeMode,
} from 'miuix-vue'
import { exec } from './lib/ksu'

setThemeMode((localStorage.getItem('rkp.theme') as 'system' | 'light' | 'dark') ?? 'system')

const version = ref('…')

onMounted(async () => {
  const r = await exec('/data/adb/modules/rkp_mini/bin/rkp-mini --version')
  version.value = r.errno === 0 ? r.stdout.trim() : `读取失败 (${r.errno})`
})
</script>

<template>
  <main class="page">
    <MiuixSmallTitle text="RKP Mini" />
    <MiuixCard>
      <MiuixText type="body2">二进制版本：{{ version }}</MiuixText>
    </MiuixCard>
    <MiuixButton type="primary">签发新证书</MiuixButton>
    <MiuixSnackbarHost />
  </main>
</template>

<style>
.page { padding: 16px; display: flex; flex-direction: column; gap: 12px; }
</style>
```

---

## Appendix D: Verification record for this plan

| Item | Result |
|---|---|
| `miuix-vue@0.1.1` production build | Passed, JS 240.39 kB / 83.89 kB gzip, CSS 35.78 kB / 5.72 kB gzip |
| Icon tree-shaking | Passed (importing 2 icons did not pull in the 1.3 MB icon set) |
| Remote CSS resources | No `url()` / `@import` / http |
| `eval`/`new Function` in the output | None |
| `tsc` resolving types directly | Failed `TS7016` (upstream defect) |
| `tsc` with `paths` + `skipLibCheck` | Passed (exit 0) |
| Vue/TS versions | Vue 3.5, Vite 8.3.7, TS 6.0, Bun 1.3.14, Node 26 |
| Running on a device | **Not verified**; Phase 0 must be done first |
