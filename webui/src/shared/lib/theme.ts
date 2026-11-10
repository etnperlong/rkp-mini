import { setThemeMode, type ThemeMode } from 'miuix-vue'

const KEY = 'rkp.theme'
const MODES: readonly ThemeMode[] = ['system', 'light', 'dark']

/** Apply the persisted mode (miuix-vue itself does not persist it). */
export function initTheme(): ThemeMode {
  let mode: ThemeMode = 'system'
  try {
    const stored = localStorage.getItem(KEY)
    if (stored && (MODES as readonly string[]).includes(stored)) mode = stored as ThemeMode
  } catch {
    // Storage unavailable: follow the system.
  }
  setThemeMode(mode)
  return mode
}

export function saveTheme(mode: ThemeMode): void {
  setThemeMode(mode)
  try {
    localStorage.setItem(KEY, mode)
  } catch {
    // Non-persistent fallback is acceptable.
  }
}
