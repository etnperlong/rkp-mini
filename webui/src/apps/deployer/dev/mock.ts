// Dev-only stand-in for the KernelSU bridge in the deployer build. Never
// imported by production bundles (shared/lib/ksu.ts imports it dynamically in DEV).
//
// Scenarios (?sync=): new | imported | ready | pending_first | pending_identity | error
// Fetch result (?fetch=): tofu | ok | fail | mismatch  (default ok)
// Deploy result (&deploy=ok|fail|omk_incompatible): `fail` makes one configured
// target report a write failure; `omk_incompatible` makes the omk target report
// `omk_incompatible` (plan warns for every gate) while the others succeed, and
// behaves like `ok` when no omk target is configured.
// Combined run (&run=): ok | fetch_fail | missing_envelope | skipped | stale | noop
//   (`noop` = idempotent pass: the deploy step reports every target as already
//    matching with reload_status `skipped`, so nothing new was deployed)
// Installed target modules (&modules=): comma list of omk | teesim | tricky_store,
//   each optionally `:disabled`; `none`; `fail` (detection error). Default omk,tricky_store.
// Cached remote check (&remote=): none | in_sync | ahead | behind | diverged | missing | error
//   (`none` = never checked; its `check` task reports in_sync).
// Wrong password (&decrypt=fail): `check` tasks fail with decrypt/decrypt_failed.
//
// Sample import links (also accepted as their JSON):
//   rkpsync://v1/import?p=eyJ2IjoxLCJwcm92aWRlciI6Imdpc3QiLCJ1cmwiOiJodHRwczovL2dpc3QuZ2l0aHVidXNlcmNvbnRlbnQuY29tL2V2YW5zL2FiYzEyMy9yYXcvcmtwLXN5bmMuYWdlIiwiZ2lzdF9pZCI6ImFiYzEyMyIsImZpbGVuYW1lIjoicmtwLXN5bmMuYWdlIiwic3RyZWFtX2lkIjoic3RfZGVtb19yZWNlaXZlciIsImxhYmVsIjoicGhvbmUtYSIsImZvcm1hdCI6ImFnZS1hcm1vciJ9
//   rkpsync://v1/import?p=eyJ2IjoxLCJwcm92aWRlciI6IndlYmRhdiIsImVuZHBvaW50IjoiaHR0cHM6Ly9kYXYuamlhbmd1b3l1bi5jb20vZGF2LyIsInBhdGgiOiIvcmtwL3Bob25lLWEta2V5Ym94LmFnZSIsInVzZXIiOiJtZUBleGFtcGxlLmNvbSIsInN0cmVhbV9pZCI6InN0X2RlbW9fcmVjZWl2ZXIiLCJsYWJlbCI6InBob25lLWEiLCJmb3JtYXQiOiJhZ2UtYXJtb3IifQ
//
// Secrets: the `secret set` command carries hex on stdin-ish shell builtins;
// this mock never logs or echoes the hex, only the fact that a file was filled.
import type { ExecResult } from '../../../shared/lib/ksu'
import { nowUnix } from '../../../shared/lib/format'
import type {
  CheckOutcome,
  DeployPlan,
  DeployReport,
  DeployState,
  ImportPreview,
  ImportResult,
  LocationInfo,
  Pending,
  RemoteCheck,
  RemoteFact,
  RiskStatement,
  Staged,
  SyncConfig,
  SyncEnvelope,
  SyncErrorInfo,
  SyncState,
  SyncStatus,
  TargetReport,
  TargetState,
  TofuFacts,
} from '../../../shared/lib/sync'
import type { BuiltinTargetInfo } from '../lib/deploy'
import scenariosJson from './fixtures/receiver/scenarios.json'
import tofuJson from './fixtures/receiver/tofu.json'

const DAY = 86_400
const SEP = '<<<RKP-SEP>>>'
const DATA = '/data/adb/rkp_sync'
const CONFIG = `${DATA}/config.toml`

interface SyncStateFixture {
  seq: number
  device_id: string
  keybox_sha256: string
  plaintext_sha256: string
  keybox_not_after_days: number
  verified_days_ago: number
}

interface StagedFixture {
  seq: number
  device_id: string
  keybox_sha256: string
  plaintext_sha256: string
  leaf_not_after_days: number
  leaf_not_before_days_ago: number
}

interface DeployedFixture {
  seq: number
  keybox_sha256: string
  leaf_not_after_days: number
  at_days_ago: number
}

interface PendingFixture {
  seq: number
  keybox_sha256: string
  leaf_not_after_days: number
  reason: string
}

interface TargetFixture {
  id: string
  path: string
  owner?: string
  mode?: string
  reload?: string
  reload_status?: string | null
  last_error?: { code: string; message: string } | null
}

interface ScenarioFixture {
  enabled: boolean
  risk_ack: boolean
  provider: string
  location: string
  stream_id: string
  label: string
  password_mode: 'secret-file' | 'prompt'
  webdav_user: string
  gist_id: string
  gist_filename: string
  webdav_endpoint: string
  webdav_path: string
  url?: string
  age_present: boolean
  webdav_password_present: boolean
  sync_state: SyncStateFixture | null
  staged: StagedFixture | null
  deployed: DeployedFixture | null
  pending: PendingFixture | null
  loop_running: boolean
  last_error: { code: string; message: string } | null
  identity_change_policy: 'scheduled' | 'always' | 'never'
  rotation_lead_days: number
  auto_deploy: boolean
  min_validity_days: number
  request_timeout_seconds?: number
  proxy?: string
  targets: TargetFixture[]
  /** Targets whose deployed file this mock removed. */
  cleared?: string[]
}

interface TofuFixture {
  provider: string
  location: string
  stream_id: string
  seq: number
  device_id: string
  leaf_not_before_days_ago: number
  leaf_not_after_days: number
  keybox_sha256: string
  plaintext_sha256: string
  targets: string[]
}

const FIXTURES = scenariosJson as unknown as Record<string, ScenarioFixture>
const TOFU = tofuJson as unknown as TofuFixture

const params = new URLSearchParams(location.search)
const initialScenario = params.get('sync') ?? 'ready'
const fetchMode = params.get('fetch') ?? 'ok'
const deployMode = params.get('deploy') ?? 'ok'
/** Combined run shape: ok | fetch_fail | missing_envelope | skipped | stale. */
const runMode = params.get('run') ?? 'ok'
/** Which builtin-target modules `targets` reports as installed. */
const modulesMode = params.get('modules') ?? 'omk,tricky_store'
/** Cached `check-remote` record: none | in_sync | ahead | behind | diverged | missing | error. */
const remoteMode = params.get('remote') ?? 'none'
const decryptFails = params.get('decrypt') === 'fail'

const BUILTIN_TARGETS: BuiltinTargetInfo[] = [
  {
    id: 'omk',
    path: '/data/misc/keystore/omk/keybox.xml',
    owner: '1017:1017',
    mode: '0600',
    module: { id: 'oh_my_keymint', installed: false, enabled: false, name: 'Oh My Keymint', version: '1.2.6' },
  },
  {
    id: 'teesim',
    path: '/data/adb/teesim/keybox.xml',
    owner: '0:0',
    mode: '0600',
    module: { id: 'teesim', installed: false, enabled: false, name: 'TEESimulator', version: 'canary-76' },
  },
  {
    id: 'tricky_store',
    path: '/data/adb/tricky_store/keybox.xml',
    owner: '0:0',
    mode: '0600',
    module: { id: 'tricky_store', installed: false, enabled: false, name: 'TEESimulator-RS', version: 'v6.0.1' },
  },
]

function detectedTargets(): BuiltinTargetInfo[] {
  const flags = new Map(
    modulesMode.split(',').map((entry) => {
      const [id, flag] = entry.trim().split(':')
      return [id, flag !== 'disabled'] as const
    }),
  )
  return BUILTIN_TARGETS.map((b) => {
    const enabled = flags.get(b.id)
    return enabled === undefined
      ? { ...b, module: { ...b.module, name: null, version: null } }
      : { ...b, module: { ...b.module, installed: true, enabled } }
  })
}

let initialized = initialScenario !== 'new'
let state: ScenarioFixture = structuredClone(
  initialScenario === 'new' ? FIXTURES.imported : (FIXTURES[initialScenario] ?? FIXTURES.ready),
)

// Trust-on-first-use facts; accepting passes exactly this plaintext digest.
let tofuFacts: TofuFacts = makeTofu(TOFU.seq, TOFU.keybox_sha256, TOFU.plaintext_sha256, 3)
let tofuBumped = false

function atDays(days: number): number {
  return nowUnix() + Math.round(days * DAY)
}

function agoDays(days: number): number {
  return nowUnix() - Math.round(days * DAY)
}

function makeTofu(seq: number, keybox: string, plaintext: string, issuedDaysAgo: number): TofuFacts {
  return {
    provider: state.provider || 'gist',
    location: state.location || TOFU.location,
    stream_id: state.stream_id || TOFU.stream_id,
    seq,
    issued_at: agoDays(issuedDaysAgo),
    device_id: TOFU.device_id,
    leaf_not_before: agoDays(TOFU.leaf_not_before_days_ago + issuedDaysAgo - 3),
    leaf_not_after: atDays(TOFU.leaf_not_after_days),
    keybox_sha256: keybox,
    plaintext_sha256: plaintext,
    targets: (state.targets ?? []).map((t) => t.path),
    root_trust_checked: false,
  }
}

// ---- Envelopes -------------------------------------------------------------------

function okEnv(data: unknown): SyncEnvelope<unknown> {
  return { ok: true, data }
}

function errEnv(
  stage: string,
  code: string,
  message: string,
  data?: unknown,
): SyncEnvelope<unknown> {
  return data === undefined
    ? { ok: false, error: { stage, code, message } }
    : { ok: false, error: { stage, code, message }, data }
}

interface Outcome {
  envelope: SyncEnvelope<unknown>
  errno: number
  stderr: string
}

function okOutcome(data: unknown): Outcome {
  return { envelope: okEnv(data), errno: 0, stderr: '' }
}

function errOutcome(stage: string, code: string, message: string, data?: unknown): Outcome {
  const errno = stage === 'deploy' ? 9 : stage === 'config' ? 2 : 8
  return { envelope: errEnv(stage, code, message, data), errno, stderr: `${code}: ${message}` }
}

function toExec(outcome: Outcome): ExecResult {
  return { errno: outcome.errno, stdout: JSON.stringify(outcome.envelope) + '\n', stderr: outcome.stderr }
}

// ---- State views -----------------------------------------------------------------

function secretStatus(path: string, present: boolean): { path: string; present: boolean; mode?: string; ok: boolean } {
  return present ? { path, present: true, mode: '0600', ok: true } : { path, present: false, ok: false }
}

function buildSyncState(): SyncState | null {
  const s = state.sync_state
  if (!s) return null
  return {
    v: 1,
    provider: state.provider,
    location: state.location,
    stream_id: state.stream_id,
    verified_seq: s.seq,
    plaintext_sha256: s.plaintext_sha256,
    keybox_sha256: s.keybox_sha256,
    device_id: s.device_id,
    keybox_not_after: atDays(s.keybox_not_after_days),
    last_remote_token: 'W/"demo-etag"',
    last_checked_at: agoDays(0.1),
    verified_at: agoDays(s.verified_days_ago),
    last_error: state.last_error
      ? { code: state.last_error.code, message: state.last_error.message, at: agoDays(0.2) }
      : null,
  }
}

function buildStaged(): Staged | null {
  const s = state.staged
  if (!s) return null
  return {
    v: 1,
    stream_id: state.stream_id,
    seq: s.seq,
    keybox_sha256: s.keybox_sha256,
    plaintext_sha256: s.plaintext_sha256,
    device_id: s.device_id,
    leaf_not_before: agoDays(s.leaf_not_before_days_ago),
    leaf_not_after: atDays(s.leaf_not_after_days),
    staged_at: agoDays(0.05),
  }
}

function ownerOf(id: string, owner?: string | null): string {
  return owner ?? (id === 'omk' ? '1017:1017' : '0:0')
}

function buildTargetStates(): TargetState[] {
  const cleared = new Set(state.cleared ?? [])
  return state.targets.map((t) => {
    const deployed = state.deployed && !cleared.has(t.id)
    return {
      id: t.id,
      path: t.path,
      owner: ownerOf(t.id, t.owner),
      mode: t.mode ?? '0600',
      deployed_sha256: deployed ? state.deployed!.keybox_sha256 : null,
      deployed_at: deployed ? agoDays(state.deployed!.at_days_ago) : null,
      reload: t.reload ?? 'auto',
      reload_status: deployed ? (t.reload_status ?? 'ok') : null,
      last_error: t.last_error
        ? { code: t.last_error.code, message: t.last_error.message, at: agoDays(0.1) }
        : null,
    }
  })
}

function buildPending(): Pending | null {
  const p = state.pending
  if (!p) return null
  return {
    seq: p.seq,
    keybox_sha256: p.keybox_sha256,
    leaf_not_after: atDays(p.leaf_not_after_days),
    reason: p.reason,
    at: agoDays(0.05),
  }
}

function buildDeployState(): DeployState | null {
  if (!state.deployed && !state.pending) return null
  return {
    v: 1,
    stream_id: state.stream_id,
    last_seen_keybox_sha256: state.staged?.keybox_sha256 ?? state.deployed?.keybox_sha256 ?? null,
    last_seen_seq: state.staged?.seq ?? state.deployed?.seq ?? null,
    deployed_keybox_sha256: state.deployed?.keybox_sha256 ?? null,
    deployed_seq: state.deployed?.seq ?? null,
    deployed_at: state.deployed ? agoDays(state.deployed.at_days_ago) : null,
    deployed_leaf_not_after: state.deployed ? atDays(state.deployed.leaf_not_after_days) : null,
    targets: buildTargetStates(),
    pending: buildPending(),
  }
}

function buildStatus(): SyncStatus {
  const secrets: SyncStatus['secrets'] = {}
  if (state.password_mode === 'secret-file') {
    secrets.age = secretStatus(`${DATA}/secrets/age.pass`, state.age_present)
  }
  if (state.provider === 'webdav' && state.webdav_user) {
    secrets.webdav_password = secretStatus(`${DATA}/secrets/webdav.pass`, state.webdav_password_present)
  }
  const targets = state.targets.length
    ? state.targets.map((t) => ({ id: t.id, path: t.path }))
    : { error: 'no deployment targets configured' }
  return {
    role: 'receiver',
    config: CONFIG,
    enabled: state.enabled,
    risk_ack_version: state.risk_ack ? 2 : 0,
    risk_ack_current: state.risk_ack,
    risk_version: 2,
    loop_running: state.loop_running,
    provider: state.provider,
    location: state.location || null,
    stream_id: state.stream_id,
    password_mode: state.password_mode,
    secrets,
    sync_state: buildSyncState(),
    remote_check: cachedRemoteCheck(),
    deploy_state: buildDeployState(),
    staged: buildStaged(),
    targets,
    identity_change_policy: state.identity_change_policy,
  }
}

/** `remote=` value → the outcome recorded in remote-check.json. */
const REMOTE_OUTCOME: Record<string, CheckOutcome> = {
  in_sync: 'in_sync',
  behind: 'remote_behind',
  ahead: 'remote_ahead',
  diverged: 'diverged',
  missing: 'missing',
  error: 'error',
}

/** When a `check` task last ran in this preview; null = never (fresh `at` each status read). */
let remoteCheckedAt: number | null = null

/** `remote-check.json` for the `remote=` scenario at time `at`. */
function buildRemoteCheck(at: number): RemoteCheck {
  const outcome: CheckOutcome = remoteMode === 'none' ? 'in_sync' : (REMOTE_OUTCOME[remoteMode] ?? 'in_sync')
  const local = state.sync_state
  const deployed = state.deployed
  let remote: RemoteFact | null = {
    seq: deployed?.seq ?? local?.seq ?? 0,
    plaintext_sha256: state.staged?.plaintext_sha256 ?? local?.plaintext_sha256 ?? null,
    keybox_sha256: deployed?.keybox_sha256 ?? local?.keybox_sha256 ?? null,
    device_id: local?.device_id ?? null,
    issued_at: agoDays(2),
    keybox_not_after: atDays(deployed?.leaf_not_after_days ?? 118),
  }
  let error: SyncErrorInfo | null = null

  if (outcome === 'remote_ahead' && remote) {
    remote = { ...remote, seq: remote.seq + 1, keybox_sha256: NEXT_KEYBOX }
  } else if (outcome === 'remote_behind' && remote) {
    remote = { ...remote, keybox_sha256: 'e7b9c1d3f5a7b9c1d3e5f7a9b1c3d5e7f9a1b3c5d7e9f1a3b5c7d9e1f3a5b7c9' }
  } else if (outcome === 'diverged' && remote) {
    remote = { ...remote, plaintext_sha256: '9d8c7b6a5f4e3d2c1b0a9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b2a1f0e9d8c' }
  } else if (outcome === 'missing') {
    remote = null
  } else if (outcome === 'error') {
    remote = null
    error = { stage: 'remote', code: 'network', message: 'mock: remote unavailable' }
  }

  return {
    at,
    role: 'receiver',
    outcome,
    remote,
    local: { seq: local?.seq ?? null, keybox_sha256: local?.keybox_sha256 ?? null },
    error,
  }
}

/** Cached record: null until a version is imported, or for `remote=none` before any check. */
function cachedRemoteCheck(): RemoteCheck | null {
  if (!initialized || !state.stream_id) return null
  if (remoteMode === 'none' && remoteCheckedAt === null) return null
  return buildRemoteCheck(remoteCheckedAt ?? agoDays(0.1))
}

/** A `check-remote` run: fresh record, remembered like `<root>/sync/remote-check.json`. */
function runRemoteCheck(): RemoteCheck {
  remoteCheckedAt = nowUnix()
  return buildRemoteCheck(remoteCheckedAt)
}

function buildConfig(): SyncConfig {
  return {
    role: 'receiver',
    config: CONFIG,
    enabled: state.enabled,
    risk_ack_version: state.risk_ack ? 2 : 0,
    risk_version: 2,
    provider: state.provider,
    stream_id: state.stream_id,
    label: state.label,
    min_validity_days: state.min_validity_days,
    request_timeout_seconds: state.request_timeout_seconds ?? 30,
    proxy: state.proxy ?? '',
    gist_id: state.gist_id,
    gist_filename: state.gist_filename,
    gist_url: state.provider === 'gist' ? state.location : '',
    webdav_endpoint: state.webdav_endpoint,
    webdav_path: state.webdav_path,
    webdav_user: state.webdav_user,
    url: state.url ?? '',
    password_mode: state.password_mode,
    secret_files: {
      age: `${DATA}/secrets/age.pass`,
      gist_token: `${DATA}/secrets/gist.token`,
      webdav_password: `${DATA}/secrets/webdav.pass`,
    },
    inbox: `${DATA}/sync/inbox`,
    identity_change_policy: state.identity_change_policy,
    rotation_lead_days: state.rotation_lead_days,
    auto_deploy: state.auto_deploy,
    targets: state.targets.map((t) => ({
      id: t.id,
      path: t.path,
      owner: ownerOf(t.id, t.owner),
      mode: t.mode ?? '0600',
      reload: t.reload ?? 'auto',
    })),
  }
}

const RISK_ITEMS = [
  {
    id: 'cloning',
    en: "The keybox is the issuing device's RKP identity. Using it on another device is keybox cloning/spoofing, not legitimate RKP provisioning.",
    zh: '该 keybox 是签发设备的 RKP 身份；在新设备上使用属于 keybox 克隆/冒充，不是合法的 RKP provisioning。',
  },
  {
    id: 'integrity',
    en: 'Play Integrity, Key Attestation and banking/payment apps may reject it, and Google may revoke it.',
    zh: 'Play Integrity、Key Attestation、银行/支付应用可能拒绝；Google 可能吊销。',
  },
  {
    id: 'key_copy',
    en: 'The private key is copied to another device; if either device leaks, both are compromised.',
    zh: '私钥会被复制到新设备；任一台设备泄露，另一台一起失守。',
  },
  {
    id: 'passphrase_on_disk',
    en: 'For unattended runs the age passphrase is stored in a 0600 file. Losing the device or a root compromise leaks it; then change the passphrase AND re-issue the keybox.',
    zh: '为支持自动运行，age 密码保存在 0600 文件；设备丢失或 root 被攻破即视为泄露，必须换密码并重新签发 keybox。',
  },
  {
    id: 'debug_manager',
    en: "Secrets entered in the WebUI pass through the KernelSU manager's root shell; a debug build of the manager logs shell commands to logcat.",
    zh: '在 WebUI 输入的密钥会经过 KernelSU 管理器的 root shell；debug 版管理器会把 shell 命令写入 logcat。',
  },
  {
    id: 'omk_retire',
    en: "OhMyKeymint's retire_stale_keybox_bound_keys (default true) deletes client keys bound to the previous keybox when the keybox changes.",
    zh: 'OMK 的 retire_stale_keybox_bound_keys=true 会在轮换 keybox 时删除绑定旧 keybox 的 key。',
  },
  {
    id: 'no_revocation',
    en: 'Disabling sync does not revoke anything; this is not legitimate cross-device provisioning and the risk is yours.',
    zh: '禁用不等于撤销；这不是合法的跨设备 provisioning，风险由使用者承担。',
  },
]

function buildRisk(): RiskStatement {
  return {
    version: 1,
    items: RISK_ITEMS,
    passphrase_path: initialized && state.password_mode === 'secret-file' ? `${DATA}/secrets/age.pass` : null,
    accepted_version: initialized ? (state.risk_ack ? 2 : 0) : null,
  }
}

// ---- Deploy plan / apply ---------------------------------------------------------

type Gate = DeployPlan['gate']

function gateFor(allow: boolean): Gate {
  const staged = state.staged
  if (!staged) throw new Error('nothing staged')
  const deployed = state.deployed
  if (!deployed) return { kind: 'first_deploy' }
  if (deployed.keybox_sha256 === staged.keybox_sha256) return { kind: 'same' }
  if (allow) return { kind: 'allowed', reason: 'manual_allow' }
  switch (state.identity_change_policy) {
    case 'always':
      return { kind: 'allowed', reason: 'policy_always' }
    case 'never':
      return { kind: 'blocked', reason: 'identity_change' }
    case 'scheduled': {
      const inside = deployed.leaf_not_after_days <= state.rotation_lead_days
      return inside && staged.leaf_not_after_days > deployed.leaf_not_after_days
        ? { kind: 'allowed', reason: 'rotation_window' }
        : { kind: 'blocked', reason: 'identity_change' }
    }
  }
}

const OMK_INCOMPATIBLE_WARNING =
  'OhMyKeymint would reject this keybox and overwrite it with its bundled template (no `<Key algorithm="ecdsa">` block in the literal form OhMyKeymint scans for); the omk target will not be written.'

function planWarnings(gate: Gate): { code: string; message: string }[] {
  const warnings: { code: string; message: string }[] = []
  if (deployMode === 'omk_incompatible' && state.targets.some((t) => t.id === 'omk')) {
    warnings.push({ code: 'omk_incompatible', message: OMK_INCOMPATIBLE_WARNING })
  }
  const changes = gate.kind === 'allowed' || gate.kind === 'first_deploy'
  if (changes && state.targets.some((t) => t.id === 'omk')) {
    warnings.push({
      code: 'omk_key_retirement',
      message:
        'OhMyKeymint `retire_stale_keybox_bound_keys` (default true) deletes client keys bound to the previous keybox after a keybox change; affected apps must provision new keys.',
    })
  }
  if (changes && state.targets.some((t) => t.id === 'teesim')) {
    warnings.push({
      code: 'teesim_reattest',
      message:
        'TEESimulator re-attests existing client keys with the new keybox; attestations issued with the previous keybox may stop verifying.',
    })
  }
  warnings.push({
    code: 'no_root_trust_check',
    message:
      'keybox certificates were checked structurally only; signatures, root trust and revocation were not verified.',
  })
  return warnings
}

function planData(allow: boolean): DeployPlan {
  const staged = buildStaged()
  if (!staged) throw new Error('nothing staged')
  const gate = gateFor(allow)
  return {
    gate,
    staged,
    old_keybox_sha256: state.deployed?.keybox_sha256 ?? null,
    old_leaf_not_after: state.deployed ? atDays(state.deployed.leaf_not_after_days) : null,
    warnings: planWarnings(gate),
    targets: state.targets.map((t) => ({
      id: t.id,
      path: t.path,
      owner: ownerOf(t.id, t.owner),
      mode: t.mode ?? '0600',
      reload: t.reload ?? 'auto',
    })),
    root_trust_checked: false,
  }
}

function targetFailure(id: string, path: string, lastId: string | null): TargetReport['error'] {
  if (deployMode === 'omk_incompatible' && id === 'omk') {
    return {
      code: 'omk_incompatible',
      message:
        'OhMyKeymint would reject this keybox and overwrite it with its bundled template; the omk target was not written.',
    }
  }
  if (deployMode === 'fail' && id === lastId) {
    return {
      code: 'target_dir_missing',
      message: `target directory for ${path} does not exist (module not installed?)`,
    }
  }
  return null
}

function reportTargets(): TargetReport[] {
  const lastId = state.targets.length ? state.targets[state.targets.length - 1].id : null
  return state.targets.map((t) => {
    const error = targetFailure(t.id, t.path, lastId)
    const ok = error === null
    t.reload_status = ok ? 'ok' : null
    t.last_error = error
    return {
      id: t.id,
      path: t.path,
      ok,
      sha256: ok ? (state.staged?.keybox_sha256 ?? null) : null,
      reload: t.reload ?? 'auto',
      reload_status: ok ? 'ok' : null,
      error,
    }
  })
}

function applyPrepared(plan: DeployPlan): { report: DeployReport; failed: TargetReport[] } {
  const reports = reportTargets()
  const anyOk = reports.some((r) => r.ok)
  if (anyOk) {
    state.deployed = {
      seq: plan.staged.seq,
      keybox_sha256: plan.staged.keybox_sha256,
      leaf_not_after_days: Math.max(
        0,
        Math.round((plan.staged.leaf_not_after - nowUnix()) / DAY),
      ),
      at_days_ago: 0,
    }
    state.pending = null
    state.cleared = []
  }
  const failed = reports.filter((r) => !r.ok)
  return {
    report: {
      keybox_sha256: plan.staged.keybox_sha256,
      seq: plan.staged.seq,
      deployed: anyOk,
      pending: buildPending(),
      warnings: plan.warnings,
      targets: reports,
    },
    failed,
  }
}

function holdPending(reason: 'first_deploy' | 'identity_change', gate: Gate): DeployReport {
  const staged = state.staged!
  state.pending = {
    seq: staged.seq,
    keybox_sha256: staged.keybox_sha256,
    leaf_not_after_days: staged.leaf_not_after_days,
    reason,
  }
  return {
    keybox_sha256: staged.keybox_sha256,
    seq: staged.seq,
    deployed: false,
    pending: buildPending(),
    warnings: planWarnings(gate),
    targets: [],
  }
}

function isShaPrefix(full: string, given: string): boolean {
  return given.length >= 16 && full.startsWith(given)
}

// ---- Import parsing --------------------------------------------------------------

const ALLOWED_FIELDS = [
  'v',
  'provider',
  'url',
  'gist_id',
  'filename',
  'endpoint',
  'path',
  'user',
  'stream_id',
  'label',
  'format',
]
const FORBIDDEN_FIELDS = ['password', 'passphrase', 'token', 'secret', 'key']

interface ImportSpec {
  provider: 'gist' | 'webdav' | 'url'
  stream_id: string
  label: string | null
  location: LocationInfo
  display: string
  url: string
  gist_id: string
  gist_filename: string
  endpoint: string
  path: string
  user: string
}

/** rkpsync://v1/import?p=<base64url(JSON)> or the decoded JSON itself. */
function parseSpec(input: string): ImportSpec {
  let text = input.trim()
  if (text.startsWith('rkpsync://')) {
    const p = new URL(text.replace('rkpsync://', 'https://')).searchParams.get('p')
    if (!p) throw new Error('import_invalid')
    const padded = p.replace(/-/g, '+').replace(/_/g, '/')
    try {
      text = atob(padded + '='.repeat((4 - (padded.length % 4)) % 4))
    } catch {
      throw new Error('import_invalid')
    }
  }
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    throw new Error('import_invalid')
  }
  if (typeof raw !== 'object' || raw === null) throw new Error('import_invalid')
  const spec = raw as Record<string, unknown>
  for (const key of Object.keys(spec)) {
    if (FORBIDDEN_FIELDS.includes(key.toLowerCase())) throw new Error('secret_field')
    if (!ALLOWED_FIELDS.includes(key)) throw new Error('unknown_field')
  }
  if (spec.v !== 1 || spec.format !== 'age-armor') throw new Error('import_invalid')
  const provider = spec.provider
  if (provider !== 'gist' && provider !== 'webdav' && provider !== 'url') throw new Error('import_invalid')
  if (typeof spec.stream_id !== 'string' || !spec.stream_id) throw new Error('import_invalid')
  const label = typeof spec.label === 'string' && spec.label ? spec.label : null
  const base = {
    provider,
    stream_id: spec.stream_id,
    label,
    url: '',
    gist_id: '',
    gist_filename: 'rkp-sync.age',
    endpoint: '',
    path: '',
    user: '',
  } as ImportSpec

  if (provider === 'gist') {
    const url = typeof spec.url === 'string' ? spec.url : ''
    const gistId = typeof spec.gist_id === 'string' ? spec.gist_id : ''
    if (!url && !gistId) throw new Error('import_invalid')
    if (url && !/^https:\/\//.test(url)) throw new Error('insecure_url')
    if (url && /^https:\/\/[^/]*@/.test(url)) throw new Error('url_userinfo')
    const filename = typeof spec.filename === 'string' ? spec.filename : 'rkp-sync.age'
    if (!/^[A-Za-z0-9._-]+\.age$/.test(filename)) throw new Error('import_invalid')
    base.url = url
    base.gist_id = gistId
    base.gist_filename = filename
    base.location = { provider: 'gist', url: url || null, gist_id: gistId || null, filename }
    base.display = url || (gistId ? `gist:${gistId}/${filename}` : '')
  } else if (provider === 'webdav') {
    const endpoint = typeof spec.endpoint === 'string' ? spec.endpoint : ''
    const path = typeof spec.path === 'string' ? spec.path : ''
    const user = typeof spec.user === 'string' ? spec.user : ''
    if (!endpoint || !path) throw new Error('import_invalid')
    if (!/^https:\/\//.test(endpoint)) throw new Error('insecure_url')
    if (/^https:\/\/[^/]*@/.test(endpoint)) throw new Error('url_userinfo')
    base.endpoint = endpoint
    base.path = path
    base.user = user
    base.location = { provider: 'webdav', endpoint, path, user: user || null }
    base.display = `${endpoint.replace(/\/$/, '')}${path}`
  } else {
    const url = typeof spec.url === 'string' ? spec.url : ''
    if (!/^https:\/\//.test(url)) throw new Error('insecure_url')
    if (/^https:\/\/[^/]*@/.test(url)) throw new Error('url_userinfo')
    base.url = url
    base.location = { provider: 'url', url }
    base.display = url
  }
  return base
}

function importPreview(spec: ImportSpec): ImportPreview {
  const current = initialized && state.stream_id ? state.stream_id : null
  const verified = initialized ? state.sync_state : null
  const streamChange = !!current && current !== spec.stream_id
  return {
    location: spec.location,
    display: spec.display,
    stream_id: spec.stream_id,
    label: spec.label,
    current_stream_id: current,
    stream_change: streamChange,
    blocked: streamChange && !!verified && verified.seq > 0,
    verified_seq: verified?.seq ?? null,
  }
}

function applyImport(spec: ImportSpec): ImportResult {
  const previous = initialized ? state.stream_id : ''
  if (previous && previous !== spec.stream_id && state.sync_state && state.sync_state.seq > 0) {
    throw new Error('stream_change')
  }
  const sameStream = initialized && previous === spec.stream_id
  if (!sameStream) state = structuredClone(FIXTURES.imported)
  initialized = true
  state.provider = spec.provider
  state.location = spec.display
  state.stream_id = spec.stream_id
  state.label = spec.label ?? ''
  state.gist_id = spec.gist_id
  state.gist_filename = spec.gist_filename
  state.webdav_endpoint = spec.endpoint
  state.webdav_path = spec.path
  state.webdav_user = spec.user
  state.url = spec.provider === 'url' ? spec.url : ''
  const notes = ['location saved; nothing was downloaded or deployed']
  remoteCheckedAt = null
  if (!state.age_present) notes.push('set the age passphrase with `rkp-mini-sync secret set age`')
  if (spec.provider === 'webdav' && spec.user) notes.push('set the WebDAV app password')
  if (!state.risk_ack) notes.push('sync stays disabled until the risk statement is accepted')
  tofuFacts = makeTofu(tofuFacts.seq, tofuFacts.keybox_sha256, tofuFacts.plaintext_sha256, 3)
  return {
    provider: spec.provider,
    location: spec.display,
    stream_id: spec.stream_id,
    label: spec.label,
    enabled: state.enabled,
    test: null,
    notes,
  }
}

// ---- Fetch / run -----------------------------------------------------------------

const NEXT_KEYBOX = 'a7c9e1b3d5f7a9c0e2b4d6f8a0c2e4b6d8f0a2c4e6b8d0f2a4c6e8b0d2f4a6c8'
const NEXT_PLAINTEXT = 'c0e2b4d6f8a0c2e4b6d8f0a2c4e6b8d0f2a4c6e8b0d2f4a6c8e0b2d4f6a8c0e2'

function stageFromFacts(facts: TofuFacts, firstImport: boolean): Outcome {
  const previous = state.staged?.seq ?? state.sync_state?.seq ?? null
  const days = Math.max(1, Math.round((facts.leaf_not_after - nowUnix()) / DAY))
  state.staged = {
    seq: facts.seq,
    device_id: facts.device_id,
    keybox_sha256: facts.keybox_sha256,
    plaintext_sha256: facts.plaintext_sha256,
    leaf_not_after_days: days,
    leaf_not_before_days_ago: 0,
  }
  state.sync_state = {
    seq: facts.seq,
    device_id: facts.device_id,
    keybox_sha256: facts.keybox_sha256,
    plaintext_sha256: facts.plaintext_sha256,
    keybox_not_after_days: days,
    verified_days_ago: 0,
  }
  state.last_error = null
  return okOutcome({
    provider: facts.provider,
    stream_id: facts.stream_id,
    changed: true,
    seq: facts.seq,
    plaintext_sha256: facts.plaintext_sha256,
    keybox_sha256: facts.keybox_sha256,
    device_id: facts.device_id,
    leaf_not_after: facts.leaf_not_after,
    staged_path: `${DATA}/staging/${facts.keybox_sha256}.xml`,
    previous_seq: previous,
    gap: false,
    first_import: firstImport,
    warnings: [],
    root_trust_checked: false,
  })
}

function doFetch(acceptFirst?: string): Outcome {
  if (!initialized) return errOutcome('config', 'not_initialized', 'no rkp-mini-sync configuration found')
  if (!state.enabled) return errOutcome('config', 'sync_disabled', 'remote sync is disabled')
  if (!state.stream_id) return errOutcome('config', 'stream_missing', 'no sync stream configured')
  if (fetchMode === 'fail') {
    state.last_error = { code: 'remote_unavailable', message: 'WebDAV endpoint returned 503' }
    return errOutcome('remote', 'remote_unavailable', 'WebDAV endpoint returned 503')
  }
  if (fetchMode === 'tofu' || fetchMode === 'mismatch') {
    if (acceptFirst === undefined) {
      return errOutcome(
        'state',
        'first_import_needs_confirmation',
        'first import must be confirmed: review the facts and pass --accept-first <plaintext_sha256>',
        tofuFacts,
      )
    }
    if (fetchMode === 'mismatch' && !tofuBumped && isShaPrefix(tofuFacts.plaintext_sha256, acceptFirst)) {
      tofuBumped = true
      tofuFacts = makeTofu(tofuFacts.seq + 1, NEXT_KEYBOX, NEXT_PLAINTEXT, 0)
    }
    if (!isShaPrefix(tofuFacts.plaintext_sha256, acceptFirst)) {
      return errOutcome(
        'state',
        'confirmation_mismatch',
        'the remote version changed since it was reviewed; review it again',
        tofuFacts,
      )
    }
    return stageFromFacts(tofuFacts, true)
  }

  // ok: an existing staged version is idempotent; otherwise stage a new one.
  if (state.staged) {
    state.last_error = null
    return okOutcome({
      provider: state.provider,
      stream_id: state.stream_id,
      changed: false,
      seq: state.staged.seq,
      plaintext_sha256: state.staged.plaintext_sha256,
      keybox_sha256: state.staged.keybox_sha256,
      device_id: state.staged.device_id,
      leaf_not_after: atDays(state.staged.leaf_not_after_days),
      staged_path: `${DATA}/staging/${state.staged.keybox_sha256}.xml`,
      previous_seq: state.staged.seq,
      gap: false,
      first_import: false,
      warnings: [],
      root_trust_checked: false,
    })
  }
  const seq = (state.sync_state?.seq ?? 0) + 1
  const facts: TofuFacts = makeTofu(seq, NEXT_KEYBOX, NEXT_PLAINTEXT, 0)
  return stageFromFacts(facts, false)
}

interface RunSnapshot {
  op: string
  json: string
  err: string
  code: number
  steps: string
}

/** Eight sections, seven separator lines — matching lib/run.sh's snapshot. */
function snapshot(
  overall: number,
  fetchCode: number | null,
  fetchJson: string,
  fetchErr: string,
  deployCode: number | null,
  deployJson: string,
  deployErr: string,
  orchestration: string,
): string {
  return [
    String(overall),
    fetchCode === null ? '' : String(fetchCode),
    fetchJson,
    fetchErr,
    deployCode === null ? '' : String(deployCode),
    deployJson,
    deployErr,
    orchestration,
  ].join(`\n${SEP}\n`)
}

function deployStep(): { code: number; json: string; err: string } {
  if (!state.staged) {
    return { code: 0, json: JSON.stringify(okEnv({ skipped: 'nothing_staged' })), err: '' }
  }
  if (!state.auto_deploy) {
    return { code: 0, json: JSON.stringify(okEnv({ skipped: 'auto_deploy_false' })), err: '' }
  }
  // `run=noop`: an idempotent pass rewrites nothing, so every target reports
  // reload_status `skipped` (the real `repair_all` behaviour).
  if (runMode === 'noop') {
    try {
      const plan = planData(false)
      const report: DeployReport = {
        keybox_sha256: plan.staged.keybox_sha256,
        seq: plan.staged.seq,
        deployed: true,
        pending: null,
        warnings: plan.warnings,
        targets: state.targets.map((t) => ({
          id: t.id,
          path: t.path,
          ok: true,
          sha256: plan.staged.keybox_sha256,
          reload: t.reload ?? 'auto',
          reload_status: 'skipped',
          error: null,
        })),
      }
      return { code: 0, json: JSON.stringify(okEnv(report)), err: '' }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      return { code: 9, json: JSON.stringify(errEnv('state', 'nothing_staged', message)), err: message }
    }
  }
  try {
    const plan = planData(false)
    if (plan.gate.kind === 'first_deploy') {
      return { code: 0, json: JSON.stringify(okEnv(holdPending('first_deploy', plan.gate))), err: '' }
    }
    if (plan.gate.kind === 'blocked') {
      return { code: 0, json: JSON.stringify(okEnv(holdPending('identity_change', plan.gate))), err: '' }
    }
    const { report, failed } = applyPrepared(plan)
    if (failed.length) {
      const message = `deployment failed for: ${failed.map((f) => f.id).join(', ')}`
      return {
        code: 9,
        json: JSON.stringify(errEnv('deploy', 'target_failed', message, report)),
        err: `target_failed: ${message}`,
      }
    }
    return { code: 0, json: JSON.stringify(okEnv(report)), err: '' }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    return { code: 9, json: JSON.stringify(errEnv('state', 'nothing_staged', message)), err: message }
  }
}

/**
 * The combined run publishes its per-step sidecars, never a Rust `run`
 * envelope; mocks produce the same files the module script does.
 */
function buildRunSnapshot(): RunSnapshot {
  const fetched =
    runMode === 'fetch_fail'
      ? errOutcome('remote', 'remote_unavailable', 'WebDAV endpoint returned 503')
      : doFetch(undefined)
  const fetchJson = JSON.stringify(fetched.envelope)
  const fetchErr = fetched.stderr

  let deploy: { code: number; json: string; err: string }
  if (runMode === 'skipped') {
    deploy = { code: 0, json: JSON.stringify(okEnv({ skipped: 'nothing_staged' })), err: '' }
  } else if (runMode === 'missing_envelope') {
    deploy = { code: 1, json: '', err: 'rkp-mini-deployer: crashed before printing JSON\n' }
  } else {
    deploy = deployStep()
  }

  const overall = deploy.code !== 0 ? deploy.code : fetched.errno
  const orchestration = runMode === 'stale' ? 'stale snapshot: last.op=fetch\n' : ''
  return {
    op: runMode === 'stale' ? 'fetch' : 'run',
    json: runMode === 'stale' ? fetchJson : '',
    err: runMode === 'stale' ? fetchErr : '',
    code: overall,
    steps: snapshot(overall, fetched.errno, fetchJson, fetchErr, deploy.code, deploy.json, deploy.err, orchestration),
  }
}


// ---- Short commands --------------------------------------------------------------

function requireInit(): Outcome | null {
  return initialized
    ? null
    : errOutcome('config', 'not_initialized', 'no rkp-mini-sync configuration found; run `rkp-mini-sync init --role receiver`')
}

function handleImport(args: string[]): ExecResult | null {
  if (args[0] !== 'import') return null
  let spec: ImportSpec
  try {
    spec = parseSpec(args[1] ?? '')
  } catch (e) {
    const code = e instanceof Error ? e.message : 'import_invalid'
    return toExec(errOutcome('config', code, `import rejected: ${code}`))
  }
  if (args[2] === '--check') {
    return toExec(okOutcome(importPreview(spec)))
  }
  try {
    const result = applyImport(spec)
    return toExec(okOutcome(result))
  } catch (e) {
    const code = e instanceof Error ? e.message : 'import_invalid'
    return toExec(errOutcome('config', code, `import rejected: ${code}`))
  }
}

/** `bin/rkp-mini-deployer --json <sub>`; the `deploy` word is gone from argv. */
function handleDeploy(args: string[]): ExecResult {
  const sub = args[0]
  // Module detection needs no configuration.
  if (sub === 'targets') {
    return modulesMode === 'fail'
      ? toExec(errOutcome('generic', 'io', 'read /data/adb/modules: permission denied'))
      : toExec(okOutcome({ builtin: detectedTargets() }))
  }
  const notInit = requireInit()
  if (notInit) return toExec(notInit)

  if (sub === 'add-target') {
    const id = args[1] ?? ''
    const d = BUILTIN_TARGETS.find((b) => b.id === id)
    if (!d) return toExec(errOutcome('config', 'usage', 'built-in targets: omk, teesim, tricky_store'))
    if (state.targets.some((t) => t.id === id)) {
      return toExec(errOutcome('config', 'duplicate_target_id', `duplicate target id \`${id}\``))
    }
    // `--path` mirrors the runtime: only a *.xml file directly inside /data/adb/teesim.
    const pathArg = flagValue(args, '--path')
    let path = d.path
    if (pathArg !== null) {
      if (id !== 'teesim' || !/^\/data\/adb\/teesim\/[A-Za-z0-9._-]{1,64}\.xml$/.test(pathArg)) {
        return toExec(errOutcome('config', 'config_invalid', `invalid --path for target \`${id}\``))
      }
      path = pathArg
    }
    state.targets.push({ id, path, owner: d.owner, mode: d.mode, reload: 'auto' })
    return toExec(okOutcome({ id, path, reload: 'auto' }))
  }

  if (sub === 'remove-target') {
    const id = args[1] ?? ''
    const before = state.targets.length
    state.targets = state.targets.filter((t) => t.id !== id)
    if (state.targets.length === before) {
      return toExec(errOutcome('config', 'unknown_target', `no target \`${id}\``))
    }
    return toExec(okOutcome({ id, removed: true }))
  }

  if (sub === 'remove') {
    if (!args.includes('--yes')) {
      return toExec(errOutcome('config', 'confirmation_required', 'removing deployed keyboxes needs --yes'))
    }
    const ids: string[] = []
    for (let i = 0; i < args.length; i++) {
      if (args[i] === '--target') ids.push(args[i + 1] ?? '')
    }
    const selected = ids.length ? state.targets.filter((t) => ids.includes(t.id)) : state.targets
    const reports: TargetReport[] = selected.map((t) => ({
      id: t.id,
      path: t.path,
      ok: true,
      sha256: null,
      reload: 'none',
      reload_status: null,
      error: null,
    }))
    state.cleared = [...(state.cleared ?? []), ...selected.map((t) => t.id)]
    if (selected.length >= state.targets.length) {
      state.deployed = null
      state.pending = null
    }
    return toExec(okOutcome({ targets: reports }))
  }

  if (sub !== 'plan' && sub !== 'apply') {
    return toExec(errOutcome('config', 'usage', `unknown deployer command \`${sub ?? ''}\``))
  }
  if (!state.stream_id) {
    return toExec(errOutcome('config', 'stream_missing', 'no sync stream configured; run `rkp-mini-sync import`'))
  }
  if (!state.staged) {
    return toExec(errOutcome('state', 'nothing_staged', 'no verified keybox yet; run `rkp-mini-sync fetch` first'))
  }

  const allow = args.includes('--allow-identity-change')
  const confirmFirst = flagValue(args, '--confirm-first')
  const allowSha = flagValue(args, '--allow-identity-change')

  if (sub === 'plan') {
    return toExec(okOutcome(planData(allow)))
  }
  if (!state.enabled) {
    return toExec(errOutcome('config', 'sync_disabled', 'remote sync is disabled'))
  }

  let plan: DeployPlan
  try {
    plan = planData(allow)
  } catch {
    return toExec(errOutcome('state', 'nothing_staged', 'no verified keybox yet'))
  }
  const reviewed = confirmFirst ?? allowSha
  if (reviewed !== null && !isShaPrefix(plan.staged.keybox_sha256, reviewed)) {
    return toExec(
      errOutcome(
        'state',
        'confirmation_mismatch',
        'the staged keybox changed since it was reviewed; review it again',
        plan,
      ),
    )
  }
  if (plan.gate.kind === 'first_deploy' && confirmFirst === null) {
    return toExec(okOutcome(holdPending('first_deploy', plan.gate)))
  }
  if (plan.gate.kind === 'blocked' && !allow) {
    return toExec(okOutcome(holdPending('identity_change', plan.gate)))
  }
  const { report, failed } = applyPrepared(plan)
  const failure = failed.length
    ? errOutcome(
        'deploy',
        'target_failed',
        `deployment failed for: ${failed.map((f) => f.id).join(', ')}`,
        report,
      )
    : okOutcome(report)
  return toExec(failure)
}

function flagValue(args: string[], flag: string): string | null {
  const i = args.indexOf(flag)
  return i >= 0 ? (args[i + 1] ?? null) : null
}

function handleConfig(args: string[]): ExecResult | null {
  if (args[0] !== 'config') return null
  const notInit = requireInit()
  if (notInit) return toExec(notInit)
  if (args[1] === 'show') return toExec(okOutcome(buildConfig()))
  if (args[1] !== 'set') return toExec(errOutcome('config', 'usage', 'config show | config set <key> <value>'))

  const key = args[2] ?? ''
  const value = args[3] ?? ''
  try {
    switch (key) {
      case 'label':
        state.label = value
        break
      case 'proxy':
        state.proxy = value
        break
      case 'min_validity_days':
      case 'rotation_lead_days':
      case 'request_timeout_seconds': {
        if (!/^\d+$/.test(value)) throw new Error(`\`${key}\` needs a non-negative integer`)
        const n = Number(value)
        if (key === 'min_validity_days') state.min_validity_days = n
        else if (key === 'rotation_lead_days') state.rotation_lead_days = n
        else state.request_timeout_seconds = n
        break
      }
      case 'auto_deploy':
        if (value !== 'true' && value !== 'false') throw new Error('`auto_deploy` needs true or false')
        state.auto_deploy = value === 'true'
        break
      case 'password_mode':
        if (value !== 'secret-file' && value !== 'prompt') throw new Error('`password_mode` needs secret-file or prompt')
        state.password_mode = value
        break
      case 'identity_change_policy':
        if (value !== 'scheduled' && value !== 'always' && value !== 'never') {
          throw new Error('`identity_change_policy` needs scheduled, always or never')
        }
        state.identity_change_policy = value
        break
      default:
        return toExec(errOutcome('config', 'key_not_settable', `\`${key}\` cannot be changed with config set`))
    }
  } catch (e) {
    return toExec(errOutcome('config', 'config_invalid', e instanceof Error ? e.message : String(e)))
  }
  return toExec(okOutcome({ key, value }))
}

function handleSecret(command: string): ExecResult | null {
  if (!command.includes('secret set ')) return null
  const kind = /secret set (age|gist-token|webdav-password)\b/.exec(command)?.[1]
  const hex = /echo ([0-9a-f]+) >/.exec(command)?.[1] ?? ''
  if (kind === undefined) return toExec(errOutcome('config', 'usage', 'unknown secret kind'))
  if (kind === 'gist-token') {
    return toExec(errOutcome('config', 'usage', 'receivers read Gists without a token'))
  }
  const notInit = requireInit()
  if (notInit) return toExec(notInit)
  if (kind === 'age' && state.password_mode !== 'secret-file') {
    return toExec(errOutcome('config', 'usage', 'password_mode is "prompt"; nothing is stored'))
  }
  const decoded = hex.length / 2
  if (decoded === 0) return toExec(errOutcome('config', 'secret_empty', 'secret is empty'))
  if (kind === 'age') state.age_present = true
  else state.webdav_password_present = true
  const path = kind === 'age' ? `${DATA}/secrets/age.pass` : `${DATA}/secrets/webdav.pass`
  return toExec(okOutcome({ path, mode: '0600', weak: kind === 'age' && decoded < 12 }))
}

// ---- Runner state files (tmp/sync-task.*) ------------------------------------------

interface RunnerFiles {
  lock: number | null
  code: number | null
  end: number | null
  op: string
  json: string
  err: string
  /** Combined-run snapshot (`sync-task.last.steps`); empty for a single fetch. */
  steps: string
}

const RUNNER_KEY = 'rkp.mock.receiver.runner'

function loadFiles(): RunnerFiles {
  const empty: RunnerFiles = { lock: null, code: null, end: null, op: '', json: '', err: '', steps: '' }
  let saved: Partial<RunnerFiles> | null = null
  try {
    saved = JSON.parse(localStorage.getItem(RUNNER_KEY) ?? 'null') as Partial<RunnerFiles> | null
  } catch {
    saved = null
  }
  if (!saved) return empty
  const merged = { ...empty, ...saved }
  // A task that was "running" when the page closed is gone in the preview
  // (no timers survive a reload): report it as an interrupted launch, not as
  // a task that keeps running forever.
  if (merged.lock !== null && merged.code === null) merged.lock = null
  return merged
}

/**
 * Persisted so reopening the page can still read the last finished task, as
 * the module's state files do on a device.
 */
const files: RunnerFiles = new Proxy(loadFiles(), {
  set(target, key, value) {
    ;(target as unknown as Record<string, unknown>)[key as string] = value
    try {
      localStorage.setItem(RUNNER_KEY, JSON.stringify(target))
    } catch {
      /* preview only */
    }
    return true
  },
})

function launch(taskArgs: string[]): void {
  if (files.lock !== null) return
  setTimeout(() => {
    files.lock = nowUnix()
    files.code = files.end = null
    files.op = taskArgs[0] ?? ''
    files.json = files.err = files.steps = ''
  }, 300)
  setTimeout(() => {
    if (files.op === 'run') {
      const snap = buildRunSnapshot()
      files.op = snap.op
      files.json = snap.json
      files.err = snap.err
      files.code = snap.code
      files.steps = snap.steps
    } else {
      const outcome =
        files.op === 'fetch'
          ? doFetch(taskArgs[1])
          : files.op === 'check'
            ? decryptFails
              ? errOutcome('decrypt', 'decrypt_failed', 'mock: age decryption failed')
              : okOutcome(runRemoteCheck())
            : errOutcome('config', 'usage', `unknown sync task \`${files.op}\``)
      const exec = toExec(outcome)
      files.json = exec.stdout
      files.err = exec.stderr
      files.code = exec.errno
      files.steps = ''
    }
    files.end = nowUnix()
    files.lock = null
  }, 2000)
}

function probeOutput(): string {
  const lines = [`now=${nowUnix()}`]
  if (files.lock !== null) lines.push(`lock=${files.lock}`)
  if (files.code !== null) lines.push(`code=${files.code}`, `end=${files.end}`)
  return lines.join('\n') + '\n'
}

function logText(): string {
  const at = (offset: number): string =>
    new Date((nowUnix() - offset) * 1000).toISOString().replace('T', ' ').slice(0, 19) + ' UTC'
  const hash = (value: string | null | undefined): string => (value ? value.slice(0, 12) : '—')
  const lines = [
    '# rkp-mini-sync receiver log (seq/provider/hash/codes only)',
    `${at(7200)} fetch provider=${state.provider || '-'} seq=${state.sync_state?.seq ?? 0} keybox=${hash(state.staged?.keybox_sha256)} changed=1`,
    `${at(7100)} deploy seq=${state.deployed?.seq ?? 0} keybox=${hash(state.deployed?.keybox_sha256)} targets=${state.targets.map((t) => `${t.id}:${t.reload_status ?? 'ok'}`).join(',') || 'none'}`,
    `${at(3600)} note: certificate signatures, root trust and revocation are not verified`,
  ]
  if (state.last_error) lines.push(`${at(600)} error ${state.last_error.code}: ${state.last_error.message}`)
  return lines.join('\n') + '\n'
}

/** Minimal single-quote-aware argv splitter; never used for secrets. */
function shellArgs(command: string): string[] {
  const out: string[] = []
  let i = 0
  while (i < command.length) {
    const c = command[i]
    if (c === ' ') {
      i++
      continue
    }
    if (c === "'") {
      i++
      let value = ''
      while (i < command.length) {
        if (command[i] === "'") {
          if (command[i + 1] === '\\' && command[i + 2] === "'") {
            value += "'"
            i += 3
          } else {
            i++
            break
          }
        } else {
          value += command[i]
          i++
        }
      }
      out.push(value)
    } else {
      let value = ''
      while (i < command.length && command[i] !== ' ') value += command[i++]
      out.push(value)
    }
  }
  return out
}

function safeCommand(command: string): string {
  // Never print the hex payload of a secret write, however short it is.
  return command.replace(/(echo\s+)[0-9a-f]+/g, '$1<hex>')
}

export async function mockExec(command: string): Promise<ExecResult> {
  await new Promise((resolve) => setTimeout(resolve, 120))
  console.debug('[mock receiver]', safeCommand(command))

  if (command.includes('runner/sync.sh')) {
    const at = command.indexOf("sync.sh'")
    if (at < 0) return { errno: 127, stdout: '', stderr: 'mock: bad runner command' }
    // Only the first branch (setsid) carries the task arguments; the nohup
    // fallback repeats them after `</dev/null`.
    const end = command.indexOf('</dev/null', at)
    const tail = command.slice(at + "sync.sh'".length, end < 0 ? undefined : end)
    launch([...tail.matchAll(/'([^']*)'/g)].map((m) => m[1]))
    return { errno: 0, stdout: 'setsid\n', stderr: '' }
  }
  if (command.includes('sync-task.lock')) return { errno: 0, stdout: probeOutput(), stderr: '' }
  if (command.includes('sync-task.last.steps')) {
    return { errno: 0, stdout: files.steps, stderr: '' }
  }
  if (command.includes('sync-task.last.op')) {
    if (!command.includes('sync-task.last.json')) {
      // lib/run.ts reads `last.op` on its own.
      return { errno: 0, stdout: `${files.op}\n`, stderr: '' }
    }
    const sep = '<<<RKP-SEP>>>'
    return {
      errno: 0,
      stdout: `${files.op}\n${sep}\n${files.json}${sep}\n${files.err}`,
      stderr: '',
    }
  }
  if (command.includes('sync-loop.sh')) {
    state.loop_running = true
    return { errno: 0, stdout: 'setsid\n', stderr: '' }
  }

  const secret = handleSecret(command)
  if (secret) return secret

  if (command.startsWith('rm -f ')) return { errno: 0, stdout: '', stderr: '' }
  if (command.startsWith('tail -n ')) return { errno: 0, stdout: logText(), stderr: '' }
  // 更多 tab: read-only config, module.prop and the deployer's --version.
  if (command.startsWith('cat ') && command.includes('/config.toml')) {
    return { errno: 0, stdout: `[sync]\nrole = "receiver"\nenabled = true\nprovider = "gist"\nlabel = "phone-a"\n\n[deploy]\nidentity_change_policy = "scheduled"\nrotation_lead_days = 4\nauto_deploy = true\n`, stderr: '' }
  }
  if (command.startsWith('cat ') && command.includes('/module.prop')) {
    return { errno: 0, stdout: 'id=rkp_mini_deployer\nname=RKP-Mini Deployer\nversion=v0.3.1\nversionCode=4\n', stderr: '' }
  }
  if (command.endsWith(' --version')) return { errno: 0, stdout: 'rkp-mini-deployer 0.3.1\n', stderr: '' }

  const args = shellArgs(command)
  const jsonAt = args.indexOf('--json')
  if (jsonAt < 0) return { errno: 127, stdout: '', stderr: `mock: unhandled command: ${safeCommand(command)}` }
  const rest = args.slice(jsonAt + 1)

  // `bin/rkp-mini-deployer` is a separate local-only binary whose argv carries
  // no `deploy` word: the binary picks the handler, not the verb.
  if (command.includes('/bin/rkp-mini-deployer')) return handleDeploy(rest)
  if (!command.includes('/bin/rkp-mini-sync')) {
    return { errno: 127, stdout: '', stderr: `mock: unhandled binary: ${safeCommand(command)}` }
  }

  const verb = rest[0]

  if (verb === 'status') {
    if (!initialized) {
      return toExec(
        errOutcome('config', 'not_initialized', 'no rkp-mini-sync configuration found; run `rkp-mini-sync init --role receiver`'),
      )
    }
    return toExec(okOutcome(buildStatus()))
  }
  if (verb === 'risk') return toExec(okOutcome(buildRisk()))
  if (verb === 'enable') {
    const notInit = requireInit()
    if (notInit) return toExec(notInit)
    const accepted = Number(rest[rest.indexOf('--accept-risk') + 1])
    if (accepted !== 1) {
      return toExec(errOutcome('config', 'risk_version_mismatch', '--accept-risk does not match the current statement'))
    }
    state.risk_ack = true
    state.enabled = true
    state.loop_running = true
    return toExec(okOutcome({ enabled: true, risk_ack_version: 2, role: 'receiver' }))
  }
  if (verb === 'disable') {
    const notInit = requireInit()
    if (notInit) return toExec(notInit)
    state.enabled = false
    state.loop_running = false
    return toExec(okOutcome({ enabled: false }))
  }
  if (verb === 'forget') {
    const notInit = requireInit()
    if (notInit) return toExec(notInit)
    const staging = rest.includes('--staging')
    const removed = [
      `${DATA}/secrets/age.pass`,
      `${DATA}/secrets/webdav.pass`,
      `${DATA}/sync/sync-state.json`,
      `${DATA}/deploy/deploy-state.json`,
    ]
    state.enabled = false
    state.loop_running = false
    state.age_present = false
    state.webdav_password_present = false
    remoteCheckedAt = null
    state.sync_state = null
    state.deployed = null
    state.pending = null
    state.cleared = []
    state.last_error = null
    state.provider = ''
    state.location = ''
    state.stream_id = ''
    state.label = ''
    state.gist_id = ''
    state.webdav_endpoint = ''
    state.webdav_path = ''
    state.webdav_user = ''
    state.url = ''
    if (staging) {
      state.staged = null
      removed.push(`${DATA}/staging`)
    }
    return toExec(okOutcome({ removed, enabled: false }))
  }

  const imported = handleImport(rest)
  if (imported) return imported
  const configured = handleConfig(rest)
  if (configured) return configured

  return { errno: 127, stdout: '', stderr: `mock: unhandled command: ${safeCommand(command)}` }
}
