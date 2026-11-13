// Command plumbing shared by both apps: error type, path guard, module-dir
// resolution and the `--json` envelope parser.
import { moduleInfo } from 'kernelsu'
import { appConfig } from '@app-config'
import { hasKsu } from './ksu'
import { t } from './i18n'

/** exec: bridge failed or timed out; exit: non-zero exit; parse: stdout not a JSON envelope. */
export type CommandErrorKind = 'exec' | 'exit' | 'parse'

export class CommandError extends Error {
  constructor(
    readonly kind: CommandErrorKind,
    message: string,
    readonly detail = '',
  ) {
    super(message)
  }
}

/** Only plain absolute paths are ever interpolated into shell commands. */
export const SAFE_PATH = /^\/[A-Za-z0-9._/-]+$/

/** This module's directory from the manager, else the app's fixed install path. */
export function resolveModuleDir(): string {
  const fallback = appConfig.moduleDir
  if (!hasKsu()) return fallback
  try {
    const dir: unknown = JSON.parse(moduleInfo()).moduleDir
    if (typeof dir === 'string' && SAFE_PATH.test(dir)) return dir
  } catch {
    // Older managers: fall through to the fixed install path.
  }
  return fallback
}

/** Last non-empty stdout line parsed as JSON (stderr is diagnostics only). */
export function parseLastJsonLine<T>(stdout: string): T {
  const line = stdout
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .pop()
  if (!line) throw new CommandError('parse', t('shared.command.noOutput'), stdout)
  try {
    return JSON.parse(line) as T
  } catch {
    throw new CommandError('parse', t('shared.command.jsonParseFailed'), line)
  }
}
