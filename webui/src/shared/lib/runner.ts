// Background tasks: launch a webroot/runner/*.sh script detached from the page,
// then poll its state files. There is no cancel: the page only reads files.
//
// State files (written by the script under `tmp`):
//   <name>.lock/       directory, present while running (mtime = start)
//   <name>.last.code   exit code, written last (mtime = end)
//   <name>.last.json   stdout: single --json envelope
//   <name>.last.err    stderr diagnostics
//   <name>.last.op     optional: which operation ran (sync tasks)
import { computed, onBeforeUnmount, ref, type ComputedRef, type Ref } from 'vue'
import { exec } from './ksu'
import { debug } from './debug'
import { parseLastJsonLine } from './command'

const STALE_AFTER_S = 600
const OVERDUE_AFTER_S = 120
const START_GRACE_MS = 8000
const SEP = '<<<RKP-SEP>>>'

/** Runner script arguments: operation names and hex digests only. */
const SAFE_ARG = /^[A-Za-z0-9._-]{1,80}$/

export interface TaskResult<E> {
  code: number
  /** Unix seconds; mtime of <name>.last.code. */
  finishedAt: number
  /** Operation recorded by the script, if any. */
  op: string | null
  envelope: E | null
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

export interface TaskRunnerOptions {
  /** Absolute path of the runner script. */
  script: string
  /** Directory holding the state files. */
  tmp: string
  /** State file prefix, e.g. `issue` → issue.lock, issue.last.code. */
  name: string
  /** localStorage key prefix for dismissed/refreshed marks. */
  storageKey: string
}

export interface TaskRunner<E> {
  phase: Ref<RunnerPhase>
  /** Unix seconds; lock mtime while running. */
  startedAt: Ref<number | null>
  result: Ref<TaskResult<E> | null>
  /** starting or running. */
  busy: ComputedRef<boolean>
  elapsedMs: ComputedRef<number>
  /** Running past 120 s: outcome unknown, still polling slowly. */
  overdue: ComputedRef<boolean>
  /** Finished result the user has not dismissed. */
  showLast: ComputedRef<boolean>
  probeError: Ref<string | null>
  check: () => Promise<void>
  start: (args?: string[]) => Promise<void>
  clearStaleLock: () => Promise<void>
  dismiss: () => void
}

interface Probe {
  now: number
  lock: number | null
  code: number | null
  end: number | null
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

/**
 * Runner controller. `onFinished` fires once per finished task (tracked by
 * the exit-code mtime), including tasks that ended while the page was closed.
 */
export function useTaskRunner<E>(
  opts: TaskRunnerOptions,
  onFinished: (result: TaskResult<E>) => Promise<void> | void,
): TaskRunner<E> {
  const { script, tmp, name } = opts
  const lockPath = `${tmp}/${name}.lock`
  const dismissedKey = `${opts.storageKey}.dismissed`
  const refreshedKey = `${opts.storageKey}.refreshed`

  const phase = ref<RunnerPhase>('idle')
  const startedAt = ref<number | null>(null)
  const result = ref<TaskResult<E> | null>(null) as Ref<TaskResult<E> | null>
  const dismissed = ref(readMark(dismissedKey))
  const probeError = ref<string | null>(null)
  const nowMs = ref(Date.now())

  let pollTimer: number | undefined
  let tickTimer: number | undefined
  let launchAt = 0
  let disposed = false

  async function probe(): Promise<Probe> {
    const r = await exec(
      `echo "now=$(date +%s)"; cd '${tmp}' 2>/dev/null || exit 0; ` +
        `[ -d '${name}.lock' ] && echo "lock=$(stat -c %Y '${name}.lock')"; ` +
        `[ -f '${name}.last.code' ] && echo "code=$(cat '${name}.last.code')" && echo "end=$(stat -c %Y '${name}.last.code')"; true`,
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

  async function readResult(code: number, finishedAt: number): Promise<TaskResult<E>> {
    const base = `${tmp}/${name}.last`
    const r = await exec(
      `cat '${base}.op' 2>/dev/null; echo '${SEP}'; cat '${base}.json' 2>/dev/null; echo '${SEP}'; tail -c 16384 '${base}.err' 2>/dev/null`,
    )
    const parts = r.stdout.split(SEP)
    const op = parts.length === 3 ? parts[0].trim() || null : null
    const stdout = parts.length === 3 ? parts[1].replace(/^\n/, '') : r.stdout
    const stderr = parts.length === 3 ? parts[2].replace(/^\n/, '') : ''
    let envelope: E | null = null
    try {
      envelope = parseLastJsonLine<E>(stdout)
    } catch {
      // Killed or crashed before printing an envelope; show code + stderr.
    }
    return { code, finishedAt, op, envelope, stdout, stderr }
  }

  const busy = computed(() => phase.value === 'starting' || phase.value === 'running')
  const elapsedMs = computed(() => {
    if (phase.value === 'starting') return nowMs.value - launchAt
    return startedAt.value === null ? 0 : nowMs.value - startedAt.value * 1000
  })
  const overdue = computed(() => busy.value && elapsedMs.value > OVERDUE_AFTER_S * 1000)
  const showLast = computed(
    () => phase.value === 'finished' && result.value !== null && result.value.finishedAt !== dismissed.value,
  )

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
      debug(`${name} probe failed: ${probeError.value}`)
      if (busy.value) schedule()
      return
    }
    nowMs.value = Date.now()

    if (p.code !== null) {
      const finishedAt = p.end ?? p.now
      if (phase.value !== 'finished' || result.value?.finishedAt !== finishedAt) {
        result.value = await readResult(p.code, finishedAt)
        debug(`${name} finished: code=${p.code} at=${finishedAt}`)
      }
      phase.value = 'finished'
      startedAt.value = null
      setTicking(false)
      if (readMark(refreshedKey) !== finishedAt) {
        writeMark(refreshedKey, finishedAt)
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
      debug(`${name} runner did not create its lock after launch`)
      phase.value = 'launch_failed'
    } else if (phase.value !== 'launch_failed') {
      phase.value = 'idle'
    }
    startedAt.value = null
    setTicking(false)
  }

  async function start(args: string[] = []): Promise<void> {
    if (busy.value || phase.value === 'stale') return
    const bad = args.find((a) => !SAFE_ARG.test(a))
    if (bad !== undefined) throw new Error(`非法任务参数：${bad}`)
    phase.value = 'starting'
    launchAt = Date.now()
    nowMs.value = launchAt
    result.value = null
    setTicking(true)
    const argv = args.map((a) => ` '${a}'`).join('')
    // setsid detaches from the manager's shell; nohup is the fallback when the
    // toybox build lacks setsid. Either way all fds go to /dev/null so the
    // bridge returns immediately.
    const cmd =
      `if command -v setsid >/dev/null 2>&1; then ` +
      `setsid sh '${script}'${argv} </dev/null >/dev/null 2>&1 & echo setsid; ` +
      `else nohup sh '${script}'${argv} </dev/null >/dev/null 2>&1 & echo nohup; fi`
    try {
      const r = await exec(cmd)
      debug(`${name} runner launched via ${r.stdout.trim() || '?'} (errno=${r.errno})`)
    } catch (e) {
      debug(`${name} runner launch failed: ${e instanceof Error ? e.message : String(e)}`)
      phase.value = 'launch_failed'
      setTicking(false)
      return
    }
    schedule()
  }

  async function clearStaleLock(): Promise<void> {
    const r = await exec(`rmdir '${lockPath}'`)
    debug(`cleared stale ${name} lock (errno=${r.errno})`)
    phase.value = 'idle'
    await check()
  }

  function dismiss(): void {
    if (!result.value) return
    dismissed.value = result.value.finishedAt
    writeMark(dismissedKey, result.value.finishedAt)
  }

  onBeforeUnmount(() => {
    disposed = true
    clearTimeout(pollTimer)
    setTicking(false)
  })

  return { phase, startedAt, result, busy, elapsedMs, overdue, showLast, probeError, check, start, clearStaleLock, dismiss }
}

