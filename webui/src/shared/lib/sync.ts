// rkp-mini-sync WebUI contract (design docs/REMOTE_SYNC_DEVELOP.md §19).
// Single source of every path, command, JSON type and Chinese label used by
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
  // sender
  local_keybox?: LocalKeybox
  auto_issue_days?: number
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
  auto_issue_days?: number
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
  | 'auto_issue_days'
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
  if (/[\u0000-\u001f\u007f]/.test(value)) throw new CommandError('exec', '参数含有控制字符')
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

export function forgetSync(staging: boolean): Promise<{ removed: string[]; enabled: false }> {
  return data(`forget --yes${staging ? ' --staging' : ''}`)
}

export type SecretKind = 'age' | 'gist-token' | 'webdav-password'

const MAX_SECRET_BYTES = 4096

/** UTF-8 → lowercase hex; refuses empty, multi-line or oversize secrets. */
function secretHex(value: string): string {
  if (!value) throw new CommandError('exec', '不能为空')
  if (/[\r\n]/.test(value)) throw new CommandError('exec', '不能包含换行')
  const bytes = new TextEncoder().encode(value)
  if (bytes.length > MAX_SECRET_BYTES) throw new CommandError('exec', '过长')
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
  if (r.errno !== 0) throw new CommandError('exit', '写入失败', r.stderr.trim())
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

/** Sender: push | rotate. Receiver: fetch [plaintext sha] | run. */
export type SyncOp = 'push' | 'rotate' | 'fetch' | 'run'
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
  if (!/^[0-9a-f]{16,64}$/.test(acceptFirst)) throw new CommandError('exec', '非法的确认值')
  return [op, acceptFirst]
}

// ---- Labels ---------------------------------------------------------------------------

const STAGE: Record<string, string> = {
  remote: '远端',
  decrypt: '解密',
  validate: '校验',
  state: '状态',
  deploy: '部署',
  config: '配置',
  generic: '其他',
}

export function syncStageLabel(stage: string): string {
  return STAGE[stage] ?? stage
}

const CODE: Record<string, string> = {
  not_initialized: '尚未初始化同步',
  risk_not_acknowledged: '尚未接受风险说明',
  risk_version_mismatch: '风险说明已更新，需要重新确认',
  sync_disabled: '同步已禁用',
  stream_missing: '尚未配置同步流',
  provider_missing: '尚未选择存储方式',
  secret_missing: '缺少密钥文件',
  secret_permissions: '密钥文件权限不正确',
  secret_empty: '密钥为空',
  secret_invalid: '密钥格式无效',
  token_missing: '缺少 Gist token',
  auth_failed: '认证失败（token/应用密码无效）',
  auth_required: '该链接需要认证，请改用 WebDAV',
  rate_limited: '请求过于频繁，稍后重试',
  remote_missing: '远端没有同步文件',
  remote_conflict: '远端被并发修改，稍后重试',
  remote_locked: '远端文件被锁定，稍后重试',
  quota_exceeded: '远端空间不足',
  remote_unavailable: '远端服务不可用',
  network: '网络错误',
  private_address: '地址指向本地/内网，已拒绝',
  insecure_url: '只允许 https',
  cross_host_redirect: '拒绝跨主机重定向',
  response_too_large: '远端响应过大',
  decrypt_failed: '解密失败（密码错误或文件被篡改）',
  not_armored: '远端文件不是 age 加密文件',
  excessive_work: '加密参数超出安全上限',
  keybox_invalid: 'keybox 校验失败',
  keybox_expiring: 'keybox 剩余有效期不足',
  keybox_missing: '本地没有 keybox，请先签发',
  keybox_permissions: 'keybox 文件权限不正确',
  manifest_invalid: '同步数据格式无效',
  manifest_mismatch: '同步数据与 keybox 不一致',
  stream_mismatch: '同步流不匹配',
  sync_rollback: '远端版本比已验证版本旧（疑似回放）',
  seq_conflict: '同一版本号内容不一致',
  prev_mismatch: '版本链不连续',
  issued_in_future: '签发时间晚于本机时间',
  clock_invalid: '本机时间异常',
  first_import_needs_confirmation: '首次导入需要确认',
  confirmation_mismatch: '内容已变化，请重新查看后确认',
  stream_change: '已有其他同步流，请先清除本地数据',
  locked: '另一个同步任务正在运行',
  rotation_pending: '上次轮换口令未完成',
  no_targets: '尚未配置部署目标',
  target_failed: '部分部署目标失败',
  target_unreferenced: 'TEESimulator 配置未引用该文件名',
  omk_incompatible: 'OhMyKeymint 无法解析该 keybox，未写入',
  target_dir_missing: '目标目录不存在（对应模块未安装？）',
  target_modified: '目标文件已被其他程序修改，未删除',
  key_not_settable: '该设置不能在此修改',
  config_invalid: '配置无效',
  import_invalid: '导入链接无效',
  secret_field: '导入链接包含敏感字段，已拒绝',
  unknown_field: '导入链接包含未知字段，已拒绝',
  url_userinfo: '链接不能包含用户名/密码',
  path_traversal: '路径不合法',
  gist_revision_url: 'Gist 链接指向固定版本，请使用最新版链接',
  not_published: '尚未推送过，暂无可分享的链接',
  read_only_provider: '公开链接只读',
}

export function syncCodeLabel(code: string): string {
  return CODE[code] ?? code
}

/** `中文说明（code）` when known, else `code: message`. */
export function syncErrorText(e: SyncErrorInfo): string {
  const label = CODE[e.code]
  return label ? `${label}（${e.code}）` : `${e.code}: ${e.message}`
}

export const IDENTITY_POLICY_LABEL: Record<IdentityPolicy, string> = {
  scheduled: '临近到期才自动轮换（推荐）',
  always: '总是自动部署新身份',
  never: '从不自动部署新身份',
}

export const TARGET_LABEL: Record<string, string> = {
  omk: 'OhMyKeymint',
  teesim: 'TEESimulator',
  tricky_store: 'TrickyStore / TEESimulator-RS',
  custom: '自定义',
}

export const WARNING_LABEL: Record<string, string> = {
  omk_key_retirement: 'OMK 会删除绑定旧 keybox 的客户端密钥（retire_stale_keybox_bound_keys）',
  omk_incompatible: 'OhMyKeymint 无法解析该 keybox（会被其内置模板覆盖），omk 目标不会写入',
  teesim_reattest: 'TEESimulator 会用新 keybox 重新为已有密钥生成证明',
  teesim_config_missing: '未找到 TEESimulator config.json，按默认文件名部署',
  no_root_trust_check: '只做了结构与密钥匹配校验，未验证证书签名/根信任/吊销',
  gap: '跳过了中间版本',
}
