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
export const DEPLOY_CONFIG = `${appConfig.dataRoot}/config.toml`

for (const p of [DEPLOY_BIN, DEPLOY_CONFIG]) {
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

export function addTarget(id: BuiltinTarget): Promise<unknown> {
  return deployData(`add-target ${id}`)
}

export function removeTarget(id: string): Promise<unknown> {
  return deployData(`remove-target ${shq(id)}`)
}

/** Deletes deployed keybox files; UI must confirm twice (§13.3). */
export function removeDeployed(ids: string[]): Promise<SyncCall<{ targets: TargetReport[] }>> {
  return callDeploy(`remove --yes${ids.map((i) => ` --target ${shq(i)}`).join('')}`)
}
