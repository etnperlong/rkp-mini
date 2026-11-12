<script setup lang="ts">
// Deployer page (rkp_mini_deployer module): bottom-navigation tabs
// 状态 / 部署 / 更多. The active tab is plain state, never history: the
// WebView's back key must keep closing the page (design §2.3).
// All sync commands go through shared/lib/sync.ts, local deployment through
// lib/deploy.ts and the combined run through lib/run.ts; secrets only through
// SecretSheet, and no keybox body or secret is ever rendered.
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
import SecretSheet from '../../shared/components/SecretSheet.vue'
import { bridgeAvailable } from '../../shared/lib/ksu'
import { nowUnix } from '../../shared/lib/format'
import { initTheme } from '../../shared/lib/theme'
import { RECEIVER_CHECK_LABEL, errorMessage, isTofuFacts } from './lib/display'
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
const TABS = [{ label: '状态' }, { label: '部署' }, { label: '更多' }]
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

const secretTitle = computed(() => (secretKind.value === 'age' ? '设置密码' : '设置 WebDAV 密码'))
const secretHint = computed(() =>
  secretKind.value === 'age'
    ? '发送端设置的密码，用来解密云端储存的 Keybox；请通过安全渠道获得，每个同步流使用独立密码。'
    : 'WebDAV 应用密码（不是账号登录密码），与链接中的用户名配套；只保存在本机 0600 文件中。',
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
      void showSnackbar({ message: '任务失败，详情见最近一次组合结果' })
    } else if (outcome) {
      const hint = runRebootHint(outcome)
      void showSnackbar({ message: `${runOutcomeLabel(outcome)}${hint ? `，${hint}` : ''}` })
    } else {
      void showSnackbar({ message: '运行完成' })
    }
    return
  }
  const env = result.envelope
  if (!env) {
    void showSnackbar({ message: `任务结束（exit ${result.code}），但没有收到 JSON 结果` })
    return
  }
  if (showTofuFrom(env)) return
  if (!env.ok) {
    void showSnackbar({ message: syncErrorText(env.error) })
    return
  }
  if (result.op === 'check') {
    const data = env.data as RemoteCheck
    void showSnackbar({ message: `云端检查：${RECEIVER_CHECK_LABEL[data.outcome] ?? data.outcome}` })
    return
  }
  const data = env.data as FetchResult
  if (tofuAccepted.value) {
    tofuAccepted.value = false
    void showSnackbar({ message: `已拉取第 ${data.seq} 版，请确认首次部署` })
    deployOpen.value = true
    return
  }
  void showSnackbar({
    message: data.changed ? `已拉取第 ${data.seq} 版，等待部署` : `云端没有新版本（第 ${data.seq} 版）`,
  })
}

async function startTask(op: SyncOp): Promise<void> {
  startedOp.value = op
  try {
    await runner.start(syncTaskArgs(op))
  } catch (e) {
    void showSnackbar({ message: errorMessage(e) })
  }
}

async function startFetch(acceptFirst?: string): Promise<void> {
  startedOp.value = 'fetch'
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
    <MiuixTopAppBar title="RKP-Mini · 部署端" />

    <MiuixScrollArea v-if="!available" v-show="tab !== MORE_TAB" class="app__scroll">
      <main class="page">
        <MiuixCard>
          <div class="notice">
            <MiuixText type="title4" weight="medium" color="var(--m-color-error)">未检测到 KernelSU 接口</MiuixText>
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              请在 KernelSU 管理器的模块页中点击 rkp_mini_deployer 模块的“打开”进入此页面。当前环境无法执行命令。
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
          <MiuixSmallTitle text="已安装模块" />
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
            <MiuixButton v-if="needsEnable" type="primary" @click="riskOpen = true">启用</MiuixButton>
            <MiuixButton v-else type="primary" :disabled="!canDeploy" @click="startTask('run')">立即部署</MiuixButton>
            <MiuixButton :disabled="runner.busy.value" @click="settingsOpen = true">部署设置</MiuixButton>
          </div>
          <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
            跨设备使用接收到的 keybox 属于 keybox 克隆/冒充，不是合法的 RKP provisioning；同步只做结构与密钥匹配校验，未验证证书签名、根信任与吊销。
          </MiuixText>
        </main>
      </MiuixScrollArea>
    </template>

    <MiuixScrollArea v-show="tab === MORE_TAB" class="app__scroll">
      <main class="page">
        <MoreTab :active="tab === MORE_TAB" @logs="logsOpen = true" />
      </main>
    </MiuixScrollArea>

    <MiuixNavigationBar :model-value="tab" :items="TABS" @update:model-value="tab = $event">
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
      <SecretSheet v-model="secretOpen" :kind="secretKind" :title="secretTitle" :hint="secretHint" @saved="load" />
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
