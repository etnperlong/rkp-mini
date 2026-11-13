// Combined run result (`run.sh`): the deployer module publishes one snapshot
// per combined run, never a CLI envelope.
//
//   <dataRoot>/tmp/sync-task.last.steps   eight sections, seven separator lines
//   <dataRoot>/tmp/sync-task.last.op      `run` (single-step `fetch` keeps its own)
//   <dataRoot>/tmp/sync-task.last.code    completion code, written last
//
// Sections, in order: overall code, fetch code, fetch stdout JSON, fetch
// stderr tail, deploy code, deploy stdout JSON, deploy stderr tail,
// orchestration stderr. A step that did not run has an empty code and an
// empty JSON section.
import { appConfig } from '@app-config'
import { t, type MessageKey } from '../../../shared/lib/i18n'
import { exec } from '../../../shared/lib/ksu'
import { CommandError } from '../../../shared/lib/command'
import type {
  DeployPlan,
  DeployReport,
  FetchResult,
  SyncEnvelope,
  TofuFacts,
} from '../../../shared/lib/sync'
import { reportWrote } from './deploy'

export type DeploySkip = 'nothing_staged' | 'auto_deploy_false'
export type DeployOutcome = DeployReport | DeployPlan | { skipped: DeploySkip }

export interface StepResult<T> {
  code: number | null
  envelope: SyncEnvelope<T> | null
  stderr: string
}

export interface RunSteps {
  overall: number
  fetch: StepResult<FetchResult | TofuFacts>
  deploy: StepResult<DeployOutcome>
  stderr: string
}

const SEP = '<<<RKP-SEP>>>'
const TMP = `${appConfig.dataRoot}/tmp`

async function readState(name: string): Promise<string> {
  const r = await exec(`cat '${TMP}/${name}' 2>/dev/null`)
  if (r.errno !== 0) throw new CommandError('parse', t('deployer.run.missingState', { name }))
  return r.stdout
}

/** Trimmed, empty → null, otherwise an integer 0..255. */
function sectionCode(text: string, label: MessageKey): number | null {
  const value = text.trim()
  if (!value) return null
  if (!/^\d{1,3}$/.test(value) || Number(value) > 255) {
    throw new CommandError('parse', t('deployer.run.badCode', { label: t(label), value: value.slice(0, 40) }))
  }
  return Number(value)
}

/** Trimmed, empty → null, otherwise a well-formed sync envelope. */
function sectionEnvelope<T>(text: string, label: MessageKey): SyncEnvelope<T> | null {
  const value = text.trim()
  if (!value) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch {
    throw new CommandError('parse', t('deployer.run.notJson', { label: t(label) }))
  }
  if (typeof parsed !== 'object' || parsed === null) {
    throw new CommandError('parse', t('deployer.run.notJsonObject', { label: t(label) }))
  }
  const env = parsed as Record<string, unknown>
  if (env.ok === true) {
    if (!('data' in env)) throw new CommandError('parse', t('deployer.run.missingData', { label: t(label) }))
    return parsed as SyncEnvelope<T>
  }
  if (env.ok === false) {
    const err = env.error as Record<string, unknown> | null | undefined
    if (typeof err !== 'object' || err === null || typeof err.code !== 'string') {
      throw new CommandError('parse', t('deployer.run.missingErrorCode', { label: t(label) }))
    }
    return parsed as SyncEnvelope<T>
  }
  throw new CommandError('parse', t('deployer.run.missingOk', { label: t(label) }))
}

/** Split on separator lines; anything but eight sections is a format error. */
function splitSections(raw: string): string[] {
  const sections: string[] = []
  let current: string[] = []
  for (const line of raw.split('\n')) {
    if (line === SEP) {
      sections.push(current.join('\n'))
      current = []
    } else {
      current.push(line)
    }
  }
  sections.push(current.join('\n'))
  if (sections.length !== 8) {
    const n = sections.length
    throw new CommandError('parse', t('deployer.run.badSectionCount', { n }, n))
  }
  return sections
}

/**
 * Read the last combined run. `last.op` must be `run`: a stale single-step
 * `fetch` snapshot must never be presented as a combined result.
 */
export async function readRunSteps(): Promise<RunSteps> {
  const [op, raw] = await Promise.all([
    readState('sync-task.last.op'),
    readState('sync-task.last.steps'),
  ])
  if (op.trim() !== 'run') {
    throw new CommandError('parse', t('deployer.run.notCombined'))
  }
  const sections = splitSections(raw.replace(/\r\n/g, '\n'))
  const overall = sectionCode(sections[0], 'deployer.run.labelOverall')
  if (overall === null) throw new CommandError('parse', t('deployer.run.missingOverall'))
  return {
    overall,
    fetch: {
      code: sectionCode(sections[1], 'deployer.run.labelFetch'),
      envelope: sectionEnvelope<FetchResult | TofuFacts>(sections[2], 'deployer.run.labelFetch'),
      stderr: sections[3],
    },
    deploy: {
      code: sectionCode(sections[4], 'deployer.run.labelDeploy'),
      envelope: sectionEnvelope<DeployOutcome>(sections[5], 'deployer.run.labelDeploy'),
      stderr: sections[6],
    },
    stderr: sections[7],
  }
}

// ---- Outcome ---------------------------------------------------------------------

/** What a combined run did to the targets (a failed step wins). */
export type RunDeployOutcome =
  | { kind: 'written' }
  | { kind: 'unchanged' }
  | { kind: 'pending' }
  | { kind: 'skipped'; reason: DeploySkip }
  | { kind: 'failed' }

export function isDeploySkipped(value: unknown): value is { skipped: DeploySkip } {
  return typeof value === 'object' && value !== null && 'skipped' in value
}

export function isDeployReport(value: unknown): value is DeployReport {
  return typeof value === 'object' && value !== null && 'deployed' in value && 'targets' in value
}

/**
 * Derive the visible outcome from the published deploy step: skip and pending
 * mean nothing was written; a report whose successful targets all report
 * `reload_status: skipped` is the idempotent pass (nothing new on disk).
 */
export function runDeployOutcome(steps: RunSteps): RunDeployOutcome {
  const env = steps.deploy.envelope
  if (!env || !env.ok) return { kind: 'failed' }
  const data = env.data
  if (isDeploySkipped(data)) return { kind: 'skipped', reason: data.skipped }
  if (isDeployReport(data)) {
    if (data.pending) return { kind: 'pending' }
    if (reportWrote(data)) return { kind: 'written' }
    return data.deployed ? { kind: 'unchanged' } : { kind: 'failed' }
  }
  return { kind: 'failed' }
}

/** One line for the snackbar, the 部署 card task line and the result sheet. */
export function runOutcomeLabel(outcome: RunDeployOutcome): string {
  switch (outcome.kind) {
    case 'written':
      return t('deployer.run.done')
    case 'unchanged':
      return t('deployer.run.unchanged')
    case 'pending':
      return t('deployer.run.pending')
    case 'skipped':
      return outcome.reason === 'auto_deploy_false'
        ? t('deployer.run.skippedAutoDeployOff')
        : t('deployer.run.skippedNothingStaged')
    case 'failed':
      return t('deployer.run.failed')
  }
}

/** Shown only when a keybox actually reached a target. */
export function runRebootHint(outcome: RunDeployOutcome): string | null {
  return outcome.kind === 'written' ? t('deployer.run.rebootHint') : null
}
