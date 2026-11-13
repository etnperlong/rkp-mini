import { currentLocale, t } from './i18n'

/** Current Unix time in seconds. */
export function nowUnix(): number {
  return Math.floor(Date.now() / 1000)
}

/** Whole days from `now` to `notAfter`, floored; negative = expired (matches util::days_left). */
export function daysLeft(notAfter: number, now = nowUnix()): number {
  return Math.floor((notAfter - now) / 86_400)
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

/** Local `YYYY-MM-DD`. */
export function formatDate(unix: number): string {
  const d = new Date(unix * 1000)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

/** Local `YYYY-MM-DD HH:mm`. */
export function formatDateTime(unix: number): string {
  const d = new Date(unix * 1000)
  return `${formatDate(unix)} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** `m:ss` for an elapsed duration. */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  return `${Math.floor(s / 60)}:${pad(s % 60)}`
}

/** Coarse localized relative time for a past Unix timestamp. */
export function formatAgo(unix: number, now = nowUnix()): string {
  const s = now - unix
  if (s < 60) return t('shared.time.justNow')
  const rtf = new Intl.RelativeTimeFormat(currentLocale(), { numeric: 'always' })
  if (s < 3600) return rtf.format(-Math.floor(s / 60), 'minute')
  if (s < 86_400) return rtf.format(-Math.floor(s / 3600), 'hour')
  return rtf.format(-Math.floor(s / 86_400), 'day')
}

/** Keep head and tail of a long identifier: `abcdef…uvwxyz`. */
export function truncateMiddle(text: string, keep = 18): string {
  if (text.length <= keep * 2 + 1) return text
  return `${text.slice(0, keep)}…${text.slice(-keep)}`
}
