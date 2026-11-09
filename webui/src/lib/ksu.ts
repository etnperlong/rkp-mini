import { exec as ksuExec } from 'kernelsu'

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
    timer = window.setTimeout(() => reject(new Error(`命令超时（${timeoutMs / 1000}s）`)), timeoutMs)
  })
  try {
    return await Promise.race([ksuExec(command), timeout])
  } finally {
    clearTimeout(timer)
  }
}
