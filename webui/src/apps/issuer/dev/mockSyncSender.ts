// Dev-only mock for the sender's rkp-mini-sync commands (routed from mock.ts).
// Query parameters:
//   ?sync=     uninit | disabled | ready | error | nosecret      (sync state)
//   ?syncpush= ok | noop | fail                     (result of one push task)
//
// Secrets: the WebUI writes them as `echo <hex> > .../sync/inbox/<kind>.hex`.
// This mock never reads, stores or logs that hex; it only flips the presence
// flag the page displays. The dispatch log in mock.ts redacts it too.
import type { ExecResult } from '../../../shared/lib/ksu'
import type { PushResult, RiskStatement, SenderConfigKey, ShareResult, SyncConfig, SyncState, SyncStatus } from '../../../shared/lib/sync'
import { nowUnix } from '../../../shared/lib/format'
import readyStatus from './fixtures/sync/sender-status-ready.json'
import disabledStatus from './fixtures/sync/sender-status-disabled.json'
import errorStatus from './fixtures/sync/sender-status-error.json'
import noSecretStatus from './fixtures/sync/sender-status-nosecret.json'
import baseConfig from './fixtures/sync/sender-config.json'
import riskFixture from './fixtures/sync/sender-risk.json'
import shareFixture from './fixtures/sync/sender-share.json'
import pushOk from './fixtures/sync/sender-push-ok.json'
import pushNoop from './fixtures/sync/sender-push-noop.json'
import pushFail from './fixtures/sync/sender-push-fail.json'
import rotateOk from './fixtures/sync/sender-rotate-ok.json'
import logText from './fixtures/sync/sender-sync.log?raw'

const SEP = '<<<RKP-SEP>>>'
const SYNC_SECRETS = '/data/misc/keystore/rkp-mini/sync/secrets'
const RISK = riskFixture as unknown as RiskStatement

const params = new URLSearchParams(location.search)
const scenario = params.get('sync') ?? 'ready'
const pushOutcome = params.get('syncpush') ?? 'ok'

const STATUS: Record<string, SyncStatus> = {
  ready: readyStatus as unknown as SyncStatus,
  disabled: disabledStatus as unknown as SyncStatus,
  error: errorStatus as unknown as SyncStatus,
  nosecret: noSecretStatus as unknown as SyncStatus,
}

/** Shared with mock.ts: the issuer page merges this into its own status. */
export const mockLoop = { running: false }

/** Fixture with all timestamps re-anchored to the current time. */
function liveStatus(base: SyncStatus): SyncStatus {
  const s = structuredClone(base)
  const now = nowUnix()
  if (s.sync_state) {
    s.sync_state.verified_at = now - 600
    s.sync_state.last_checked_at = now - 300
    s.sync_state.keybox_not_after = now + 10 * 86_400
    if (s.sync_state.last_error) s.sync_state.last_error.at = now - 3600
  }
  if (s.local_keybox) {
    s.local_keybox.leaf_not_after = now + 10 * 86_400
    s.local_keybox.days_left = 10
  }
  return s
}

let initialized = scenario !== 'uninit'
let status = liveStatus(STATUS[scenario] ?? STATUS.ready)
const config = structuredClone(baseConfig) as unknown as SyncConfig

// ---- Envelopes -------------------------------------------------------------------

function okJson(data: unknown): ExecResult {
  return { errno: 0, stdout: `${JSON.stringify({ ok: true, data })}\n`, stderr: '' }
}

function errJson(errno: number, stage: string, code: string, message: string): ExecResult {
  return { errno, stdout: `${JSON.stringify({ ok: false, error: { stage, code, message } })}\n`, stderr: '' }
}

/** Everything after `--json` (`status`, `config show`, `push --accept-first …`). */
function rest(command: string): string {
  const i = command.indexOf(' --json ')
  return i >= 0 ? command.slice(i + 7).trim() : ''
}

// ---- Runner state files (tmp/sync-task.*) ----------------------------------------

interface RunnerFiles {
  lock: number | null
  code: number | null
  end: number | null
  json: string
  err: string
  op: string | null
}

const files: RunnerFiles = { lock: null, code: null, end: null, json: '', err: '', op: null }

function applyPush(): void {
  const { data } = pushOk as unknown as { data: PushResult }
  const now = nowUnix()
  const state: SyncState = {
    v: 1,
    provider: data.provider,
    location: data.url,
    stream_id: data.stream_id,
    verified_seq: data.seq,
    plaintext_sha256: data.plaintext_sha256,
    keybox_sha256: data.keybox_sha256,
    device_id: data.device_id,
    keybox_not_after: data.leaf_not_after,
    last_remote_token: 'mock-revision',
    last_checked_at: now,
    verified_at: now,
    last_error: null,
  }
  status.sync_state = state
  status.location = data.url
  status.enabled = true
  status.loop_running = true
}

function taskResult(op: string): { json: string; err: string; code: number } {
  if (op === 'rotate') {
    status.secrets.age = { path: `${SYNC_SECRETS}/age.pass`, present: true, mode: '0600', ok: true }
    return { json: `${JSON.stringify(rotateOk)}\n`, err: '', code: 0 }
  }
  if (op === 'push') {
    if (pushOutcome === 'fail') {
      return { json: `${JSON.stringify(pushFail)}\n`, err: 'rkp-mini-sync: remote: auth_failed\n', code: 8 }
    }
    if (pushOutcome === 'ok') applyPush()
    return { json: `${JSON.stringify(pushOutcome === 'noop' ? pushNoop : pushOk)}\n`, err: '', code: 0 }
  }
  return { json: `${JSON.stringify({ ok: true, data: {} })}\n`, err: '', code: 0 }
}

function launch(op: string): void {
  if (files.lock !== null) return // runner exits 9: a task is already running
  files.op = op
  setTimeout(() => {
    files.lock = nowUnix()
    files.code = files.end = null
    files.json = files.err = ''
  }, 300)
  setTimeout(() => {
    const r = taskResult(op)
    files.json = r.json
    files.err = r.err
    files.code = r.code
    files.end = nowUnix()
    files.lock = null
  }, 3200)
}

function probeOutput(): string {
  const lines = [`now=${nowUnix()}`]
  if (files.lock !== null) lines.push(`lock=${files.lock}`)
  if (files.code !== null) lines.push(`code=${files.code}`, `end=${files.end}`)
  return `${lines.join('\n')}\n`
}

// ---- rkp-mini-sync commands --------------------------------------------------------

const NUMERIC: SenderConfigKey[] = ['min_validity_days', 'request_timeout_seconds']

function dispatch(sub: string, command: string): ExecResult {
  if (sub === 'status') {
    return initialized ? okJson(status) : errJson(2, 'config', 'not_initialized', 'sync is not initialized yet')
  }

  if (sub === 'risk') {
    return okJson({ ...RISK, accepted_version: status.risk_ack_current ? status.risk_version : null })
  }

  if (/^init --provider/.test(sub)) {
    const provider = /--provider 'webdav'/.test(sub) ? 'webdav' : 'gist'
    initialized = true
    status = liveStatus(STATUS.disabled)
    status.provider = provider
    status.location = null
    config.provider = provider
    return okJson({ initialized: true, provider })
  }

  const enable = /^enable --accept-risk (\d+)$/.exec(sub)
  if (enable) {
    if (Number(enable[1]) !== RISK.version) {
      return errJson(2, 'config', 'risk_version_mismatch', `risk statement is v${RISK.version}`)
    }
    status.enabled = true
    status.risk_ack_version = RISK.version
    status.risk_ack_current = true
    status.loop_running = true
    return okJson({ enabled: true })
  }

  if (sub === 'disable') {
    status.enabled = false
    status.loop_running = false
    return okJson({ enabled: false })
  }

  if (sub.startsWith('forget --yes')) {
    initialized = false
    status.enabled = false
    status.sync_state = null
    return okJson({
      removed: ['sync-state.json', 'sync/secrets/age.pass', 'sync/secrets/gist.token'],
      enabled: false,
    })
  }

  if (sub === 'config show') {
    config.enabled = status.enabled
    config.provider = status.provider
    return okJson(config)
  }

  const set = /^config set '([a-z_]+)' '(.*)'$/.exec(sub)
  if (set) {
    const key = set[1] as SenderConfigKey
    const raw = set[2].replace(/'\\''/g, "'")
    if (key === 'provider' && raw !== 'gist' && raw !== 'webdav') {
      return errJson(2, 'config', 'config_invalid', `provider must be gist or webdav`)
    }
    const value = NUMERIC.includes(key) ? Number(raw) : raw
    if (typeof value === 'number' && (!Number.isFinite(value) || value < 0)) {
      return errJson(2, 'config', 'config_invalid', `${key} must be a non-negative integer`)
    }
    ;(config as unknown as Record<string, string | number>)[key] = value
    if (key === 'provider') {
      status.provider = raw
      status.location = null
    }
    return okJson({ key, value })
  }

  const secret = /secret set (age|gist-token|webdav-password) --from-file/.exec(sub)
  if (secret) {
    const kind = secret[1]
    const file = kind === 'gist-token' ? 'gist.token' : kind === 'webdav-password' ? 'webdav.pass' : 'age.pass'
    const slot = kind === 'gist-token' ? 'gist_token' : kind === 'webdav-password' ? 'webdav_password' : 'age'
    // Only the length of the hex is inspected; the value itself is discarded.
    const hex = /echo ([0-9a-f]+) >/.exec(command)?.[1] ?? ''
    status.secrets[slot] = { path: `${SYNC_SECRETS}/${file}`, present: true, mode: '0600', ok: true }
    return okJson({ path: `${SYNC_SECRETS}/${file}`, mode: '0600', weak: hex.length < 40 })
  }

  if (sub === 'share' || sub.startsWith('share --label')) {
    if (!status.enabled || !status.sync_state) {
      return errJson(8, 'remote', 'not_published', 'nothing pushed to the remote yet')
    }
    return okJson(structuredClone(shareFixture) as unknown as ShareResult)
  }

  // Receiver-only commands never run in this build; report them instead of faking.
  return errJson(2, 'generic', 'read_only_provider', `sender mock: unsupported \`${sub.split(' ')[0] || '?'}\``)
}

// ---- Entry point -------------------------------------------------------------------

export async function mockSync(command: string): Promise<ExecResult> {
  // Executor form: the shared tsconfig targets es2022, where Promise.withResolvers
  // does not exist (same reason as lib/ksu.ts).
  await new Promise((resolve) => setTimeout(resolve, 120))

  if (command.includes('runner/sync.sh')) {
    const op = /runner\/sync\.sh' '([a-z]+)'/.exec(command)?.[1] ?? 'push'
    launch(op)
    return okJson(command.includes('command -v setsid') ? 'setsid' : '')
  }
  if (command.includes('sync-task.last') || command.includes("'sync-task.lock'")) {
    // Only the probe command carries the quoted lock path; the result reader
    // carries sync-task.last.* and must not be mistaken for it.
    if (command.includes("'sync-task.lock'")) {
      return { errno: 0, stdout: probeOutput(), stderr: '' }
    }
    return { errno: 0, stdout: `${files.op ?? ''}\n${SEP}\n${files.json}${SEP}\n${files.err}`, stderr: '' }
  }
  if (command.includes('logs/auto.log')) {
    return { errno: 0, stdout: logText, stderr: '' }
  }
  if (command.includes('auto-loop.sh')) {
    mockLoop.running = true
    status.loop_running = true
    return okJson({ loop: 'started' })
  }
  // prepareRotation / discardRotation write the inbox file only; `secret set`
  // also writes one but then runs the binary, so the binary check comes first.
  if (!command.includes('/bin/rkp-mini-sync') && command.includes('sync/inbox')) {
    return { errno: 0, stdout: '', stderr: '' }
  }
  if (command.includes('/bin/rkp-mini-sync')) {
    return dispatch(rest(command), command)
  }

  return { errno: 127, stdout: '', stderr: `mock: unhandled sync command: ${command}` }
}
