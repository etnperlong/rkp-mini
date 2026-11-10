import { moduleInfo } from 'kernelsu'
import { exec, hasKsu } from './ksu'
import { daysLeft, nowUnix } from './format'

// ---- Paths ------------------------------------------------------------------

export const MODULE_ID = 'rkp_mini_issuer'
const DEFAULT_MODULE_DIR = '/data/adb/modules/rkp_mini_issuer'

/** Only plain absolute paths are ever interpolated into shell commands. */
export const SAFE_PATH = /^\/[A-Za-z0-9._/-]+$/

/** This module's directory from the manager, else `fallback` (older managers / dev). */
export function resolveModuleDir(fallback: string): string {
  if (!hasKsu()) return fallback
  try {
    const dir: unknown = JSON.parse(moduleInfo()).moduleDir
    if (typeof dir === 'string' && SAFE_PATH.test(dir)) return dir
  } catch {
    // Older managers: fall through to the fixed install path.
  }
  return fallback
}

export const MODULE_DIR = resolveModuleDir(DEFAULT_MODULE_DIR)
export const BIN = `${MODULE_DIR}/bin/rkp-mini-issuer`
export const DATA = '/data/misc/keystore/rkp-mini'
export const TMP = `${DATA}/tmp`
export const CONFIG = `${DATA}/config.toml`
export const STATUS_JSON = `${DATA}/status.json`
export const LOGS = `${DATA}/logs`

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

export type Envelope<T> = { ok: true; data: T } | { ok: false; error: IssueFailure }

// ---- Errors -------------------------------------------------------------------

/** exec: bridge failed or timed out; exit: non-zero exit; parse: stdout not a JSON envelope. */
export type CommandErrorKind = 'exec' | 'exit' | 'parse'

export class CommandError extends Error {
  constructor(
    readonly kind: CommandErrorKind,
    message: string,
    readonly detail = '',
  ) {
    super(message)
  }
}

async function run(command: string): Promise<{ stdout: string; stderr: string; errno: number }> {
  try {
    return await exec(command)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
}

/** Last non-empty stdout line parsed as JSON (stderr is diagnostics only). */
export function parseLastJsonLine<T>(stdout: string): T {
  const line = stdout
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .pop()
  if (!line) throw new CommandError('parse', '没有输出', stdout)
  try {
    return JSON.parse(line) as T
  } catch {
    throw new CommandError('parse', 'JSON 解析失败', line)
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

export async function readVersion(): Promise<string> {
  const r = await run(`'${BIN}' --version`)
  if (r.errno !== 0) throw new CommandError('exit', `退出码 ${r.errno}`, r.stderr.trim())
  return r.stdout.trim()
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

/** Keybox state; expiry takes precedence over `valid`. */
export function keyboxState(k: KeyboxInfo, now = nowUnix()): KeyboxState {
  if (!k.exists) return 'missing'
  if (k.not_after !== null && k.not_after <= now) return 'expired'
  if (!k.valid) return 'invalid'
  if (k.not_after !== null && daysLeft(k.not_after, now) <= 7) return 'expiring'
  return 'valid'
}

export const KEYBOX_STATE_LABEL: Record<KeyboxState, string> = {
  valid: '有效',
  expiring: '即将过期',
  expired: '已过期',
  invalid: '无效',
  missing: '未签发',
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
