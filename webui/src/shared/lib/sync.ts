// rkp-mini-sync WebUI contract (design docs/REMOTE_SYNC_DEVELOP.md §19).
// Single source of every path, command, JSON type and label used by
// both apps. The role and paths come from `@app-config`, so one module serves
// the sender (rkp_mini_issuer) and receiver (rkp_mini_deployer) pages.
//
// Two binaries with disjoint jobs (they never call each other):
//   bin/rkp-mini-sync      every remote command (init/push/share/import/fetch/…)
//   bin/rkp-mini-deployer  local plan/apply/add-target/remove-target/remove;
//                          no network I/O and no age passphrase file.
//                          Its commands live in apps/deployer/lib/deploy.ts.
//
// Secrets: only `setSecret` / `prepareRotation` handle them. They hex-encode
// the value and write it with shell builtins (`umask`, `echo`, `>`) into a
// 0600 inbox file; `ksu.exec` delivers the command over the root shell's
// stdin, so the secret never appears in any process argv. rkp-mini-sync
// consumes and deletes the inbox file. Nothing here ever reads a secret back.
import { appConfig } from '@app-config'
import { exec } from './ksu'
import { CommandError, SAFE_PATH, parseLastJsonLine, resolveModuleDir } from './command'
import { useTaskRunner, type TaskResult, type TaskRunner } from './runner'
import { t, tCode } from './i18n'

// ---- Role and paths --------------------------------------------------------------

export type SyncRole = 'sender' | 'receiver'

/** Fixed per build by the app's `config.ts` (`@app-config`). */
export const SYNC_ROLE: SyncRole = appConfig.syncRole

export const SYNC_MODULE_DIR = resolveModuleDir()
export const SYNC_BIN = `${SYNC_MODULE_DIR}/bin/rkp-mini-sync`
export const SYNC_DATA = appConfig.dataRoot
export const SYNC_CONFIG = `${SYNC_DATA}/config.toml`
export const SYNC_TMP = `${SYNC_DATA}/tmp`
export const SYNC_INBOX = `${SYNC_DATA}/sync/inbox`
export const SYNC_LOG = `${SYNC_DATA}/logs/${appConfig.loopLog}`
/** Path shown in the UI, e.g. `logs/auto.log`. */
export const SYNC_LOG_LABEL = `logs/${appConfig.loopLog}`
const SYNC_LOOP = `${SYNC_MODULE_DIR}/${appConfig.loopScript}`
const SYNC_RUNNER = `${SYNC_MODULE_DIR}/webroot/runner/sync.sh`

for (const p of [SYNC_BIN, SYNC_CONFIG, SYNC_INBOX, SYNC_LOG, SYNC_LOOP, SYNC_RUNNER]) {
  if (!SAFE_PATH.test(p)) throw new Error(`unsafe path: ${p}`)
}

// ---- JSON contract ---------------------------------------------------------------

export interface SyncErrorInfo {
  stage: 'remote' | 'decrypt' | 'validate' | 'state' | 'deploy' | 'config' | 'generic' | string
  code: string
  message: string
}

/** `data` may accompany an error (TOFU facts, partial deploy, mismatched plan). */
export type SyncEnvelope<T> = { ok: true; data: T } | { ok: false; error: SyncErrorInfo; data?: unknown }

export interface SecretStatus {
  path: string
  present: boolean
  /** e.g. "0600"; absent when missing. */
  mode?: string
  ok: boolean
}

export interface LastError {
  code: string
  message: string
  at: number
}

export interface SyncState {
  v: number
  provider: string
  location: string
  stream_id: string
  verified_seq: number
  plaintext_sha256: string | null
  keybox_sha256: string | null
  device_id: string | null
  keybox_not_after: number | null
  last_remote_token: string | null
  last_checked_at: number | null
  verified_at: number | null
  last_error: LastError | null
}

export interface Staged {
  v: number
  stream_id: string
  seq: number
  keybox_sha256: string
  plaintext_sha256: string
  device_id: string
  leaf_not_before: number
  leaf_not_after: number
  staged_at: number
}

export interface TargetState {
  id: string
  path: string
  owner: string
  mode: string
  deployed_sha256: string | null
  deployed_at: number | null
  reload: string
  reload_status: string | null
  last_error: LastError | null
}

export interface Pending {
  seq: number
  keybox_sha256: string
  leaf_not_after: number
  /** `first_deploy` | `identity_change` */
  reason: string
  at: number
}

export interface DeployState {
  v: number
  stream_id: string
  last_seen_keybox_sha256: string | null
  last_seen_seq: number | null
  deployed_keybox_sha256: string | null
  deployed_seq: number | null
  deployed_at: number | null
  deployed_leaf_not_after: number | null
  targets: TargetState[]
  pending: Pending | null
}

/** `check-remote` outcome, first match wins (design §13.3). */
export type CheckOutcome = 'in_sync' | 'remote_behind' | 'remote_ahead' | 'diverged' | 'missing' | 'error'

/** Head record of the remote file as read by `check-remote`. */
export interface RemoteFact {
  seq: number
  plaintext_sha256: string | null
  keybox_sha256: string | null
  device_id: string | null
  issued_at: number | null
  keybox_not_after: number | null
}

/** `<root>/sync/remote-check.json`, surfaced as `status.remote_check`. */
export interface RemoteCheck {
  at: number
  role: SyncRole
  outcome: CheckOutcome
  remote: RemoteFact | null
  local: { seq: number | null; keybox_sha256: string | null }
  error: SyncErrorInfo | null
}

export interface LocalKeybox {
  path: string
  valid: boolean
  device_id?: string
  keybox_sha256?: string
  leaf_not_after?: number
  days_left?: number
  summary: string
}

/** `status --json` (local only; never contacts the remote). */
export interface SyncStatus {
  role: SyncRole
  config: string
  enabled: boolean
  risk_ack_version: number
  risk_ack_current: boolean
  risk_version: number
  loop_running: boolean
  provider: string
  /** Credential-free display location, null when not configured. */
  location: string | null
  stream_id: string
  password_mode: 'secret-file' | 'prompt'
  secrets: { age?: SecretStatus; gist_token?: SecretStatus; webdav_password?: SecretStatus }
  sync_state: SyncState | null
  /** Cached `check-remote` record (both roles); null when it never ran. */
  remote_check: RemoteCheck | null
  // sender
  auto_push?: boolean
  local_keybox?: LocalKeybox
  // receiver
  deploy_state?: DeployState | null
  staged?: Staged | null
  targets?: { id: string; path: string }[] | { error: string }
  identity_change_policy?: IdentityPolicy
}

export interface RiskItem {
  id: string
  en: string
  zh: string
}

export interface RiskStatement {
  version: number
  items: RiskItem[]
  passphrase_path: string | null
  accepted_version: number | null
}

export type IdentityPolicy = 'scheduled' | 'always' | 'never'

export interface TargetConfig {
  id: string
  path: string | null
  owner: string | null
  mode: string | null
  reload: string | null
}

/** `config show --json`. Secret *paths* only, never values. */
export interface SyncConfig {
  role: SyncRole
  config: string
  enabled: boolean
  risk_ack_version: number
  risk_version: number
  provider: string
  stream_id: string
  label: string
  min_validity_days: number
  request_timeout_seconds: number
  proxy: string
  gist_id: string
  gist_filename: string
  gist_url: string
  webdav_endpoint: string
  webdav_path: string
  webdav_user: string
  url: string
  password_mode: 'secret-file' | 'prompt'
  secret_files: { age: string; gist_token: string; webdav_password: string }
  inbox: string
  // sender
  output_dir?: string
  auto_push?: boolean
  // receiver
  identity_change_policy?: IdentityPolicy
  rotation_lead_days?: number
  auto_deploy?: boolean
  targets?: TargetConfig[]
}

/** Keys accepted by `config set` (mirrors rkp-mini-sync's allow-list). */
export type SenderConfigKey =
  | 'provider'
  | 'label'
  | 'min_validity_days'
  | 'request_timeout_seconds'
  | 'proxy'
  | 'gist_id'
  | 'gist_filename'
  | 'webdav_endpoint'
  | 'webdav_path'
  | 'webdav_user'
  | 'password_mode'
  | 'auto_push'
export type ReceiverConfigKey =
  | 'label'
  | 'min_validity_days'
  | 'request_timeout_seconds'
  | 'proxy'
  | 'password_mode'
  | 'identity_change_policy'
  | 'rotation_lead_days'
  | 'auto_deploy'

export interface PushResult {
  provider: string
  stream_id: string
  seq: number
  noop: boolean
  ciphertext_sha256: string | null
  plaintext_sha256: string
  keybox_sha256: string
  device_id: string
  leaf_not_after: number
  url: string
  remote_seq: number | null
  /** `none` | `remote` | `local` */
  base: string
  uploaded_at: number | null
}

export interface ShareResult {
  uri: string
  json: Record<string, unknown>
  contains_password: false
  /** QR module matrix: `size` rows of `size` chars, '1' = dark. */
  qr: { size: number; rows: string[] }
}

export type LocationInfo =
  | { provider: 'gist'; url: string | null; gist_id: string | null; filename: string }
  | { provider: 'webdav'; endpoint: string; path: string; user: string | null }
  | { provider: 'url'; url: string }

/** `import --check` (writes nothing). */
export interface ImportPreview {
  location: LocationInfo
  display: string
  stream_id: string
  label: string | null
  current_stream_id: string | null
  stream_change: boolean
  /** Importing would replace a stream that already has verified versions. */
  blocked: boolean
  verified_seq: number | null
}

export interface ImportResult {
  provider: string
  location: string
  stream_id: string
  label: string | null
  enabled: boolean
  test: unknown
  notes: string[]
}

/** `fetch` success data. */
export interface FetchResult {
  provider: string
  stream_id: string
  changed: boolean
  seq: number
  plaintext_sha256?: string
  keybox_sha256?: string
  device_id?: string
  leaf_not_after?: number
  staged_path?: string
  previous_seq?: number | null
  gap?: boolean
  first_import?: boolean
  warnings?: { code: string; message: string }[]
  root_trust_checked?: false
}

/** Error `data` of `first_import_needs_confirmation` (trust on first use). */
export interface TofuFacts {
  provider: string
  location: string
  stream_id: string
  seq: number
  issued_at: number
  device_id: string
  leaf_not_before: number
  leaf_not_after: number
  keybox_sha256: string
  /** Pass this back as `--accept-first`. */
  plaintext_sha256: string
  targets: string[]
  root_trust_checked: false
}

export type Gate =
  | { kind: 'same' }
  | { kind: 'first_deploy' }
  | { kind: 'allowed'; reason: string }
  | { kind: 'blocked'; reason: string }

export interface PlanWarning {
  /**
   * `omk_key_retirement` | `omk_incompatible` | `teesim_reattest` |
   * `teesim_config_missing` | `no_root_trust_check` | …
   */
  code: string
  message: string
}

/** `rkp-mini-deployer ... --json plan` (read-only). */
export interface DeployPlan {
  gate: Gate
  staged: Staged
  old_keybox_sha256: string | null
  old_leaf_not_after: number | null
  warnings: PlanWarning[]
  targets: { id: string; path: string; owner: string; mode: string; reload: string }[]
  root_trust_checked: false
}

export interface TargetReport {
  id: string
  path: string
  ok: boolean
  sha256: string | null
  reload: string
  reload_status: string | null
  error: { code: string; message: string } | null
}

/** `rkp-mini-deployer ... --json apply` data (also on `target_failed` errors). */
export interface DeployReport {
  keybox_sha256: string
  seq: number
  deployed: boolean
  pending: Pending | null
  warnings: PlanWarning[]
  targets: TargetReport[]
}

// ---- Command plumbing --------------------------------------------------------------

/** Single-quote a non-secret argument; newlines/control characters are refused. */
export function shq(value: string): string {
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(value)) throw new CommandError('exec', t('shared.command.controlChars'))
  return `'${value.replace(/'/g, `'\\''`)}'`
}

/** Every sync invocation: role and config are pinned here, never by callers. */
const PREFIX = `'${SYNC_BIN}' --role ${SYNC_ROLE} --config '${SYNC_CONFIG}' --json`

export interface SyncCall<T> {
  envelope: SyncEnvelope<T>
  errno: number
  stderr: string
}

async function invoke<T>(prefix: string, args: string, timeoutMs?: number): Promise<SyncCall<T>> {
  let r
  try {
    r = await exec(`${prefix} ${args}`, timeoutMs)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
  const envelope = parseLastJsonLine<SyncEnvelope<T>>(r.stdout)
  return { envelope, errno: r.errno, stderr: r.stderr }
}

/** Run `rkp-mini-sync <args>`; non-zero exits still return their envelope. */
async function call<T>(args: string, timeoutMs?: number): Promise<SyncCall<T>> {
  return invoke<T>(PREFIX, args, timeoutMs)
}

/** Like `call` but throws a CommandError on `ok: false`. */
async function data<T>(args: string, timeoutMs?: number): Promise<T> {
  const { envelope } = await call<T>(args, timeoutMs)
  if (!envelope.ok) {
    throw new CommandError('exit', syncErrorText(envelope.error), JSON.stringify(envelope.error))
  }
  return envelope.data
}

// ---- Commands (short; run through exec) ----------------------------------------------

/** Not initialized yet: `status` fails with `not_initialized`. */
export function readSyncStatus(): Promise<SyncCall<SyncStatus>> {
  return call<SyncStatus>('status')
}

export function readRisk(): Promise<RiskStatement> {
  return data<RiskStatement>('risk')
}

export function readSyncConfig(): Promise<SyncConfig> {
  return data<SyncConfig>('config show')
}

export function setConfig(key: SenderConfigKey | ReceiverConfigKey, value: string | number | boolean): Promise<unknown> {
  return data(`config set ${shq(key)} ${shq(String(value))}`)
}

/** Sender: add the disabled `[sync]` table to config.toml (`--role` comes from PREFIX). */
export function initSender(provider: 'gist' | 'webdav'): Promise<unknown> {
  return data(`init --provider ${shq(provider)}`)
}

/** `version` must be the `RiskStatement.version` the user just accepted. */
export function enableSync(version: number): Promise<unknown> {
  return data(`enable --accept-risk ${Math.trunc(version)}`)
}

export function disableSync(): Promise<unknown> {
  return data('disable')
}

/**
 * Both roles: remove local sync data (and the secrets) but never the remote file.
 * `reset` (sender only) also drops the whole `[sync]` table → back to uninitialized.
 */
export function forgetSync(staging: boolean, reset = false): Promise<{ removed: string[]; enabled: false; reset: boolean }> {
  return data(`forget --yes${staging ? ' --staging' : ''}${reset ? ' --reset' : ''}`)
}

export type SecretKind = 'age' | 'gist-token' | 'webdav-password'

const MAX_SECRET_BYTES = 4096

/** UTF-8 → lowercase hex; refuses empty, multi-line or oversize secrets. */
function secretHex(value: string): string {
  if (!value) throw new CommandError('exec', t('shared.secret.empty'))
  if (/[\r\n]/.test(value)) throw new CommandError('exec', t('shared.secret.newline'))
  const bytes = new TextEncoder().encode(value)
  if (bytes.length > MAX_SECRET_BYTES) throw new CommandError('exec', t('shared.secret.tooLong'))
  let hex = ''
  for (const b of bytes) hex += b.toString(16).padStart(2, '0')
  return hex
}

/** Inbox write: builtins only (`umask`, `echo`, redirection); the hex never reaches an argv. */
function inboxWrite(file: string, hex: string): string {
  return `umask 077; mkdir -p '${SYNC_INBOX}' 2>/dev/null; echo ${hex} > '${SYNC_INBOX}/${file}'`
}

/** Store a secret via the 0600 inbox (§19.2). The caller must clear its copy. */
export async function setSecret(
  kind: SecretKind,
  value: string,
): Promise<{ path: string; mode: string; weak: boolean }> {
  const file = `${kind}.hex`
  const cmd = `${inboxWrite(file, secretHex(value))} && ${PREFIX} secret set ${kind} --from-file '${SYNC_INBOX}/${file}' --hex`
  let r
  try {
    r = await exec(cmd)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
  const env = parseLastJsonLine<SyncEnvelope<{ path: string; mode: string; weak: boolean }>>(r.stdout)
  if (!env.ok) throw new CommandError('exit', syncErrorText(env.error), JSON.stringify(env.error))
  return env.data
}

/** Sender: stage the new passphrase for the `rotate` task; start it right after. */
export async function prepareRotation(newPassphrase: string): Promise<void> {
  const r = await exec(inboxWrite('rotate.hex', secretHex(newPassphrase)))
  if (r.errno !== 0) throw new CommandError('exit', t('shared.command.writeFailed'), r.stderr.trim())
}

/** Remove a staged rotation passphrase whose task never started. */
export async function discardRotation(): Promise<void> {
  await exec(`rm -f '${SYNC_INBOX}/rotate.hex'`)
}

export function share(label?: string): Promise<ShareResult> {
  return data<ShareResult>(label ? `share --label ${shq(label)}` : 'share')
}

// receiver

export function importCheck(uri: string): Promise<ImportPreview> {
  return data<ImportPreview>(`import ${shq(uri.trim())} --check`)
}

/** Only after the user confirmed the `importCheck` preview. */
export function importApply(uri: string): Promise<ImportResult> {
  return data<ImportResult>(`import ${shq(uri.trim())} --yes`)
}

// receiver — local deployment lives in apps/deployer/lib/deploy.ts, which
// drives bin/rkp-mini-deployer (no network, no passphrase).

/** Start the module's unattended loop now (it exits by itself if not allowed). */
export async function startLoop(): Promise<void> {
  await exec(
    `if command -v setsid >/dev/null 2>&1; then setsid sh '${SYNC_LOOP}' </dev/null >/dev/null 2>&1 & ` +
      `else nohup sh '${SYNC_LOOP}' </dev/null >/dev/null 2>&1 & fi; true`,
  )
}

/** Tail of logs/sync.log (seq/hash/provider/error codes only, never secrets). */
export async function readSyncLog(lines = 200): Promise<string> {
  const r = await exec(`tail -n ${Math.trunc(lines)} '${SYNC_LOG}' 2>/dev/null`)
  return r.stdout
}

// ---- Long tasks (detached runner/sync.sh) -----------------------------------------------

/**
 * Sender: push | rotate | check (check-remote) | delete (remote-delete --yes).
 * Receiver: fetch [plaintext sha] | check (check-remote) | run.
 * One runner, so ops never overlap.
 */
export type SyncOp = 'push' | 'rotate' | 'check' | 'delete' | 'fetch' | 'run'
export type SyncTaskResult = TaskResult<SyncEnvelope<unknown>>
export type SyncRunner = TaskRunner<SyncEnvelope<unknown>>

export function useSyncRunner(onFinished: (result: SyncTaskResult) => Promise<void> | void): SyncRunner {
  return useTaskRunner<SyncEnvelope<unknown>>(
    { script: SYNC_RUNNER, tmp: SYNC_TMP, name: 'sync-task', storageKey: `rkp.sync.${SYNC_ROLE}` },
    onFinished,
  )
}

/** Runner arguments for an operation (validated again by sync.sh). */
export function syncTaskArgs(op: SyncOp, acceptFirst?: string): string[] {
  if (op !== 'fetch' || acceptFirst === undefined) return [op]
  if (!/^[0-9a-f]{16,64}$/.test(acceptFirst)) throw new CommandError('exec', t('shared.command.badConfirmation'))
  return [op, acceptFirst]
}

// ---- Labels ---------------------------------------------------------------------------
// Machine codes map to `shared.sync.*` keys; an unknown code shows verbatim.

export function syncStageLabel(stage: string): string {
  return tCode(`shared.sync.stage.${stage}`) ?? stage
}

export function syncCodeLabel(code: string): string {
  return tCode(`shared.sync.code.${code}`) ?? code
}

/** Sender wording: what the cached `check-remote` result means for this device. */
export function checkOutcomeLabel(outcome: string): string | undefined {
  return tCode(`shared.sync.checkOutcome.${outcome}`)
}

/** `label（code）` (localized) when the code is known, else `code: message`. */
export function syncErrorText(e: Pick<SyncErrorInfo, 'code' | 'message'>): string {
  const label = tCode(`shared.sync.code.${e.code}`)
  return label ? t('shared.sync.errorText', { label, code: e.code }) : `${e.code}: ${e.message}`
}

export function identityPolicyLabel(policy: IdentityPolicy): string {
  return t(`shared.sync.identityPolicy.${policy}`)
}

const TARGET_NAME: Record<string, string> = {
  omk: 'OhMyKeymint',
  teesim: 'TEESimulator',
  tricky_store: 'TrickyStore / TEESimulator-RS',
}

/** Builtin target's product name, `custom` translated, anything else verbatim. */
export function targetLabel(id: string): string {
  return id === 'custom' ? t('shared.sync.targetCustom') : (TARGET_NAME[id] ?? id)
}

export function warningLabel(code: string): string | undefined {
  return tCode(`shared.sync.warning.${code}`)
}
