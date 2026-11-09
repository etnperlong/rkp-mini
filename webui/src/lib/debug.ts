import { reactive } from 'vue'

const MAX = 200

/** In-memory diagnostic lines, shown in Settings. Never holds credentials. */
export const debugLog = reactive<string[]>([])

export function debug(message: string): void {
  const d = new Date()
  const ts = d.toTimeString().slice(0, 8)
  debugLog.push(`${ts} ${message}`)
  if (debugLog.length > MAX) debugLog.splice(0, debugLog.length - MAX)
  console.debug('[rkp]', message)
}
