// Dev-only stand-in for the KernelSU bridge. Never imported by production
// builds (see shared/lib/ksu.ts). Query parameters select the scenario:
//   ?mock=    ready | expiring | expired | invalid | missing | blocked
//             | status-exit | status-parse | status-timeout     (status read)
//   ?runner=  idle | running | overdue | success | failure | crashed | stale
//                                                         (state at page load)
//   ?outcome= success | failure                (result of an issue started here)
//   ?auto=    off | on | stopped  (auto-issue: off = 0 days; stopped = on, loop down)
//   ?autolast= none | skipped | ok | fail   (last auto-issue run in tmp/auto-issue.last.*)
//   ?sync=    uninit | disabled | ready | error | nosecret        (sync state)
//   ?syncpush= ok | noop | fail                 (result of one push task)
//   ?remote=  none | in_sync | behind | ahead | diverged | missing | error
//                                            (cached remote_check; none = null)
//   ?autopush= on | off                                      (default on)
//   ?locale=  ok | fail   (module description refresh; `runner=running` models
//                          the busy issuer skipping it while the locale is saved)
// rkp-mini-sync commands are answered by ./mockSyncSender.ts.
import type { ExecResult } from '../../../shared/lib/ksu'
import type { StatusData } from '../lib/rkp'
import { nowUnix } from '../../../shared/lib/format'
import { mockSync, mockLoop } from './mockSyncSender'
import ready from './fixtures/status-ready.json'
import expiring from './fixtures/status-expiring.json'
import expired from './fixtures/status-expired.json'
import invalid from './fixtures/status-invalid.json'
import missing from './fixtures/status-missing.json'
import blocked from './fixtures/status-blocked.json'
import issueSuccess from './fixtures/issue-success.json'
import issueFailure from './fixtures/issue-failure.json'

const params = new URLSearchParams(location.search)
let scenario = params.get('mock') ?? 'ready'
const outcome = params.get('outcome') ?? 'success'
const localeMode = params.get('locale') ?? 'ok'
/** Observations for the module-language bridge; names are part of the dev harness. */
const MODULE_LOCALE_KEY = 'rkp.mock.moduleLocale'
const MODULE_SYNC_KEY = 'rkp.mock.moduleSync'
const MODULE_COMMANDS_KEY = 'rkp.mock.moduleCommands'

const STATUS: Record<string, StatusData> = {
  ready: ready as StatusData,
  expiring: expiring as StatusData,
  expired: expired as StatusData,
  invalid: invalid as StatusData,
  missing: missing as StatusData,
  blocked: blocked as StatusData,
}

const CONFIG_TOML = `# RKP Mini configuration
server_url = "https://remoteprovisioning.googleapis.com/v1"
fingerprint = ""                  # empty = read ro.build.fingerprint
include_fingerprint = true        # append {"fingerprint": ...} to the CSR
output_dir = "/data/misc/keystore/rkp-mini/output"
request_timeout_seconds = 30
ta_timeout_seconds = 30
proxy = ""                        # e.g. "http://127.0.0.1:7890"; empty = direct
log_retention = 10                # keep the most recent N issue logs

# Re-issue when the current leaf has <= N days left. Used only by the module's
# auto-loop.sh (\`issue --if-expiring-days N\`). 0 = off.
auto_issue_days = 0
`

// Auto-issue is issuer-owned now (C1/C2): setting + whether auto-loop.sh is
// alive. Both survive for this session; the loop flag also comes from the sync
// mock, which is what actually handles the `auto-loop.sh` launch.
const autoParam = params.get('auto') ?? 'off'
const autoIssue = { days: autoParam === 'off' ? 0 : 3, loop_running: autoParam === 'on' }

const logs: string[] = [`issue-${nowUnix() - 86_400 * 3}.log`, `issue-${nowUnix() - 86_400 * 40}.log`]

function logBody(name: string): string {
  return `# rkp-mini-issuer issue log (ok)
2026-11-06 10:00:00 UTC issue start
2026-11-06 10:00:00 UTC prereq ok
2026-11-06 10:00:01 UTC challenge 32 bytes
2026-11-06 10:00:02 UTC ta_status=0 frame=status+len
2026-11-06 10:00:02 UTC csr 1412 bytes
2026-11-06 10:00:04 UTC chain 3 certificates
2026-11-06 10:00:04 UTC issue ok
(${name})
`
}

/** Status fixture with expiry re-anchored to the current time. */
function liveStatus(): StatusData {
  const base = structuredClone(STATUS[scenario] ?? STATUS.ready)
  const now = nowUnix()
  base.generated_at = now
  const k = base.keybox
  if (k.days_left !== null) {
    k.not_after = now + k.days_left * 86_400 + 3600
    k.not_after_utc = new Date(k.not_after * 1000).toISOString().replace('T', ' ').slice(0, 19) + ' UTC'
  }
  base.auto_issue = { days: autoIssue.days, loop_running: autoIssue.loop_running || mockLoop.running }
  return base
}

// ---- Runner state files (tmp/issue.*) ------------------------------------------

interface RunnerFiles {
  lock: number | null
  code: number | null
  end: number | null
  json: string
  err: string
}

function successJson(): string {
  const env = structuredClone(issueSuccess)
  env.data.leaf_not_after = nowUnix() + 13 * 86_400
  env.data.leaf_not_after_utc = new Date(env.data.leaf_not_after * 1000).toISOString().slice(0, 19) + 'Z'
  return JSON.stringify(env) + '\n'
}

const FAILURE_ERR = 'warning: KeyMint returned -18\n❌ [ta] KeyMint -18 INCOMPATIBLE_KEY_FORMAT\n'

function initialFiles(): RunnerFiles {
  const now = nowUnix()
  const idle: RunnerFiles = { lock: null, code: null, end: null, json: '', err: '' }
  switch (params.get('runner')) {
    case 'running':
      return { ...idle, lock: now - 15 }
    case 'overdue':
      return { ...idle, lock: now - 130 }
    case 'stale':
      return { ...idle, lock: now - 900 }
    case 'success':
      return { ...idle, code: 0, end: now - 3600, json: successJson() }
    case 'failure':
      return { ...idle, code: 5, end: now - 600, json: JSON.stringify(issueFailure) + '\n', err: FAILURE_ERR }
    case 'crashed':
      return { ...idle, code: 137, end: now - 120, err: 'Killed\n' }
    default:
      return idle
  }
}

const files = initialFiles()

// ---- Auto-issue loop files (tmp/auto-issue.last.*) ------------------------------

interface AutoLastFiles {
  at: number | null
  code: number | null
  json: string
  err: string
}

function initialAutoLast(): AutoLastFiles {
  const now = nowUnix()
  switch (params.get('autolast')) {
    case 'skipped':
      return {
        at: now - 3600,
        code: 0,
        json: `${JSON.stringify({
          ok: true,
          data: {
            skipped: true,
            reason: 'not_expiring',
            days_left: 9,
            threshold_days: 3,
            keybox: '/data/misc/keystore/rkp-mini/output/keybox.xml',
          },
        })}\n`,
        err: '',
      }
    case 'ok':
      return { at: now - 7200, code: 0, json: successJson(), err: '' }
    case 'fail':
      return { at: now - 86_400, code: 5, json: `${JSON.stringify(issueFailure)}\n`, err: FAILURE_ERR }
    default:
      return { at: null, code: null, json: '', err: '' }
  }
}

const autoLast = initialAutoLast()

function launch(): void {
  if (files.lock !== null) return // runner exits 9: already running
  setTimeout(() => {
    files.lock = nowUnix()
    files.code = files.end = null
    files.json = files.err = ''
  }, 400)
  setTimeout(() => {
    if (outcome === 'failure') {
      files.json = JSON.stringify(issueFailure) + '\n'
      files.err = FAILURE_ERR
      files.code = 5
    } else {
      files.json = successJson()
      files.code = 0
      scenario = 'ready'
    }
    files.end = nowUnix()
    files.lock = null
  }, 4500)
}

function probeOutput(): string {
  const lines = [`now=${nowUnix()}`]
  if (files.lock !== null) lines.push(`lock=${files.lock}`)
  if (files.code !== null) lines.push(`code=${files.code}`, `end=${files.end}`)
  return lines.join('\n') + '\n'
}

// ---- Dispatcher -----------------------------------------------------------------

function ok(stdout: string): ExecResult {
  return { errno: 0, stdout, stderr: '' }
}

function fail(errno: number, stderr: string): ExecResult {
  return { errno, stdout: '', stderr }
}

/** Redact the secret hex the inbox writes put on the command line. */
function sanitized(command: string): string {
  return command.replace(/(echo )[0-9a-f]+( >)/g, '$1<redacted>$2').replace(/[0-9a-f]{32,}/g, '<redacted>')
}

/** Model the native file + the last refresh invocation for browser assertions. */
function recordModuleSync(
  command: string,
  action: 'persist' | 'refresh',
  ok: boolean,
  code = ok ? 0 : 1,
  skipped?: string,
): void {
  const entry = {
    command: sanitized(command),
    action,
    locale: localStorage.getItem(MODULE_LOCALE_KEY)?.trim() ?? '',
    ok,
    code,
    at: nowUnix(),
    ...(skipped ? { skipped } : {}),
    // What a successful refresh would have written to module.prop (fixture data,
    // not a translated catalog).
    ...(action === 'refresh' && ok && !skipped ? { description: liveStatus().description } : {}),
  }
  try {
    localStorage.setItem(MODULE_SYNC_KEY, JSON.stringify(entry))
    const prev = JSON.parse(localStorage.getItem(MODULE_COMMANDS_KEY) ?? '[]') as unknown
    const list = Array.isArray(prev) ? prev.filter((x): x is string => typeof x === 'string') : []
    localStorage.setItem(MODULE_COMMANDS_KEY, JSON.stringify([...list.slice(-9), entry.command]))
  } catch {
    /* preview only */
  }
}

/** Commands owned by the remote-sync mock: sync binary, runner, loop, inbox, log. */
const SYNC_COMMAND = /\/bin\/rkp-mini-sync|runner\/sync\.sh|sync-task\.|auto-loop\.sh|sync\/inbox|logs\/auto\.log/
const SEP = '<<<RKP-SEP>>>'

export async function mockExec(command: string): Promise<ExecResult> {
  await new Promise((resolve) => setTimeout(resolve, 200))
  console.debug('[mock exec]', sanitized(command))

  // Sync commands first: their paths share the module's `bin/` and `logs/`.
  if (SYNC_COMMAND.test(command)) return mockSync(command)

  // Module-language bridge (shared/lib/moduleLocale.ts): write the locale file,
  // then refresh module.prop unless an issuance holds the lock.
  if (command.includes('/ui-language.tmp') && command.includes('/ui-language')) {
    const locale = /printf '%s\\n' '([^']+)'/.exec(command)?.[1] ?? ''
    try {
      localStorage.setItem(MODULE_LOCALE_KEY, locale)
    } catch {
      /* preview only */
    }
    recordModuleSync(command, 'persist', true)
    return ok('')
  }
  if (command.includes(' update-description')) {
    if (files.lock !== null) {
      recordModuleSync(command, 'refresh', true, 0, 'issue.lock')
      return ok('')
    }
    if (localeMode === 'fail') {
      recordModuleSync(command, 'refresh', false, 7)
      return fail(7, 'mock: update-description failed\n')
    }
    recordModuleSync(command, 'refresh', true)
    return ok('')
  }

  // Auto-issue is the issuer's own setting (C1) and state (C6), mocked here.
  if (command.includes(' config set auto_issue_days ')) {
    const n = Number(/config set auto_issue_days (\d+) --json/.exec(command)?.[1] ?? NaN)
    if (!Number.isInteger(n) || n < 0 || n > 365) {
      const error = { stage: 'config', code: 'invalid_value', message: 'auto_issue_days must be 0..=365' }
      return { errno: 2, stdout: `${JSON.stringify({ ok: false, error })}\n`, stderr: '' }
    }
    autoIssue.days = n
    return ok(`${JSON.stringify({ ok: true, data: { auto_issue_days: n } })}\n`)
  }
  if (command.includes(' remove --yes --json')) {
    const dir = '/data/misc/keystore/rkp-mini/output'
    const had = (STATUS[scenario] ?? STATUS.ready).keybox.exists
    const names = ['keybox.xml', 'keybox.xml.prev', 'private-key.pem', 'cert-chain.pem', 'cert-chain.der', 'meta.json']
    scenario = 'missing'
    return ok(`${JSON.stringify({ ok: true, data: { removed: had ? names.map((n) => `${dir}/${n}`) : [], output_dir: dir } })}\n`)
  }
  if (command.includes('/auto-issue.last.code')) {
    return ok(
      [SEP, autoLast.at ?? '', SEP, autoLast.code ?? '', SEP, autoLast.json.trimEnd(), SEP, autoLast.err].join('\n'),
    )
  }

  if (command.includes(' status --json')) {
    if (scenario === 'status-exit') return fail(2, 'Error: TOML parse error: expected `=`')
    if (scenario === 'status-parse') return ok('thread main panicked\n')
    if (scenario === 'status-timeout') return new Promise(() => {})
    return ok(JSON.stringify({ ok: true, data: liveStatus() }) + '\n')
  }
  if (command.startsWith('cat ') && command.includes('status.json')) {
    return ok(JSON.stringify(liveStatus(), null, 2))
  }
  if (command.endsWith(' --version')) return ok('rkp-mini-issuer 0.4.0\n')
  if (command.startsWith('cat ') && command.includes('config.toml')) return ok(CONFIG_TOML)
  if (command.startsWith('cat ') && command.includes('module.prop')) {
    return ok(
      'id=rkp_mini_issuer\nname=RKP-Mini Issuer\nversion=v0.4.0\nversionCode=5\nauthor=Evans Mike\n',
    )
  }
  if (command.startsWith('ls -1t ')) return ok(logs.join('\n') + '\n')
  if (command.startsWith('tail -n 200 ')) {
    const name = command.split('/').pop()?.replace(/'$/, '') ?? ''
    return logs.includes(name) ? ok(logBody(name)) : fail(1, `tail: ${name}: No such file`)
  }
  if (command.includes('now=$(date +%s)') && command.includes("'issue.lock'")) return ok(probeOutput())
  if (command.includes('/issue.last.json')) return ok(`${SEP}\n${files.json}${SEP}\n${files.err}`)
  if (command.includes('command -v setsid') && command.includes('/issue.sh')) {
    launch()
    return ok('setsid\n')
  }
  if (command.startsWith('rmdir ') && command.includes('issue.lock')) {
    files.lock = null
    return ok('')
  }
  return fail(127, `mock: unhandled command: ${command}`)
}
