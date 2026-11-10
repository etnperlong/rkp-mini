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
import { exec } from '../../../shared/lib/ksu'
import { CommandError } from '../../../shared/lib/command'
import type {
  DeployPlan,
  DeployReport,
  FetchResult,
  SyncEnvelope,
  TofuFacts,
} from '../../../shared/lib/sync'

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
  if (r.errno !== 0) throw new CommandError('parse', `没有组合任务结果（缺少 ${name}）`)
  return r.stdout
}

/** Trimmed, empty → null, otherwise an integer 0..255. */
function sectionCode(text: string, label: string): number | null {
  const value = text.trim()
  if (!value) return null
  if (!/^\d{1,3}$/.test(value) || Number(value) > 255) {
    throw new CommandError('parse', `${label}退出码无效：${value.slice(0, 40)}`)
  }
  return Number(value)
}

/** Trimmed, empty → null, otherwise a well-formed sync envelope. */
function sectionEnvelope<T>(text: string, label: string): SyncEnvelope<T> | null {
  const value = text.trim()
  if (!value) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch {
    throw new CommandError('parse', `${label}输出不是 JSON`)
  }
  if (typeof parsed !== 'object' || parsed === null) {
    throw new CommandError('parse', `${label}输出不是 JSON 对象`)
  }
  const env = parsed as Record<string, unknown>
  if (env.ok === true) {
    if (!('data' in env)) throw new CommandError('parse', `${label}输出缺少 data`)
    return parsed as SyncEnvelope<T>
  }
  if (env.ok === false) {
    const err = env.error as Record<string, unknown> | null | undefined
    if (typeof err !== 'object' || err === null || typeof err.code !== 'string') {
      throw new CommandError('parse', `${label}错误输出缺少 code`)
    }
    return parsed as SyncEnvelope<T>
  }
  throw new CommandError('parse', `${label}输出缺少 ok 字段`)
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
    throw new CommandError('parse', `组合结果应为 8 段，实际 ${sections.length} 段`)
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
    throw new CommandError('parse', '最近一次任务不是组合运行，不能按组合结果展示')
  }
  const sections = splitSections(raw.replace(/\r\n/g, '\n'))
  const overall = sectionCode(sections[0], '总体')
  if (overall === null) throw new CommandError('parse', '组合结果缺少总退出码')
  return {
    overall,
    fetch: {
      code: sectionCode(sections[1], '拉取'),
      envelope: sectionEnvelope<FetchResult | TofuFacts>(sections[2], '拉取'),
      stderr: sections[3],
    },
    deploy: {
      code: sectionCode(sections[4], '部署'),
      envelope: sectionEnvelope<DeployOutcome>(sections[5], '部署'),
      stderr: sections[6],
    },
    stderr: sections[7],
  }
}
