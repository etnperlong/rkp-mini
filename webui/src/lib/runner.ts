// Background issuance: launch webroot/runner/issue.sh detached from the page,
// then poll its state files. There is no cancel: the page only reads files.
import { computed, onBeforeUnmount, ref, type ComputedRef, type Ref } from 'vue'
import { exec } from './ksu'
import { debug } from './debug'
import { MODULE_DIR, TMP, parseLastJsonLine, type Envelope, type IssueSuccess } from './rkp'

const RUNNER = `${MODULE_DIR}/webroot/runner/issue.sh`
const LOCK = `${TMP}/issue.lock`
const STALE_AFTER_S = 600
const OVERDUE_AFTER_S = 120
const START_GRACE_MS = 8000
const SEP = '<<<RKP-SEP>>>'

const DISMISSED_KEY = 'rkp.issue.dismissed'
const REFRESHED_KEY = 'rkp.issue.refreshed'

export interface IssueResult {
  code: number
  /** Unix seconds; mtime of issue.last.code. */
  finishedAt: number
  envelope: Envelope<IssueSuccess> | null
  stdout: string
  stderr: string
}

/**
 * idle: nothing running, no lock.
 * starting: launch sent, runner has not created its lock yet.
 * running: lock present, no exit code.
 * stale: lock older than 10 min without exit code (runner was killed).
 * finished: exit code present; `result` holds the outcome.
 * launch_failed: runner never appeared after launch.
 */
export type RunnerPhase = 'idle' | 'starting' | 'running' | 'stale' | 'finished' | 'launch_failed'

interface Probe {
  now: number
  lock: number | null
  code: number | null
  end: number | null
}

async function probe(): Promise<Probe> {
  const r = await exec(
    `echo "now=$(date +%s)"; cd '${TMP}' 2>/dev/null || exit 0; ` +
      `[ -d issue.lock ] && echo "lock=$(stat -c %Y issue.lock)"; ` +
      `[ -f issue.last.code ] && echo "code=$(cat issue.last.code)" && echo "end=$(stat -c %Y issue.last.code)"; true`,
  )
  const kv = new Map<string, number>()
  for (const line of r.stdout.split('\n')) {
    const m = /^(\w+)=(-?\d+)$/.exec(line.trim())
    if (m) kv.set(m[1], Number(m[2]))
  }
  return {
    now: kv.get('now') ?? Math.floor(Date.now() / 1000),
    lock: kv.get('lock') ?? null,
    code: kv.get('code') ?? null,
    end: kv.get('end') ?? null,
  }
}

async function readResult(code: number, finishedAt: number): Promise<IssueResult> {
  const r = await exec(`cat '${TMP}/issue.last.json' 2>/dev/null; echo '${SEP}'; tail -c 16384 '${TMP}/issue.last.err' 2>/dev/null`)
  const at = r.stdout.lastIndexOf(SEP)
  const stdout = at >= 0 ? r.stdout.slice(0, at) : r.stdout
  const stderr = at >= 0 ? r.stdout.slice(at + SEP.length).replace(/^\n/, '') : ''
  let envelope: Envelope<IssueSuccess> | null = null
  try {
    envelope = parseLastJsonLine<Envelope<IssueSuccess>>(stdout)
  } catch {
    // Killed or crashed before printing an envelope; show code + stderr.
  }
  return { code, finishedAt, envelope, stdout, stderr }
}

function readMark(key: string): number {
  try {
    return Number(localStorage.getItem(key) ?? 0)
  } catch {
    return 0
  }
}

function writeMark(key: string, value: number): void {
  try {
    localStorage.setItem(key, String(value))
  } catch {
    // Storage unavailable: worst case the card reappears / status refreshes again.
  }
}

export interface IssueRunner {
  phase: Ref<RunnerPhase>
  /** Unix seconds; lock mtime while running. */
  startedAt: Ref<number | null>
  result: Ref<IssueResult | null>
  /** starting or running. */
  busy: ComputedRef<boolean>
  elapsedMs: ComputedRef<number>
  /** Running past 120 s: outcome unknown, still polling slowly. */
  overdue: ComputedRef<boolean>
  /** Finished result the user has not dismissed. */
  showLast: ComputedRef<boolean>
  probeError: Ref<string | null>
  check: () => Promise<void>
  start: () => Promise<void>
  clearStaleLock: () => Promise<void>
  dismiss: () => void
}

/**
 * Runner controller. `onFinished` fires once per finished task (tracked by
 * the exit-code mtime), including tasks that ended while the page was closed.
 */
export function useIssueRunner(onFinished: (result: IssueResult) => Promise<void> | void): IssueRunner {
  const phase = ref<RunnerPhase>('idle')
  const startedAt = ref<number | null>(null)
  const result = ref<IssueResult | null>(null)
  const dismissed = ref(readMark(DISMISSED_KEY))
  const probeError = ref<string | null>(null)
  const nowMs = ref(Date.now())

  let pollTimer: number | undefined
  let tickTimer: number | undefined
  let launchAt = 0
  let disposed = false

  const busy = computed(() => phase.value === 'starting' || phase.value === 'running')
  const elapsedMs = computed(() => {
    if (phase.value === 'starting') return nowMs.value - launchAt
    return startedAt.value === null ? 0 : nowMs.value - startedAt.value * 1000
  })
  const overdue = computed(() => busy.value && elapsedMs.value > OVERDUE_AFTER_S * 1000)
  const showLast = computed(() => phase.value === 'finished' && result.value !== null && result.value.finishedAt !== dismissed.value)

  function setTicking(on: boolean): void {
    if (on && tickTimer === undefined) {
      tickTimer = window.setInterval(() => (nowMs.value = Date.now()), 1000)
    } else if (!on && tickTimer !== undefined) {
      clearInterval(tickTimer)
      tickTimer = undefined
    }
  }

  function schedule(): void {
    clearTimeout(pollTimer)
    if (disposed) return
    const s = elapsedMs.value / 1000
    const delay = s < 10 ? 600 : s < OVERDUE_AFTER_S ? 1000 : 5000
    pollTimer = window.setTimeout(() => void check(), delay)
  }

  /** One probe + state transition; keeps polling while busy. */
  async function check(): Promise<void> {
    let p: Probe
    try {
      p = await probe()
      probeError.value = null
    } catch (e) {
      probeError.value = e instanceof Error ? e.message : String(e)
      debug(`runner probe failed: ${probeError.value}`)
      if (busy.value) schedule()
      return
    }
    nowMs.value = Date.now()

    if (p.code !== null) {
      const finishedAt = p.end ?? p.now
      if (phase.value !== 'finished' || result.value?.finishedAt !== finishedAt) {
        result.value = await readResult(p.code, finishedAt)
        debug(`runner finished: code=${p.code} at=${finishedAt}`)
      }
      phase.value = 'finished'
      startedAt.value = null
      setTicking(false)
      if (readMark(REFRESHED_KEY) !== finishedAt) {
        writeMark(REFRESHED_KEY, finishedAt)
        await onFinished(result.value!)
      }
      return
    }

    if (p.lock !== null) {
      startedAt.value = p.lock
      if (p.now - p.lock > STALE_AFTER_S) {
        phase.value = 'stale'
        setTicking(false)
        return
      }
      phase.value = 'running'
      setTicking(true)
      schedule()
      return
    }

    if (phase.value === 'starting' && Date.now() - launchAt < START_GRACE_MS) {
      schedule()
      return
    }
    if (phase.value === 'starting') {
      debug('runner did not create its lock after launch')
      phase.value = 'launch_failed'
    } else if (phase.value !== 'launch_failed') {
      phase.value = 'idle'
    }
    startedAt.value = null
    setTicking(false)
  }

  async function start(): Promise<void> {
    if (busy.value || phase.value === 'stale') return
    phase.value = 'starting'
    launchAt = Date.now()
    nowMs.value = launchAt
    result.value = null
    setTicking(true)
    // setsid detaches from the manager's shell; nohup is the fallback when the
    // toybox build lacks setsid. Either way all fds go to /dev/null so the
    // bridge returns immediately.
    const cmd =
      `if command -v setsid >/dev/null 2>&1; then ` +
      `setsid sh '${RUNNER}' </dev/null >/dev/null 2>&1 & echo setsid; ` +
      `else nohup sh '${RUNNER}' </dev/null >/dev/null 2>&1 & echo nohup; fi`
    try {
      const r = await exec(cmd)
      debug(`runner launched via ${r.stdout.trim() || '?'} (errno=${r.errno})`)
    } catch (e) {
      debug(`runner launch failed: ${e instanceof Error ? e.message : String(e)}`)
      phase.value = 'launch_failed'
      setTicking(false)
      return
    }
    schedule()
  }

  async function clearStaleLock(): Promise<void> {
    const r = await exec(`rmdir '${LOCK}'`)
    debug(`cleared stale lock (errno=${r.errno})`)
    phase.value = 'idle'
    await check()
  }

  function dismiss(): void {
    if (!result.value) return
    dismissed.value = result.value.finishedAt
    writeMark(DISMISSED_KEY, result.value.finishedAt)
  }

  onBeforeUnmount(() => {
    disposed = true
    clearTimeout(pollTimer)
    setTicking(false)
  })

  return { phase, startedAt, result, busy, elapsedMs, overdue, showLast, probeError, check, start, clearStaleLock, dismiss }
}
