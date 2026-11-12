// Deployer-only commands: bin/rkp-mini-deployer.
// Local only — no network I/O, no age passphrase, and the argv carries no
// `deploy` word (the binary picks the handler). The `apply --auto` /
// `--if-staged` flags belong to the module's unattended script, not to this
// manual UI path.
import { appConfig } from '@app-config'
import { exec } from '../../../shared/lib/ksu'
import { CommandError, SAFE_PATH, parseLastJsonLine, resolveModuleDir } from '../../../shared/lib/command'
import {
  shq,
  type DeployPlan,
  type DeployReport,
  type SyncCall,
  type SyncEnvelope,
  type TargetReport,
} from '../../../shared/lib/sync'
import { deployErrorText } from './display'

export const DEPLOY_BIN = `${resolveModuleDir()}/bin/rkp-mini-deployer`
/** The deployer only ever reads and writes the receiver's configuration. */
export const DEPLOY_DATA = appConfig.dataRoot
export const DEPLOY_CONFIG = `${DEPLOY_DATA}/config.toml`
const DEPLOY_MODULE_DIR = resolveModuleDir()

for (const p of [DEPLOY_BIN, DEPLOY_CONFIG, DEPLOY_MODULE_DIR]) {
  if (!SAFE_PATH.test(p)) throw new Error(`unsafe path: ${p}`)
}

/** Deployer is receiver-only; the role is pinned here, never by callers. */
const PREFIX = `'${DEPLOY_BIN}' --role receiver --config '${DEPLOY_CONFIG}' --json`

async function callDeploy<T>(args: string): Promise<SyncCall<T>> {
  let r
  try {
    r = await exec(`${PREFIX} ${args}`)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
  return { envelope: parseLastJsonLine<SyncEnvelope<T>>(r.stdout), errno: r.errno, stderr: r.stderr }
}

async function deployData<T>(args: string): Promise<T> {
  const { envelope } = await callDeploy<T>(args)
  if (!envelope.ok) {
    throw new CommandError('exit', deployErrorText(envelope.error), JSON.stringify(envelope.error))
  }
  return envelope.data
}

/** Plain exec for the read-only facts on the 更多 tab (no `--json`). */
async function run(command: string): Promise<{ stdout: string; stderr: string; errno: number }> {
  try {
    return await exec(command)
  } catch (e) {
    throw new CommandError('exec', e instanceof Error ? e.message : String(e))
  }
}

/** `rkp-mini-deployer --version` prints `rkp-mini-deployer 0.1.0`; keep the version only. */
export async function readDeployVersion(): Promise<string> {
  const r = await run(`'${DEPLOY_BIN}' --version`)
  if (r.errno !== 0) throw new CommandError('exit', `退出码 ${r.errno}`, r.stderr.trim())
  return r.stdout.trim().split(/\s+/).pop() ?? ''
}

export interface ModuleProp {
  version: string
  versionCode: string
}

export async function readModuleProp(): Promise<ModuleProp> {
  const r = await run(`cat '${DEPLOY_MODULE_DIR}/module.prop'`)
  if (r.errno !== 0) throw new CommandError('exit', `读取失败（${r.errno}）`, r.stderr.trim())
  const get = (key: string): string => new RegExp(`^${key}=(.*)$`, 'm').exec(r.stdout)?.[1]?.trim() ?? '?'
  return { version: get('version'), versionCode: get('versionCode') }
}

/** Read-only config.toml text (the 更多 tab never edits it). */
export async function readDeployConfig(): Promise<string> {
  const r = await run(`cat '${DEPLOY_CONFIG}'`)
  if (r.errno !== 0) throw new CommandError('exit', '配置文件不存在', r.stderr.trim())
  return r.stdout
}

/** Read-only deployment plan (`rkp-mini-deployer --json plan`). */
export function readDeployPlan(): Promise<SyncCall<DeployPlan>> {
  return callDeploy<DeployPlan>('plan')
}

const SHA = /^[0-9a-f]{16,64}$/

export interface ApplyApproval {
  /** `DeployPlan.staged.keybox_sha256` the user reviewed for a first deployment. */
  confirmFirst?: string
  /** Same, for an identity change outside the rotation policy. */
  allowIdentityChange?: string
}

/** `apply`; approvals are bound to the reviewed keybox (§19.3). */
export function deployApply(approval: ApplyApproval = {}): Promise<SyncCall<DeployReport>> {
  let args = 'apply'
  for (const [flag, sha] of [
    ['--confirm-first', approval.confirmFirst],
    ['--allow-identity-change', approval.allowIdentityChange],
  ] as const) {
    if (sha === undefined) continue
    if (!SHA.test(sha)) throw new CommandError('exec', '非法的确认值')
    args += ` ${flag} ${sha}`
  }
  return callDeploy<DeployReport>(args)
}

export type BuiltinTarget = 'omk' | 'teesim' | 'tricky_store'

/** TEESimulator's watcher only reloads `*.xml` files directly inside this directory. */
const TEESIM_DIR = '/data/adb/teesim'
/** A plain `.xml` file name: no slash, no leading dot. */
const TEESIM_FILE = /^(?!\.)[A-Za-z0-9._-]{1,64}\.xml$/

export function isTeesimFileName(name: string): boolean {
  return TEESIM_FILE.test(name)
}

/** File name of a configured teesim target path, or `—` when it has none. */
export function teesimFileName(path: string | null | undefined): string {
  if (!path || !path.startsWith(`${TEESIM_DIR}/`)) return '—'
  return path.slice(TEESIM_DIR.length + 1)
}

/** The root module that reads a builtin target's keybox. */
export interface TargetModule {
  /** Directory name under /data/adb/modules. */
  id: string
  installed: boolean
  /** Installed and neither disabled nor queued for removal. */
  enabled: boolean
  /** `name=` / `version=` from its module.prop. */
  name: string | null
  version: string | null
}

export interface BuiltinTargetInfo {
  id: BuiltinTarget
  path: string
  owner: string
  mode: string
  module: TargetModule
}

/** `targets`: builtin targets plus module detection (read-only, no config needed). */
export async function readBuiltinTargets(): Promise<BuiltinTargetInfo[]> {
  const data = await deployData<{ builtin?: unknown }>('targets')
  if (!Array.isArray(data.builtin)) throw new CommandError('parse', '无法解析内置目标列表')
  return data.builtin as BuiltinTargetInfo[]
}

/**
 * `add-target`; only `teesim` takes a file name, and it must be a plain `.xml`
 * name: the runtime accepts nothing outside `/data/adb/teesim` (its watcher
 * only reloads `*.xml` there). Without a name the default keybox.xml is used.
 */
export function addTarget(id: BuiltinTarget, fileName?: string): Promise<unknown> {
  if (id !== 'teesim' || fileName === undefined) return deployData(`add-target ${id}`)
  if (!isTeesimFileName(fileName)) {
    throw new CommandError('exec', '文件名必须是 /data/adb/teesim 下的 .xml 文件名')
  }
  return deployData(`add-target teesim --path ${shq(`${TEESIM_DIR}/${fileName}`)}`)
}

/**
 * Did this report actually write a keybox? An idempotent `same` pass reports
 * `deployed: true` too, but only rewrites missing/damaged files — a target
 * whose content already matched comes back with reload_status `skipped`.
 */
export function reportWrote(report: DeployReport): boolean {
  const ok = report.targets.filter((t) => t.ok)
  return report.deployed && !report.pending && ok.length > 0 && ok.some((t) => t.reload_status !== 'skipped')
}

export function removeTarget(id: string): Promise<unknown> {
  return deployData(`remove-target ${shq(id)}`)
}

/** Deletes deployed keybox files; UI must confirm twice (§13.3). */
export function removeDeployed(ids: string[]): Promise<SyncCall<{ targets: TargetReport[] }>> {
  return callDeploy(`remove --yes${ids.map((i) => ` --target ${shq(i)}`).join('')}`)
}
