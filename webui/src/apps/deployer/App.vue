<script setup lang="ts">
// Deployer page (rkp_mini_deployer module). One page + bottom sheets:
// KernelSU's WebView has no SPA history, so every flow stays on this page.
// All sync commands go through shared/lib/sync.ts, local deployment through
// lib/deploy.ts and the combined run through lib/run.ts; secrets only through
// SecretSheet.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  MiuixButton,
  MiuixCard,
  MiuixIcon,
  MiuixIconButton,
  MiuixScrollArea,
  MiuixSnackbarHost,
  MiuixText,
  MiuixTopAppBar,
  showSnackbar,
} from 'miuix-vue'
import { Refresh, Settings } from 'miuix-vue/icons'
import ReceiverStatusCard from './components/ReceiverStatusCard.vue'
import ImportSheet from './components/ImportSheet.vue'
import TofuSheet from './components/TofuSheet.vue'
import DeploySheet from './components/DeploySheet.vue'
import TargetsSheet from './components/TargetsSheet.vue'
import ReceiverSettingsSheet from './components/ReceiverSettingsSheet.vue'
import RunResultSheet from './components/RunResultSheet.vue'
import RiskSheet from '../../shared/components/RiskSheet.vue'
import SecretSheet from '../../shared/components/SecretSheet.vue'
import { bridgeAvailable } from '../../shared/lib/ksu'
import { nowUnix } from '../../shared/lib/format'
import { initTheme } from '../../shared/lib/theme'
import { errorMessage, isTofuFacts } from './lib/display'
import { readRunSteps, type RunSteps } from './lib/run'
import {
  readSyncConfig,
  readSyncStatus,
  syncErrorText,
  syncTaskArgs,
  useSyncRunner,
  type FetchResult,
  type ImportResult,
  type SecretKind,
  type SyncConfig,
  type SyncEnvelope,
  type SyncStatus,
  type SyncTaskResult,
  type TofuFacts,
} from '../../shared/lib/sync'

initTheme()

const available = bridgeAvailable()

const status = ref<SyncStatus | null>(null)
const config = ref<SyncConfig | null>(null)
const loading = ref(false)
const statusError = ref<string | null>(null)
const notInitialized = ref(false)

const now = ref(nowUnix())
let clock: number | undefined

const importOpen = ref(false)
const settingsOpen = ref(false)
const targetsOpen = ref(false)
const deployOpen = ref(false)
const runOpen = ref(false)
const riskOpen = ref(false)
const secretOpen = ref(false)
const secretKind = ref<SecretKind>('age')
const tofuOpen = ref(false)
const tofuFacts = ref<TofuFacts | null>(null)
const tofuMismatch = ref(false)
const runSteps = ref<RunSteps | null>(null)
/** A combined-run snapshot exists on disk (it survives closing the WebUI). */
const hasLastRun = ref(false)

const secretTitle = computed(() => (secretKind.value === 'age' ? 'age 口令' : 'WebDAV 应用密码'))
const secretHint = computed(() =>
  secretKind.value === 'age'
    ? '发送端通过另一个安全渠道告诉你的 age 口令；保存到 0600 文件后后台循环才能自动拉取。每个同步流应使用唯一口令。'
    : 'WebDAV 应用密码（不是账号登录密码），与链接中的用户名配套；只保存在本机 0600 文件中。',
)

async function load(): Promise<void> {
  loading.value = true
  const [s, c] = await Promise.allSettled([readSyncStatus(), readSyncConfig()])
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
  loading.value = false
  now.value = nowUnix()
}

// Long tasks run detached (webroot/runner/sync.sh) and are polled; every
// finished task — including one that ended while the page was closed — lands
// here exactly once.
const runner = useSyncRunner(async (result) => {
  await handleTask(result)
})

/** Combined run publishes no envelope: read the published per-step snapshot. */
async function openRunResult(): Promise<void> {
  try {
    runSteps.value = await readRunSteps()
    hasLastRun.value = true
    runOpen.value = true
  } catch (e) {
    // A single-step fetch or a malformed snapshot must not reuse older steps.
    runSteps.value = null
    hasLastRun.value = false
    void showSnackbar({ message: errorMessage(e) })
  }
}

/**
 * Offer the last combined result after reopening the page: the snapshot is on
 * disk, but the sheet is not forced open again for a result already seen.
 */
async function probeLastRun(): Promise<void> {
  try {
    await readRunSteps()
    hasLastRun.value = true
  } catch {
    hasLastRun.value = false
  }
}

/** TOFU facts travel in the fetch step's error data, as for a single fetch. */
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
  // never publishes one.
  if (result.op === 'run') {
    await openRunResult()
    if (runSteps.value) {
      showTofuFrom(runSteps.value.fetch.envelope as SyncEnvelope<unknown> | null)
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
  const data = env.data as FetchResult
  void showSnackbar({
    message: data.changed ? `已拉取 seq ${data.seq}，等待部署` : `远端没有新版本（seq ${data.seq}）`,
  })
}

const actionBlocked = computed(() => {
  if (loading.value) return null
  if (!status.value) return notInitialized.value ? '请先导入同步链接' : '需要先成功读取同步状态'
  if (!status.value.stream_id) return '请先导入同步链接'
  if (runner.busy.value) return null
  if (!status.value.enabled) return '同步未启用：请先完成风险确认并启用'
  if (!status.value.risk_ack_current) return '风险说明已更新，需要重新确认'
  return null
})

const canFetch = computed(() => actionBlocked.value === null && !runner.busy.value && !!status.value)
const canRun = computed(() => canFetch.value)
const canDeploy = computed(() => canFetch.value && !!status.value?.staged)
const noStaged = computed(() => canFetch.value && !status.value?.staged)

const deployedTargets = computed(() => status.value?.deploy_state?.targets ?? [])

async function startFetch(acceptFirst?: string): Promise<void> {
  try {
    await runner.start(syncTaskArgs('fetch', acceptFirst))
  } catch (e) {
    void showSnackbar({ message: errorMessage(e) })
  }
}

async function startRun(): Promise<void> {
  try {
    await runner.start(syncTaskArgs('run'))
  } catch (e) {
    void showSnackbar({ message: errorMessage(e) })
  }
}

function openRisk(): void {
  importOpen.value = false
  settingsOpen.value = false
  riskOpen.value = true
}

function openImport(): void {
  settingsOpen.value = false
  importOpen.value = true
}

function openSecret(kind: SecretKind): void {
  secretKind.value = kind
  importOpen.value = false
  settingsOpen.value = false
  secretOpen.value = true
}

function onImported(_result: ImportResult): void {
  void load()
}

function onTofuAccept(plaintextSha256: string): void {
  tofuOpen.value = false
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
  if (!status.value) await load()
  await probeLastRun()
})

onBeforeUnmount(() => clearInterval(clock))
</script>

<template>
  <div class="app">
    <MiuixTopAppBar title="RKP-Mini · 部署端">
      <template #actions>
        <MiuixIconButton aria-label="刷新状态" :disabled="loading" @click="load">
          <MiuixIcon :icon="Refresh" />
        </MiuixIconButton>
        <MiuixIconButton aria-label="设置" @click="settingsOpen = true">
          <MiuixIcon :icon="Settings" />
        </MiuixIconButton>
      </template>
    </MiuixTopAppBar>

    <MiuixScrollArea class="app__scroll">
      <main class="page">
        <MiuixCard v-if="!available">
          <div class="notice">
            <MiuixText type="title4" weight="medium" color="var(--m-color-error)">未检测到 KernelSU 接口</MiuixText>
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              请在 KernelSU 管理器的模块页中点击 rkp_mini_deployer 模块的“打开”进入此页面。当前环境无法执行命令。
            </MiuixText>
          </div>
        </MiuixCard>

        <template v-else>
          <ReceiverStatusCard
            :status="status"
            :config="config"
            :loading="loading"
            :error="statusError"
            :not-initialized="notInitialized"
            :busy="runner.busy.value"
            :now="now"
            @import="openImport"
            @risk="openRisk"
            @deploy="deployOpen = true"
            @retry="load"
            @secret="openSecret"
          />

          <MiuixCard v-if="status?.stream_id">
            <div class="actions-card">
              <MiuixText type="title4" weight="medium">同步操作</MiuixText>
              <div class="actions">
                <MiuixButton type="primary" :disabled="!canFetch" @click="startFetch()">
                  {{ runner.busy.value ? '任务进行中…' : '拉取' }}
                </MiuixButton>
                <MiuixButton :disabled="!canDeploy" @click="deployOpen = true">部署</MiuixButton>
                <MiuixButton :disabled="!canRun" @click="startRun">立即运行（拉取并部署）</MiuixButton>
                <MiuixButton v-if="hasLastRun" :disabled="runner.busy.value" @click="openRunResult()">
                  查看最近一次组合结果
                </MiuixButton>
                <MiuixButton :disabled="runner.busy.value || loading" @click="targetsOpen = true">部署目标</MiuixButton>
              </div>
              <MiuixText v-if="actionBlocked" type="footnote1" color="var(--m-color-error)">
                {{ actionBlocked }}
              </MiuixText>
              <MiuixText v-else-if="noStaged" type="footnote1" color="var(--app-warning)">
                部署需要先“拉取”并验证一份 keybox。
              </MiuixText>
              <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
                长任务在后台运行，离开页面不会中断；完成后自动刷新状态。首次拉取与首次部署都需要显式核对。
              </MiuixText>
            </div>
          </MiuixCard>

          <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
            跨设备使用接收到的 keybox 属于 keybox 克隆/冒充，不是合法的 RKP provisioning；同步只做结构与密钥匹配校验，未验证证书签名、根信任与吊销。
          </MiuixText>
        </template>
      </main>
    </MiuixScrollArea>

    <MiuixSnackbarHost />

    <template v-if="available">
      <ImportSheet
        v-model="importOpen"
        @imported="onImported"
        @risk="openRisk"
        @secret="openSecret"
      />
      <TofuSheet
        v-model="tofuOpen"
        :facts="tofuFacts"
        :mismatch="tofuMismatch"
        @accept="onTofuAccept"
        @retry="onTofuRetry"
      />
      <DeploySheet v-model="deployOpen" @changed="load" />
      <TargetsSheet v-model="targetsOpen" :deployed="deployedTargets" @changed="load" />
      <ReceiverSettingsSheet
        v-model="settingsOpen"
        :status="status"
        @refresh="load"
        @risk="openRisk"
        @import="openImport"
        @secret="openSecret"
      />
      <RunResultSheet v-model="runOpen" :steps="runSteps" />
      <RiskSheet v-model="riskOpen" @enabled="load" />
      <SecretSheet
        v-model="secretOpen"
        :kind="secretKind"
        :title="secretTitle"
        :hint="secretHint"
        @saved="load"
      />
    </template>
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
.actions-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px;
}
.actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
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
</style>
