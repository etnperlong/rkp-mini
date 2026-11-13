<script setup lang="ts">
// Deployer page (rkp_mini_deployer module): bottom-navigation tabs
// 状态 / 部署 / 更多. The active tab is plain state, never history: the
// WebView's back key must keep closing the page (design §2.3).
// All sync commands go through shared/lib/sync.ts, local deployment through
// lib/deploy.ts and the combined run through lib/run.ts; secrets only through
// SecretDialog, and no keybox body or secret is ever rendered.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  MiuixButton,
  MiuixCard,
  MiuixIcon,
  MiuixNavigationBar,
  MiuixScrollArea,
  MiuixSmallTitle,
  MiuixSnackbarHost,
  MiuixText,
  MiuixTopAppBar,
  showSnackbar,
} from 'miuix-vue'
import { Download, Info, More } from 'miuix-vue/icons'
import DeployedCard from './components/DeployedCard.vue'
import DeployedDetailSheet from './components/DeployedDetailSheet.vue'
import AutoStatusRow from './components/AutoStatusRow.vue'
import ModulesCard from './components/ModulesCard.vue'
import DeployStatusCard from './components/DeployStatusCard.vue'
import DeploySettingsSheet from './components/DeploySettingsSheet.vue'
import MoreTab from './components/MoreTab.vue'
import ImportSheet from './components/ImportSheet.vue'
import TofuSheet from './components/TofuSheet.vue'
import DeploySheet from './components/DeploySheet.vue'
import RunResultSheet from './components/RunResultSheet.vue'
import LogsSheet from '../../shared/components/LogsSheet.vue'
import RiskSheet from '../../shared/components/RiskSheet.vue'
import SecretDialog from '../../shared/components/SecretDialog.vue'
import { bridgeAvailable } from '../../shared/lib/ksu'
import { nowUnix } from '../../shared/lib/format'
import { t, type MessageKey } from '../../shared/lib/i18n'
import { initTheme } from '../../shared/lib/theme'
import { errorMessage, isTofuFacts, receiverCheckLabel } from './lib/display'
import { readBuiltinTargets, type BuiltinTargetInfo } from './lib/deploy'
import { readRunSteps, runDeployOutcome, runOutcomeLabel, runRebootHint, type RunDeployOutcome, type RunSteps } from './lib/run'
import {
  readSyncConfig,
  readSyncStatus,
  syncErrorText,
  syncTaskArgs,
  useSyncRunner,
  type FetchResult,
  type ImportResult,
  type RemoteCheck,
  type SecretKind,
  type SyncConfig,
  type SyncEnvelope,
  type SyncOp,
  type SyncStatus,
  type SyncTaskResult,
  type TofuFacts,
} from '../../shared/lib/sync'

initTheme()

const available = bridgeAvailable()

const status = ref<SyncStatus | null>(null)
const config = ref<SyncConfig | null>(null)
/** Builtin target module detection (`rkp-mini-deployer targets`). */
const builtins = ref<BuiltinTargetInfo[] | null>(null)
const builtinError = ref<string | null>(null)
const loading = ref(false)
const statusError = ref<string | null>(null)
const notInitialized = ref(false)

const now = ref(nowUnix())
let clock: number | undefined

// Bottom navigation: one scroll area per tab (`v-show`) keeps each tab's
// scroll position; the tab itself is never URL state.
const TABS: { label: MessageKey }[] = [
  { label: 'deployer.app.tabStatus' },
  { label: 'deployer.app.tabDeploy' },
  { label: 'deployer.app.tabMore' },
]
const tabs = computed(() => TABS.map((x) => ({ label: t(x.label) })))
const TAB_ICONS = [Info, Download, More]
const MORE_TAB = 2
const tab = ref(0)

const importOpen = ref(false)
const settingsOpen = ref(false)
const deployOpen = ref(false)
const runOpen = ref(false)
const riskOpen = ref(false)
const secretOpen = ref(false)
const secretKind = ref<SecretKind>('age')
const detailOpen = ref(false)
const logsOpen = ref(false)
const tofuOpen = ref(false)
const tofuFacts = ref<TofuFacts | null>(null)
const tofuMismatch = ref(false)
/** The next successful fetch was started from TofuSheet: open DeploySheet after it. */
const tofuAccepted = ref(false)
const runSteps = ref<RunSteps | null>(null)
/** Deploy outcome of that run: drives the completion text and the reboot hint. */
const runOutcome = ref<RunDeployOutcome | null>(null)

const secretTitle = computed(() =>
  secretKind.value === 'age' ? t('deployer.app.secretAgeTitle') : t('deployer.app.secretWebdavTitle'),
)
const secretHint = computed(() =>
  secretKind.value === 'age' ? t('deployer.app.secretAgeHint') : t('deployer.app.secretWebdavHint'),
)

async function load(): Promise<void> {
  loading.value = true
  const [s, c, b] = await Promise.allSettled([readSyncStatus(), readSyncConfig(), readBuiltinTargets()])
  if (s.status === 'fulfilled') {
    const env = s.value.envelope
    if (env.ok) {
      status.value = env.data
      notInitialized.value = false
      statusError.value = null
    } else if (env.error.code === 'not_initialized') {
      status.value = null
      notInitialized.value = true
      statusError.value = null
    } else {
      status.value = null
      statusError.value = syncErrorText(env.error)
    }
  } else {
    status.value = null
    statusError.value = errorMessage(s.reason)
  }
  config.value = c.status === 'fulfilled' ? c.value : null
  if (b.status === 'fulfilled') {
    builtins.value = b.value
    builtinError.value = null
  } else {
    builtins.value = null
    builtinError.value = errorMessage(b.reason)
  }
  loading.value = false
  now.value = nowUnix()
}

// Long tasks run detached (webroot/runner/sync.sh) and are polled; every
// finished task — including one that ended while the page was closed — lands
// here exactly once.
const runner = useSyncRunner(async (result) => {
  await handleTask(result)
})

/** Op started from this page (null after a reload mid-task): only for button labels. */
const startedOp = ref<SyncOp | null>(null)
/** The running check was started by saving a password (decrypt test wording). */
const checkAfterSecret = ref(false)

async function collectRunSteps(): Promise<void> {
  try {
    runSteps.value = await readRunSteps()
    runOutcome.value = runDeployOutcome(runSteps.value)
  } catch {
    // Not a combined run (or a stale single-step snapshot): never reuse older steps.
    runSteps.value = null
    runOutcome.value = null
  }
}

/** TOFU facts travel in the fetch step's error `data`, as for a single fetch. */
function showTofuFrom(env: SyncEnvelope<unknown> | null): boolean {
  if (!env || env.ok) return false
  if (env.error.code === 'first_import_needs_confirmation') {
    const facts = isTofuFacts(env.data) ? env.data : null
    tofuFacts.value = facts
    tofuMismatch.value = false
    if (facts) {
      tofuOpen.value = true
      return true
    }
  }
  if (env.error.code === 'confirmation_mismatch') {
    tofuFacts.value = isTofuFacts(env.data) ? env.data : null
    tofuMismatch.value = true
    tofuOpen.value = true
    return true
  }
  return false
}

async function handleTask(result: SyncTaskResult): Promise<void> {
  await load()
  // Branch on the operation before dereferencing any envelope: a combined run
  // publishes per-step snapshots, never its own envelope.
  if (result.op === 'run') {
    await collectRunSteps()
    const fetchEnv = (runSteps.value?.fetch.envelope ?? null) as SyncEnvelope<unknown> | null
    if (showTofuFrom(fetchEnv)) return
    const outcome = runOutcome.value
    if (result.code !== 0) {
      void showSnackbar({ message: t('deployer.app.runFailed') })
    } else if (outcome) {
      const hint = runRebootHint(outcome)
      const label = runOutcomeLabel(outcome)
      void showSnackbar({ message: hint ? t('deployer.app.runDoneWithHint', { label, hint }) : label })
    } else {
      void showSnackbar({ message: t('deployer.run.done') })
    }
    return
  }
  const env = result.envelope
  const afterSecret = checkAfterSecret.value
  if (!env) {
    void showSnackbar({ message: t('deployer.app.taskEndNoJson', { code: result.code }) })
    return
  }
  if (showTofuFrom(env)) return
  if (!env.ok) {
    const detail = syncErrorText(env.error)
    void showSnackbar({ message: afterSecret ? t('deployer.app.passwordCheckFailed', { detail }) : detail })
    return
  }
  if (result.op === 'check') {
    const data = env.data as RemoteCheck
    const label = receiverCheckLabel(data.outcome)
    void showSnackbar({ message: afterSecret ? t('deployer.app.decryptOk', { label }) : t('deployer.app.cloudCheck', { label }) })
    return
  }
  const data = env.data as FetchResult
  if (tofuAccepted.value) {
    tofuAccepted.value = false
    void showSnackbar({ message: t('deployer.app.fetchedFirstDeploy', { seq: data.seq }) })
    deployOpen.value = true
    return
  }
  void showSnackbar({
    message: data.changed
      ? t('deployer.app.fetchedWaiting', { seq: data.seq })
      : t('deployer.app.noNewVersion', { seq: data.seq }),
  })
}

/** `afterSecret`: a saved password triggered this check, so its result reads as a decrypt test. */
async function startTask(op: SyncOp, afterSecret = false): Promise<void> {
  startedOp.value = op
  checkAfterSecret.value = afterSecret
  try {
    await runner.start(syncTaskArgs(op))
  } catch (e) {
    void showSnackbar({ message: errorMessage(e) })
  }
}

async function startFetch(acceptFirst?: string): Promise<void> {
  startedOp.value = 'fetch'
  checkAfterSecret.value = false
  try {
    await runner.start(syncTaskArgs('fetch', acceptFirst))
  } catch (e) {
    void showSnackbar({ message: errorMessage(e) })
  }
}

const canRunTask = computed(
  () => !!status.value?.stream_id && !loading.value && !runner.busy.value && runner.phase.value !== 'stale',
)
/** `启用` replaces 立即部署 until the stream is enabled with the current risk text. */
const needsEnable = computed(() => !!status.value && (!status.value.enabled || !status.value.risk_ack_current))
const canDeploy = computed(() => canRunTask.value && !!status.value?.enabled && !!status.value.risk_ack_current)

function openImport(): void {
  settingsOpen.value = false
  importOpen.value = true
}

function openSecret(kind: SecretKind): void {
  secretKind.value = kind
  secretOpen.value = true
}

/** A receiver password is only known to be right once the remote decrypts: check right away. */
async function onSecretSaved(): Promise<void> {
  await load()
  if (canRunTask.value) await startTask('check', true)
}

function onImported(_result: ImportResult): void {
  void load()
}

function onTofuAccept(plaintextSha256: string): void {
  tofuOpen.value = false
  tofuAccepted.value = true
  void startFetch(plaintextSha256)
}

function onTofuRetry(): void {
  tofuOpen.value = false
  void startFetch()
}

onMounted(async () => {
  clock = window.setInterval(() => (now.value = nowUnix()), 60_000)
  if (!available) return
  await runner.check()
  await load()
  await collectRunSteps()
})

onBeforeUnmount(() => clearInterval(clock))
</script>

<template>
  <div class="app">
    <MiuixTopAppBar :title="t('deployer.app.title')" />

    <MiuixScrollArea v-if="!available" v-show="tab !== MORE_TAB" class="app__scroll">
      <main class="page">
        <MiuixCard>
          <div class="notice">
            <MiuixText type="title4" weight="medium" color="var(--m-color-error)">{{ t('deployer.app.noBridgeTitle') }}</MiuixText>
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              {{ t('deployer.app.noBridgeBody') }}
            </MiuixText>
          </div>
        </MiuixCard>
      </main>
    </MiuixScrollArea>

    <!-- One scroll area per tab (v-show) so each keeps its own scroll position. -->
    <template v-else>
      <MiuixScrollArea v-show="tab === 0" class="app__scroll">
        <main class="page">
          <DeployedCard
            :status="status"
            :not-initialized="notInitialized"
            :loading="loading"
            :error="statusError"
            :runner="runner"
            :busy-op="startedOp"
            :now="now"
            @detail="detailOpen = true"
            @refresh="load"
          />
          <AutoStatusRow
            :status="status"
            :config="config"
            :loading="loading"
            :error="statusError"
            @open="tab = $event"
          />
          <MiuixSmallTitle :text="t('deployer.app.installedModules')" />
          <ModulesCard :builtins="builtins" :error="builtinError" :status="status" :loading="loading" />
        </main>
      </MiuixScrollArea>

      <MiuixScrollArea v-show="tab === 1" class="app__scroll">
        <main class="page">
          <DeployStatusCard
            :status="status"
            :not-initialized="notInitialized"
            :loading="loading"
            :error="statusError"
            :config="config"
            :runner="runner"
            :busy-op="startedOp"
            :now="now"
            :can-refresh="canRunTask"
            :run-outcome="runOutcome"
            @import="openImport"
            @check="startTask('check')"
            @run="startTask('run')"
            @deploy="deployOpen = true"
            @risk="riskOpen = true"
            @secret="openSecret"
            @result="runOpen = true"
          />
          <div v-if="status?.stream_id" class="actions">
            <MiuixButton v-if="needsEnable" type="primary" @click="riskOpen = true">{{ t('deployer.app.enable') }}</MiuixButton>
            <MiuixButton v-else type="primary" :disabled="!canDeploy" @click="startTask('run')">{{ t('deployer.app.deployNow') }}</MiuixButton>
            <MiuixButton :disabled="runner.busy.value" @click="settingsOpen = true">{{ t('deployer.app.settings') }}</MiuixButton>
          </div>
          <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
            {{ t('deployer.app.cloneFootnote') }}
          </MiuixText>
        </main>
      </MiuixScrollArea>
    </template>

    <MiuixScrollArea v-show="tab === MORE_TAB" class="app__scroll">
      <main class="page">
        <MoreTab :active="tab === MORE_TAB" @logs="logsOpen = true" />
      </main>
    </MiuixScrollArea>

    <MiuixNavigationBar :model-value="tab" :items="tabs" @update:model-value="tab = $event">
      <template #icon="{ index }">
        <MiuixIcon :icon="TAB_ICONS[index]" />
      </template>
    </MiuixNavigationBar>

    <MiuixSnackbarHost />

    <template v-if="available">
      <ImportSheet v-model="importOpen" @imported="onImported" @risk="riskOpen = true" @secret="openSecret" />
      <TofuSheet v-model="tofuOpen" :facts="tofuFacts" :mismatch="tofuMismatch" @accept="onTofuAccept" @retry="onTofuRetry" />
      <DeploySheet v-model="deployOpen" @changed="load" />
      <DeploySettingsSheet
        v-model="settingsOpen"
        :status="status"
        :busy="runner.busy.value"
        @secret="openSecret"
        @import="openImport"
        @changed="load"
      />
      <DeployedDetailSheet v-model="detailOpen" :status="status" :now="now" />
      <RunResultSheet v-model="runOpen" :steps="runSteps" />
      <RiskSheet v-model="riskOpen" @enabled="load" />
      <SecretDialog v-model="secretOpen" :kind="secretKind" :title="secretTitle" :hint="secretHint" @saved="onSecretSaved" />
    </template>
    <LogsSheet v-model="logsOpen" />
  </div>
</template>

<style scoped>
.app {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.app__scroll {
  flex: 1;
  min-height: 0;
}
.page {
  max-width: 640px;
  margin: 0 auto;
  padding: 4px 12px 24px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.page :deep(.m-small-title) {
  padding: 4px 16px 0;
}
.actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
.notice {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.footnote {
  padding: 4px 16px 0;
}
/* The snackbar host is teleported to <body> with bottom: 0; keep it above the navigation bar (64px items + 1px divider). */
:global(.m-snackbar-host) {
  bottom: 65px;
}
</style>
