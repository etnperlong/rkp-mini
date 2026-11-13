// Bridge between the WebUI language picker and the owning Rust binary.
//
// The page owns the choice; each app persists the *resolved* canonical locale
// in `<dataRoot>/ui-language` (0600, atomic rename) so the native side can use
// the last effective locale without any global state. Refreshing the
// module.prop description is then delegated to the binary: each app's
// `update-description` (the issuer's skipped while an issuance holds
// issue.lock) renders and writes the prop without any status side effects.
//
// Writes are serialized: a slow older change can never overwrite a newer one.
import { appConfig } from '@app-config'
import { LOCALES, currentLocale, saveLocale, t, type Locale, type LocalePref } from './i18n'
import { CommandError, SAFE_PATH, resolveModuleDir } from './command'
import { bridgeAvailable, exec } from './ksu'

const DATA = appConfig.dataRoot
const LOCALE_FILE = `${DATA}/ui-language`
const LOCALE_TMP = `${DATA}/ui-language.tmp`
const BIN = `${resolveModuleDir()}/bin/rkp-mini-${appConfig.syncRole === 'sender' ? 'issuer' : 'deployer'}`
const ISSUE_LOCK = `${DATA}/tmp/issue.lock`
const SENDER = appConfig.syncRole === 'sender'

for (const p of [DATA, LOCALE_FILE, LOCALE_TMP, BIN, ...(SENDER ? [ISSUE_LOCK] : [])]) {
  if (!SAFE_PATH.test(p)) throw new Error(`unsafe path: ${p}`)
}

/** One line, 0600, atomic rename; the directory is root-private. */
function persistCommand(locale: Locale): string {
  return (
    `umask 077; mkdir -p '${DATA}' && chmod 700 '${DATA}' && ` +
    `printf '%s\\n' '${locale}' > '${LOCALE_TMP}' && chmod 600 '${LOCALE_TMP}' && ` +
    `mv -f '${LOCALE_TMP}' '${LOCALE_FILE}'`
  )
}

/** Native owns the description text; the page only asks for a refresh. */
const REFRESH = SENDER
  ? `if [ ! -d '${ISSUE_LOCK}' ]; then '${BIN}' update-description; fi`
  : `'${BIN}' update-description`

async function writeLocale(locale: Locale): Promise<void> {
  const r = await exec(persistCommand(locale))
  if (r.errno !== 0) {
    throw new CommandError('exit', t('shared.command.writeFailed'), (r.stderr || r.stdout).trim())
  }
}

async function refreshDescription(): Promise<void> {
  const r = await exec(REFRESH)
  if (r.errno !== 0) {
    throw new CommandError('exit', t('shared.language.syncFailed'), (r.stderr || r.stdout).trim())
  }
}

let queue: Promise<void> = Promise.resolve()

/**
 * Persist the effective locale and refresh the module description.
 * The default is the locale Vue actually activated, including a fallback when
 * the requested catalog failed to load. Queued, newest change wins.
 */
export function syncModuleLocale(locale: Locale = currentLocale()): Promise<void> {
  if (!bridgeAvailable()) return Promise.resolve()
  // A runtime guard for the type: the locale is interpolated into a shell
  // command, so only the known canonical codes may ever reach it.
  if (!(LOCALES as readonly string[]).includes(locale)) {
    return Promise.reject(new CommandError('exec', `unsupported locale: ${locale}`))
  }
  const run = queue.then(async () => {
    await writeLocale(locale)
    await refreshDescription()
  })
  queue = run.catch(() => {})
  return run.catch((e: unknown) => {
    throw e instanceof CommandError ? e : new CommandError('exec', e instanceof Error ? e.message : String(e))
  })
}

/** Resolves once every locale sync queued so far has settled; never rejects. */
export function localeSyncSettled(): Promise<void> {
  return queue
}

/**
 * Change the UI language, then sync; a newer selection supersedes an older
 * one before it reaches the native side (`saveLocale` reports that).
 */
export async function changeModuleLocale(pref: LocalePref): Promise<void> {
  if (await saveLocale(pref)) await syncModuleLocale()
}
