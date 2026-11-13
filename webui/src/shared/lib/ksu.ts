import { exec as ksuExec } from 'kernelsu'
import { t } from './i18n'

export interface ExecResult {
  errno: number
  stdout: string
  stderr: string
}

const TIMEOUT_MS = 10_000

/** True when running inside the KernelSU manager WebView. */
export function hasKsu(): boolean {
  return typeof window !== 'undefined' && 'ksu' in window
}

/** Dev server in a plain browser: commands are answered by fixtures. */
export const MOCK = import.meta.env.DEV && !hasKsu()

/** Whether commands can run at all (real bridge or dev mock). */
export function bridgeAvailable(): boolean {
  return MOCK || hasKsu()
}

async function dispatch(command: string): Promise<ExecResult> {
  if (import.meta.env.DEV && MOCK) {
    // Dynamic so the dev-only mocks and their fixtures never reach a production
    // bundle; `@app-mock` resolves to the app being built.
    const { mockExec } = await import('@app-mock')
    return mockExec(command)
  }
  return ksuExec(command)
}

/**
 * Run a shell command as root through the KernelSU bridge.
 * The bridge has no timeout of its own; this only stops waiting, it never
 * kills the command. Long-running work (issuance) must not go through here.
 */
export async function exec(command: string, timeoutMs = TIMEOUT_MS): Promise<ExecResult> {
  let timer: number | undefined
  // Executor form rather than Promise.withResolvers: the build targets
  // es2020 and older manager WebViews lack withResolvers.
  const timeout = new Promise<never>((_, reject) => {
    timer = window.setTimeout(
      () => reject(new Error(t('shared.command.timeout', { seconds: timeoutMs / 1000 }))),
      timeoutMs,
    )
  })
  try {
    return await Promise.race([dispatch(command), timeout])
  } finally {
    clearTimeout(timer)
  }
}
