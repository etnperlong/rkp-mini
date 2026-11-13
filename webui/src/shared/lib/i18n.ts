import { ref } from 'vue'
import { createI18n, type DefineLocaleMessage, type PickupPaths } from 'vue-i18n'

// zh-CN is the source language: every other locale file is typed against it
// (`satisfies typeof zhCN`), so a missing or extra key fails `vue-tsc`.
export const LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'de', 'fr', 'it'] as const
export type Locale = (typeof LOCALES)[number]
export type LocalePref = Locale | 'system'

/** Endonyms: each language is listed in its own script, whatever the UI language. */
export const LOCALE_NAMES: Record<Locale, string> = {
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  en: 'English',
  ja: '日本語',
  ko: '한국어',
  de: 'Deutsch',
  fr: 'Français',
  it: 'Italiano',
}

const SOURCE: Locale = 'zh-CN'
const KEY = 'rkp.locale'

type Messages = Record<string, unknown>
export interface LocaleModule {
  default: Messages
}
/** `import.meta.glob<LocaleModule>('./locales/*.ts')` of one message tree. */
export type LocaleLoaders = Record<string, () => Promise<LocaleModule>>

const sharedLoaders: LocaleLoaders = import.meta.glob<LocaleModule>('../locales/*.ts')

// Schema = `DefineLocaleMessage` (src/i18n.d.ts), so `t` rejects unknown keys.
export const i18n = createI18n<[DefineLocaleMessage], Locale, false>({
  legacy: false,
  locale: SOURCE,
  fallbackLocale: SOURCE,
  // Plural messages have exactly two forms (`singular | plural`). French uses the
  // singular for 0 and 1; the default rule treats 0 as plural.
  pluralRules: { fr: (n: number) => (n > 1 ? 1 : 0) },
})

/** Every leaf key of the schema, e.g. `shared.language.title` (index signature dropped). */
export type MessageKey = PickupPaths<{
  [K in keyof DefineLocaleMessage as string extends K ? never : K]: DefineLocaleMessage[K]
}>

const translate = i18n.global.t

/**
 * Translate; reactive inside render/computed. Use in `.vue` and `.ts` alike.
 * vue-i18n's own `t` accepts any string, this one rejects unknown keys at type-check.
 * `plural` (or a bare number) picks the `singular | plural` form and also binds `{n}`.
 */
export function t(key: MessageKey, arg?: number | Record<string, unknown>, plural?: number): string {
  if (arg === undefined) return translate(key)
  if (typeof arg === 'number') return translate(key, arg)
  return plural === undefined ? translate(key, arg) : translate(key, arg, plural)
}

/**
 * Translate a key assembled at runtime from a machine code (`shared.sync.code.${code}`);
 * `undefined` when the source locale has no such key, so callers fall back to the raw code.
 */
export function tCode(key: string): string | undefined {
  return i18n.global.te(key, SOURCE) ? translate(key) : undefined
}

/** The active locale (reactive), for `Intl` formatters. */
export function currentLocale(): Locale {
  return i18n.global.locale.value
}

/** The persisted choice; `system` follows the WebView's language list. */
export const localePref = ref<LocalePref>('system')

/** Map BCP 47 tags (`navigator.languages`) to a supported locale; English otherwise. */
export function systemLocale(tags: readonly string[] = navigator.languages ?? [navigator.language]): Locale {
  for (const tag of tags) {
    const lower = tag.toLowerCase()
    if (lower.startsWith('zh')) return /^zh-(tw|hk|mo|hant)/.test(lower) ? 'zh-TW' : 'zh-CN'
    const base = lower.split('-')[0]
    const hit = LOCALES.find((l) => l === base)
    if (hit) return hit
  }
  return 'en'
}

let appName = ''
let appLoaders: LocaleLoaders = {}
const loaded = new Set<Locale>()

async function load(locale: Locale): Promise<void> {
  if (loaded.has(locale)) return
  const file = async (loaders: LocaleLoaders, dir: string): Promise<Messages> => {
    const loader = loaders[`${dir}/${locale}.ts`]
    if (!loader) throw new Error(`missing locale file ${dir}/${locale}.ts`)
    return (await loader()).default
  }
  const [shared, app] = await Promise.all([file(sharedLoaders, '../locales'), file(appLoaders, './locales')])
  // Each bundle carries only `shared` and its own app tree, a subset of the schema.
  i18n.global.setLocaleMessage(locale, { shared, [appName]: app } as unknown as DefineLocaleMessage)
  loaded.add(locale)
}

let applySeq = 0

async function apply(pref: LocalePref): Promise<boolean> {
  const seq = ++applySeq
  const locale = pref === 'system' ? systemLocale() : pref
  await load(locale)
  // A newer selection loaded first: it owns the UI and the stored value.
  if (seq !== applySeq) return false
  i18n.global.locale.value = locale
  localePref.value = pref
  document.documentElement.lang = locale
  return true
}

/**
 * Load the source locale plus the persisted (or system) one before mounting.
 * `loaders` is the app's own `import.meta.glob('./locales/*.ts')`.
 */
export async function setupI18n(app: string, loaders: LocaleLoaders): Promise<typeof i18n> {
  appName = app
  appLoaders = loaders
  let pref: LocalePref = 'system'
  try {
    const stored = localStorage.getItem(KEY)
    if (stored && (LOCALES as readonly string[]).includes(stored)) pref = stored as Locale
  } catch {
    // Storage unavailable: follow the system.
  }
  await load(SOURCE)
  // A locale that fails to load must not leave a blank page: stay on the source locale.
  await apply(pref).catch((e: unknown) => console.error('locale load failed', e))
  return i18n
}

/**
 * Switch language at runtime and persist the choice.
 * `false` when a newer selection superseded this one before it applied.
 */
export async function saveLocale(pref: LocalePref): Promise<boolean> {
  if (!(await apply(pref))) return false
  try {
    if (pref === 'system') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, pref)
  } catch {
    // Non-persistent fallback is acceptable.
  }
  return true
}
