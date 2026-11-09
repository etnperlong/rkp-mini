// Dev-only stand-in for the KernelSU bridge. Pick a scenario with `?mock=<name>`.
// Never imported by production builds (see lib/ksu.ts).
import type { ExecResult } from '../lib/ksu'
import type { StatusData } from '../lib/rkp'
import { nowUnix } from '../lib/format'
import ready from './fixtures/status-ready.json'
import expiring from './fixtures/status-expiring.json'
import expired from './fixtures/status-expired.json'
import invalid from './fixtures/status-invalid.json'
import missing from './fixtures/status-missing.json'
import blocked from './fixtures/status-blocked.json'

export const MOCK_SCENARIOS = [
  'ready',
  'expiring',
  'expired',
  'invalid',
  'missing',
  'blocked',
  'status-exit',
  'status-parse',
  'status-timeout',
] as const

const scenario = new URLSearchParams(location.search).get('mock') ?? 'ready'

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

# Reserved; not implemented in this version. 0 = off.
auto_issue_days = 0
`

const logs: string[] = [`issue-${nowUnix() - 86_400 * 3}.log`, `issue-${nowUnix() - 86_400 * 40}.log`]

function logBody(name: string): string {
  return `# rkp-mini issue log (ok)
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
  return base
}

function ok(stdout: string): ExecResult {
  return { errno: 0, stdout, stderr: '' }
}

function fail(errno: number, stderr: string): ExecResult {
  return { errno, stdout: '', stderr }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

export async function mockExec(command: string): Promise<ExecResult> {
  await sleep(250)
  console.debug('[mock exec]', command)

  if (command.includes(' status --json')) {
    if (scenario === 'status-exit') return fail(2, 'Error: TOML parse error: expected `=`')
    if (scenario === 'status-parse') return ok('thread main panicked\n')
    if (scenario === 'status-timeout') return new Promise(() => {})
    return ok(JSON.stringify({ ok: true, data: liveStatus() }) + '\n')
  }
  if (command.startsWith('cat ') && command.includes('status.json')) {
    return ok(JSON.stringify(liveStatus(), null, 2))
  }
  if (command.endsWith(' --version')) return ok('rkp-mini 0.1.0\n')
  if (command.startsWith('cat ') && command.includes('config.toml')) return ok(CONFIG_TOML)
  if (command.startsWith('ls -1t ')) return ok(logs.join('\n') + '\n')
  if (command.startsWith('tail -n 200 ')) {
    const name = command.split('/').pop()?.replace(/'$/, '') ?? ''
    return logs.includes(name) ? ok(logBody(name)) : fail(1, `tail: ${name}: No such file`)
  }
  return fail(127, `mock: unhandled command: ${command}`)
}
