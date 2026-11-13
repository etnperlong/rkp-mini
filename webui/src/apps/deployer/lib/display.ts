// Presentation helpers shared by the receiver page and its sheets.
// Security: only identifiers, dates and short hashes are ever rendered.
import type { PillTone } from '../../../shared/components/StatusPill.vue'
import { CommandError } from '../../../shared/lib/command'
import { daysLeft, formatDate, formatDateTime } from '../../../shared/lib/format'
import { t, tCode } from '../../../shared/lib/i18n'
import {
  syncCodeLabel,
  syncErrorText,
  type DeployPlan,
  type SyncErrorInfo,
  type TofuFacts,
} from '../../../shared/lib/sync'

// Qualcomm native RKP leaves last ≈12–14 days, so the display threshold stays
// small (the issuer page uses the same 3 days).
/** A deployed keybox counts as "expiring" from this many days left. */
export const EXPIRING_DAYS = 3

/** Receiver wording for the cached `check-remote` outcome; unknown codes stay verbatim. */
export function receiverCheckLabel(outcome: string): string {
  return tCode(`deployer.display.check.${outcome}`) ?? outcome
}

/** Codes only the local deployer binary can return. */
export function deployCodeLabel(code: string): string {
  return tCode(`deployer.display.code.${code}`) ?? syncCodeLabel(code)
}

export function deployErrorText(e: SyncErrorInfo): string {
  const label = tCode(`deployer.display.code.${e.code}`)
  return label ? t('shared.sync.errorText', { label, code: e.code }) : syncErrorText(e)
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
}

export function providerLabel(provider: string | null | undefined): string {
  if (!provider) return t('deployer.display.providerNone')
  if (provider === 'url') return t('deployer.display.providerUrl')
  return PROVIDER[provider] ?? provider
}

export function stamp(unix: number | null | undefined): string {
  return unix && unix > 0 ? formatDateTime(unix) : '—'
}

/** `2027-02-04（剩余 86 天）`; negative days are shown as expired. */
export function leafLabel(notAfter: number | null | undefined, now: number): string {
  if (!notAfter || notAfter <= 0) return '—'
  const days = daysLeft(notAfter, now)
  const date = formatDate(notAfter)
  const n = Math.abs(days)
  return days < 0
    ? t('deployer.display.leafExpired', { date, n }, n)
    : t('deployer.display.leafLeft', { date, n }, n)
}

export function pendingTitle(reason: string): string {
  if (reason === 'first_deploy') return t('deployer.display.pendingFirstDeploy')
  if (reason === 'identity_change') return t('deployer.display.pendingIdentityChange')
  return t('deployer.display.pendingOther', { reason })
}

function gateReason(reason: string): string {
  return tCode(`deployer.display.gateReason.${reason}`) ?? reason
}

export function gateTitle(plan: DeployPlan): string {
  switch (plan.gate.kind) {
    case 'same':
      return t('deployer.display.gateSame')
    case 'first_deploy':
      return t('deployer.display.gateFirstDeploy')
    case 'allowed':
      return t('deployer.display.gateAllowed')
    case 'blocked':
      return t('deployer.display.gateBlocked')
  }
}

export function gateDetail(plan: DeployPlan): string {
  switch (plan.gate.kind) {
    case 'same':
      return t('deployer.display.gateSameDetail')
    case 'first_deploy':
      return t('deployer.display.gateFirstDeployDetail')
    case 'allowed':
      return gateReason(plan.gate.reason)
    case 'blocked':
      return t('deployer.display.gateBlockedDetail', { reason: gateReason(plan.gate.reason) })
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
