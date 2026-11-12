import { appConfig } from '@app-config'
import { exec } from '../../../shared/lib/ksu'
import { CommandError, SAFE_PATH, parseLastJsonLine, resolveModuleDir } from '../../../shared/lib/command'
import { daysLeft, nowUnix } from '../../../shared/lib/format'
import { ICON_CHECK_CIRCLE, ICON_ERROR, ICON_HELP, ICON_WARNING } from '../../../shared/lib/icons'

// ---- Paths ------------------------------------------------------------------

export const MODULE_ID = appConfig.moduleId

export const MODULE_DIR = resolveModuleDir()
export const BIN = `${MODULE_DIR}/bin/rkp-mini-issuer`
export const DATA = appConfig.dataRoot
export const TMP = `${DATA}/tmp`
export const CONFIG = `${DATA}/config.toml`
export const STATUS_JSON = `${DATA}/status.json`
export const LOGS = `${DATA}/logs`
const AUTO_LOOP = `${MODULE_DIR}/${appConfig.loopScript}`
const AUTO_LAST = `${TMP}/auto-issue.last`

for (const p of [AUTO_LOOP, AUTO_LAST]) {
  if (!SAFE_PATH.test(p)) throw new Error(`unsafe path: ${p}`)
}

// ---- JSON contract (rkp-mini-issuer --json) ----------------------------------

export interface DeviceInfo {
  android_arm64: boolean
  root: boolean
  qualcomm: boolean
  library_present: boolean
  device_present: boolean
  fingerprint: string
}

export interface KeyboxInfo {
  path: string
  exists: boolean
  valid: boolean
  certificates: number
  leaf_subject: string
  not_after: number | null
  not_after_utc: string | null
  days_left: number | null
  key_match: boolean
  error: string | null
}

export interface StatusData {
  generated_at: number
  device: DeviceInfo
  blockers: string[]
  keybox: KeyboxInfo
  description: string
  /** Issuer-owned auto-issue setting plus whether auto-loop.sh is alive. */
  auto_issue: { days: number; loop_running: boolean }
}

export interface IssueSuccess {
  keybox: string
  chain_certificates: number
  leaf_subject: string
  leaf_not_after: number
  leaf_not_after_utc: string
  device_id: string
  csr_len: number
  frame_layout: string
  ta_status: number
}

export interface IssueFailure {
  stage: string
  code: string
  message: string
  ta_status?: number
  head_hex?: string
}

/** `issue --if-expiring-days` data when the leaf is not due (exit 0). */
export interface IssueSkip {
  skipped: true
  reason: string
  days_left: number
  threshold_days: number
  keybox: string
}

export type Envelope<T> = { ok: true; data: T } | { ok: false; error: IssueFailure }

// ---- Errors -------------------------------------------------------------------

async function run(command: string): Promise<{ stdout: string; stderr: string; errno: number }> {
  try {
    return await exec(command)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
}

// ---- Commands -----------------------------------------------------------------

/**
 * `status --json`; local checks only, also rewrites status.{json,txt}.
 * `updateDescription` additionally syncs module.prop.
 */
export async function readStatus(updateDescription = false): Promise<StatusData> {
  const flags = updateDescription ? ' --update-description' : ''
  const r = await run(`'${BIN}' status --json${flags}`)
  if (r.errno !== 0) {
    throw new CommandError('exit', `退出码 ${r.errno}`, (r.stderr || r.stdout).trim())
  }
  const env = parseLastJsonLine<Envelope<StatusData>>(r.stdout)
  if (!env.ok) throw new CommandError('exit', env.error.message, JSON.stringify(env.error))
  return env.data
}

/** Last persisted status.json; read-only, safe while an issuance is running. */
export async function readCachedStatus(): Promise<StatusData> {
  const r = await run(`cat '${STATUS_JSON}'`)
  if (r.errno !== 0) throw new CommandError('exit', '尚无缓存的状态', r.stderr.trim())
  try {
    return JSON.parse(r.stdout) as StatusData
  } catch {
    throw new CommandError('parse', 'status.json 解析失败', r.stdout.slice(0, 400))
  }
}

/** `rkp-mini-issuer --version` prints `rkp-mini-issuer 0.2.0`; keep the version only. */
export async function readVersion(): Promise<string> {
  const r = await run(`'${BIN}' --version`)
  if (r.errno !== 0) throw new CommandError('exit', `退出码 ${r.errno}`, r.stderr.trim())
  return r.stdout.trim().split(/\s+/).pop() ?? ''
}

export async function readConfig(): Promise<string> {
  const r = await run(`cat '${CONFIG}'`)
  if (r.errno !== 0) throw new CommandError('exit', '配置文件不存在，使用内置默认值', r.stderr.trim())
  return r.stdout
}

const LOG_NAME = /^issue-\d+\.log$/

/** Issue log names, newest first. */
export async function listLogs(): Promise<string[]> {
  const r = await run(`ls -1t '${LOGS}' 2>/dev/null`)
  return r.stdout
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => LOG_NAME.test(l))
}

export async function readLog(name: string): Promise<string> {
  if (!LOG_NAME.test(name)) throw new CommandError('exec', `非法日志名：${name}`)
  const r = await run(`tail -n 200 '${LOGS}/${name}'`)
  if (r.errno !== 0) throw new CommandError('exit', `读取失败（${r.errno}）`, r.stderr.trim())
  return r.stdout
}

const AUTO_MAX_DAYS = 365
const SEP = '<<<RKP-SEP>>>'

/** C1: only key the issuer lets the page set; 0 = off, 365 = max. */
export async function setAutoIssueDays(days: number): Promise<number> {
  if (!Number.isInteger(days) || days < 0 || days > AUTO_MAX_DAYS) {
    throw new CommandError('exec', `天数必须是 0–${AUTO_MAX_DAYS} 的整数`)
  }
  const r = await run(`'${BIN}' config set auto_issue_days ${days} --json`)
  if (r.errno !== 0 && !r.stdout.trim()) throw new CommandError('exit', `退出码 ${r.errno}`, r.stderr.trim())
  const env = parseLastJsonLine<Envelope<{ auto_issue_days: number }>>(r.stdout)
  if (!env.ok) {
    throw new CommandError('exit', `${stageLabel(env.error.stage)}：${env.error.message}`, JSON.stringify(env.error))
  }
  return env.data.auto_issue_days
}

/** Delete the issued keybox and its artifacts (`remove --yes`); refused while an issuance holds issue.lock. */
export async function removeKeybox(): Promise<{ removed: string[]; output_dir: string }> {
  const r = await run(`'${BIN}' remove --yes --json`)
  if (r.errno !== 0 && !r.stdout.trim()) throw new CommandError('exit', `退出码 ${r.errno}`, r.stderr.trim())
  const env = parseLastJsonLine<Envelope<{ removed: string[]; output_dir: string }>>(r.stdout)
  if (!env.ok) {
    const label = env.error.code === 'issue_running' ? '签发进行中，请稍后再试' : env.error.message
    throw new CommandError('exit', `${stageLabel(env.error.stage)}：${label}`, JSON.stringify(env.error))
  }
  return env.data
}

/** Start `auto-loop.sh` detached; its pid file makes repeat starts harmless. */
export async function startAutoLoop(): Promise<void> {
  await exec(
    `if command -v setsid >/dev/null 2>&1; then setsid sh '${AUTO_LOOP}' </dev/null >/dev/null 2>&1 & ` +
      `else nohup sh '${AUTO_LOOP}' </dev/null >/dev/null 2>&1 & fi; true`,
  )
}

/** Last finished auto-issue run (C6); null when no `.code` file exists. */
export interface AutoIssueLast {
  /** Unix seconds, null when `.at` is missing or not a positive integer. */
  at: number | null
  code: number
  envelope: Envelope<IssueSuccess | IssueSkip> | null
  stderr: string
}

/** C6: read `$TMP/auto-issue.last.{at,code,json,err}` in one exec. */
export async function readAutoIssueLast(): Promise<AutoIssueLast | null> {
  const r = await run(
    `echo '${SEP}'; cat '${AUTO_LAST}.at' 2>/dev/null; echo '${SEP}'; cat '${AUTO_LAST}.code' 2>/dev/null; ` +
      `echo '${SEP}'; cat '${AUTO_LAST}.json' 2>/dev/null; echo '${SEP}'; ` +
      `tail -c 16384 '${AUTO_LAST}.err' 2>/dev/null`,
  )
  const parts = r.stdout.split(SEP)
  if (parts.length !== 5) return null
  const [atRaw, codeRaw, jsonRaw, errRaw] = parts.slice(1).map((p) => p.replace(/^\n/, ''))
  const code = Number(codeRaw.trim())
  if (!codeRaw.trim() || !Number.isInteger(code)) return null
  const at = Number(atRaw.trim())
  let envelope: Envelope<IssueSuccess | IssueSkip> | null = null
  try {
    envelope = parseLastJsonLine<Envelope<IssueSuccess | IssueSkip>>(jsonRaw)
  } catch {
    // Killed before printing an envelope: the caller shows code + stderr.
  }
  return { at: Number.isInteger(at) && at > 0 ? at : null, code, envelope, stderr: errRaw.replace(/^\n/, '') }
}

export interface ModuleProp {
  version: string
  versionCode: string
}

export async function readModuleProp(): Promise<ModuleProp> {
  const r = await run(`cat '${MODULE_DIR}/module.prop'`)
  if (r.errno !== 0) throw new CommandError('exit', `读取失败（${r.errno}）`, r.stderr.trim())
  const get = (key: string) => new RegExp(`^${key}=(.*)$`, 'm').exec(r.stdout)?.[1]?.trim() ?? '?'
  return { version: get('version'), versionCode: get('versionCode') }
}

// ---- UI mappings --------------------------------------------------------------

export type KeyboxState = 'missing' | 'expired' | 'invalid' | 'expiring' | 'valid'

// Qualcomm native RKP leaves last ≈12–14 days, so day thresholds stay small.
/** "Expiring" from this many days left (matches the auto-issue default). */
export const EXPIRING_DAYS = 3
/** Auto-issue threshold suggested when switching it on. */
export const AUTO_ISSUE_DEFAULT_DAYS = 3
/** Above this the threshold exceeds a leaf's lifetime and every check would re-issue. */
export const AUTO_ISSUE_WARN_DAYS = 14

/** Keybox state; expiry takes precedence over `valid`. */
export function keyboxState(k: KeyboxInfo, now = nowUnix()): KeyboxState {
  if (!k.exists) return 'missing'
  if (k.not_after !== null && k.not_after <= now) return 'expired'
  if (!k.valid) return 'invalid'
  if (k.not_after !== null && daysLeft(k.not_after, now) <= EXPIRING_DAYS) return 'expiring'
  return 'valid'
}

export const KEYBOX_STATE_LABEL: Record<KeyboxState, string> = {
  valid: '有效',
  expiring: '即将过期',
  expired: '已过期',
  invalid: '无效',
  missing: '未签发',
}

/** Accent colour (CSS value) and icon path (shared/lib/icons.ts) per state. */
export const KEYBOX_STATE_VISUAL: Record<KeyboxState, { color: string; icon: string }> = {
  valid: { color: 'var(--app-ok)', icon: ICON_CHECK_CIRCLE },
  expiring: { color: 'var(--app-warning)', icon: ICON_WARNING },
  expired: { color: 'var(--m-color-error)', icon: ICON_ERROR },
  invalid: { color: 'var(--m-color-error)', icon: ICON_ERROR },
  missing: { color: 'var(--m-color-on-surface-variant-actions)', icon: ICON_HELP },
}

const BLOCKER_LABEL: Record<string, string> = {
  android_arm64_required: '需要 Android arm64 设备',
  root_required: '需要 root 权限',
  qualcomm_required: '仅支持高通平台',
  soc_unknown: '无法识别 SoC 厂商',
  soc_conflicting: 'SoC 识别结果互相矛盾',
  keymint_library_missing: '缺少 /vendor/lib64/libkeymasterdeviceutils.so',
  smcinvoke_missing: '缺少 /dev/smcinvoke',
  fingerprint_missing: '读取不到系统指纹',
}

export function blockerLabel(code: string): string {
  return BLOCKER_LABEL[code] ?? `前置条件不满足（${code}）`
}

const STAGE_LABEL: Record<string, string> = {
  config: '配置',
  prereq: '前置条件',
  challenge: '获取挑战',
  server: '证书服务',
  ta: 'TA 签名',
  csr: 'CSR 校验',
  chain: '证书链',
  write: '写入',
  generic: '其他',
}

export function stageLabel(stage: string): string {
  return STAGE_LABEL[stage] ?? stage
}

const EXIT_LABEL: Record<number, string> = {
  0: '已签发',
  1: '未知错误',
  2: '配置/用法错误',
  3: '前置条件不满足',
  4: '网络或服务端失败（HTTP）',
  5: '设备 TA 签名或 CSR 校验失败',
  6: '证书链校验失败',
  7: '写入文件失败',
}

export function exitLabel(code: number): string {
  return EXIT_LABEL[code] ?? `异常退出（${code}）`
}

/** Actionable explanation for known failure signatures (README go/no-go list). */
export function failureHint(e: IssueFailure): string | null {
  if (e.code === 'device_not_registered') return '设备未在 Google RKP 注册（HTTP 444），无法通过本模块解决。'
  if (e.ta_status === -100) return 'TA 未实现该命令（UNIMPLEMENTED），当前固件不支持 Native V2。'
  if (e.ta_status === -18) return 'TA 要求 MacedPublicKey（INCOMPATIBLE_KEY_FORMAT），当前固件不支持 Native V2。'
  if (e.ta_status === -40) return '厂商命令或帧格式不匹配（INVALID_TAG）。'
  if (e.code === 'ta_timeout') return 'TA 调用超时，可在配置中调大 ta_timeout_seconds 后重试。'
  switch (e.stage) {
    case 'challenge':
    case 'server':
      return '请检查网络或 proxy 配置后重试。'
    case 'prereq':
      return '请查看“环境检查”中未通过的项目。'
    case 'chain':
      return '服务端返回的证书链未通过本地校验。'
    case 'write':
      return '写入失败，请检查存储空间与 SELinux（dmesg | grep avc）。'
    case 'config':
      return '请检查 config.toml。'
    default:
      return null
  }
}
