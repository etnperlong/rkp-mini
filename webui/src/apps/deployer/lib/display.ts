// Presentation helpers shared by the receiver page and its sheets.
// Security: only identifiers, dates and short hashes are ever rendered.
import type { PillTone } from '../../../shared/components/StatusPill.vue'
import { CommandError } from '../../../shared/lib/command'
import { daysLeft, formatDate, formatDateTime } from '../../../shared/lib/format'
import {
  syncCodeLabel,
  syncErrorText,
  type CheckOutcome,
  type DeployPlan,
  type SyncErrorInfo,
  type TofuFacts,
} from '../../../shared/lib/sync'

// Qualcomm native RKP leaves last ≈12–14 days, so the display threshold stays
// small (the issuer page uses the same 3 days).
/** A deployed keybox counts as "expiring" from this many days left. */
export const EXPIRING_DAYS = 3

/** Receiver wording for the cached `check-remote` outcome. */
export const RECEIVER_CHECK_LABEL: Record<CheckOutcome, string> = {
  in_sync: '云端与本机一致',
  remote_ahead: '云端有新版本',
  remote_behind: '云端版本比本机验证的旧（拉取会被拒绝，需要发送端重新推送）',
  diverged: '同一版本号的内容不一致',
  missing: '云端没有同步文件',
  error: '云端检查失败',
}

/** Codes only the local deployer binary can return. */
const DEPLOY_CODE: Record<string, string> = {
  nothing_staged: '还没有已验证的 keybox，请先拉取',
}

export function deployCodeLabel(code: string): string {
  return DEPLOY_CODE[code] ?? syncCodeLabel(code)
}

export function deployErrorText(e: SyncErrorInfo): string {
  const label = DEPLOY_CODE[e.code]
  return label ? `${label}（${e.code}）` : syncErrorText(e)
}

export function errorMessage(e: unknown): string {
  if (e instanceof CommandError) return e.message
  if (e instanceof Error) return e.message
  return String(e)
}

/** First `keep` hex characters; enough to compare, too short to be a credential. */
export function shortHash(hash: string | null | undefined, keep = 16): string {
  if (!hash) return '—'
  return hash.length > keep ? `${hash.slice(0, keep)}…` : hash
}

const PROVIDER: Record<string, string> = {
  gist: 'GitHub Gist',
  webdav: 'WebDAV',
  url: '公开 HTTPS',
}

export function providerLabel(provider: string | null | undefined): string {
  if (!provider) return '未配置'
  return PROVIDER[provider] ?? provider
}

export function stamp(unix: number | null | undefined): string {
  return unix && unix > 0 ? formatDateTime(unix) : '—'
}

/** `2027-02-04（剩余 86 天）`; negative days are shown as expired. */
export function leafLabel(notAfter: number | null | undefined, now: number): string {
  if (!notAfter || notAfter <= 0) return '—'
  const days = daysLeft(notAfter, now)
  return `${formatDate(notAfter)}（${days < 0 ? `已过期 ${-days} 天` : `剩余 ${days} 天`}）`
}

export function pendingTitle(reason: string): string {
  if (reason === 'first_deploy') return '首次部署待确认'
  if (reason === 'identity_change') return '身份变化待确认'
  return `待确认（${reason}）`
}

const GATE_REASON: Record<string, string> = {
  rotation_window: '旧 keybox 临近到期，处于自动轮换窗口内',
  policy_always: '身份变化策略为“总是自动部署新身份”',
  manual_allow: '你已手动允许这次身份变化',
  identity_change: '当前身份变化策略不允许自动切换身份',
}

export function gateTitle(plan: DeployPlan): string {
  switch (plan.gate.kind) {
    case 'same':
      return '同一 keybox'
    case 'first_deploy':
      return '首次部署'
    case 'allowed':
      return '策略允许'
    case 'blocked':
      return '身份变化待批准'
  }
}

export function gateDetail(plan: DeployPlan): string {
  switch (plan.gate.kind) {
    case 'same':
      return 'staged 与已部署的 keybox 相同：写入是幂等修复，不会改变 RKP 身份。'
    case 'first_deploy':
      return '本机还没有部署过 keybox。确认后会把 staged 版本写入下列目标。'
    case 'allowed':
      return GATE_REASON[plan.gate.reason] ?? plan.gate.reason
    case 'blocked':
      return `${GATE_REASON[plan.gate.reason] ?? plan.gate.reason}：scheduled 仅在旧 keybox 临近到期（rotation_lead_days 窗口内）时允许自动轮换，never 从不自动允许。继续会替换设备的 RKP 身份。`
  }
}

export function gateTone(plan: DeployPlan): PillTone {
  switch (plan.gate.kind) {
    case 'same':
      return 'ok'
    case 'first_deploy':
      return 'warning'
    case 'allowed':
      return 'warning'
    case 'blocked':
      return 'error'
  }
}

/** TOFU facts arrive as error `data`; trust only a well-formed object. */
export function isTofuFacts(value: unknown): value is TofuFacts {
  if (typeof value !== 'object' || value === null) return false
  const facts = value as Partial<TofuFacts>
  return (
    typeof facts.stream_id === 'string' &&
    typeof facts.plaintext_sha256 === 'string' &&
    /^[0-9a-f]{16,64}$/.test(facts.plaintext_sha256) &&
    typeof facts.keybox_sha256 === 'string' &&
    typeof facts.device_id === 'string' &&
    typeof facts.location === 'string' &&
    Array.isArray(facts.targets) &&
    typeof facts.seq === 'number'
  )
}
