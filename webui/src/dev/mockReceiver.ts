import type { ExecResult } from '../lib/ksu'

export async function mockExec(command: string): Promise<ExecResult> {
  return { errno: 127, stdout: '', stderr: `mock: unhandled command: ${command}` }
}
