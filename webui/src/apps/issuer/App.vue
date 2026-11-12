<script setup lang="ts">
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
import { Info, Lock, More, UploadCloud } from 'miuix-vue/icons'
import StatusCard from './components/StatusCard.vue'
import AutoStatusRow from './components/AutoStatusRow.vue'
import CheckCard from './components/CheckCard.vue'
import IssueCard from './components/IssueCard.vue'
import KeyboxSummaryCard from './components/KeyboxSummaryCard.vue'
import IssueSheet from './components/IssueSheet.vue'
import AutoIssueSheet from './components/AutoIssueSheet.vue'
import ResultSheet from './components/ResultSheet.vue'
import DetailSheet from './components/DetailSheet.vue'
import MoreTab from './components/MoreTab.vue'
import SenderSyncCard from './components/SenderSyncCard.vue'
import SenderSyncSheet from './components/SenderSyncSheet.vue'
import SenderShareSheet from './components/SenderShareSheet.vue'
import SenderRotateSheet from './components/SenderRotateSheet.vue'
import SecretSheet from '../../shared/components/SecretSheet.vue'
import { bridgeAvailable } from '../../shared/lib/ksu'
import { CommandError } from '../../shared/lib/command'
import { blockerLabel, readCachedStatus, readStatus, type StatusData } from './lib/rkp'
import { useIssueRunner } from './lib/issueRunner'
import { readSyncStatus, syncErrorText, useSyncRunner, type SecretKind, type SyncStatus } from '../../shared/lib/sync'
import { nowUnix } from '../../shared/lib/format'
import { initTheme } from '../../shared/lib/theme'

initTheme()

const available = bridgeAvailable()

const status = ref<StatusData | null>(null)
const statusError = ref<CommandError | null>(null)
const statusLoading = ref(false)
const statusCached = ref(false)

const issueOpen = ref(false)
const autoIssueOpen = ref(false)
const resultOpen = ref(false)
const detailOpen = ref(false)

// Bottom navigation. The active tab is plain state, never history: the WebView's
// back key must keep closing the page (design §2.3).
const TABS = [{ label: '状态' }, { label: '签发' }, { label: '同步' }, { label: '更多' }]
const TAB_ICONS = [Info, Lock, UploadCloud, More]
const MORE_TAB = 3
const tab = ref(0)
const logsFocus = ref(false)

function selectTab(index: number): void {
  logsFocus.value = false
  tab.value = index
}

// Remote sync (rkp-mini-sync). The status read is local-only; the card starts a
// detached `push` task and no other sync command runs while it is busy.
const syncStatus = ref<SyncStatus | null>(null)
const syncNotInitialized = ref(false)
const syncError = ref<string | null>(null)
const syncLoading = ref(false)
const syncSheetOpen = ref(false)
const shareOpen = ref(false)
const rotateOpen = ref(false)
const secretOpen = ref(false)
const secretKind = ref<SecretKind>('age')

const SECRET_SHEET: Record<SecretKind, { title: string; hint: string }> = {
  age: {
    title: '设置 age 口令',
    hint: '每个同步流使用独立的强口令（建议 6 个以上随机单词或 20+ 随机字符）；接收端需要用它解密。',
  },
  'gist-token': {
    title: '设置 Gist token',
    hint: 'GitHub PAT，只勾选 gist 权限即可；用于创建和更新 Gist。',
  },
  'webdav-password': {
    title: '设置 WebDAV 密码',
    hint: '建议使用可单独撤销的应用密码（例如坚果云的“应用密码”）。',
  },
}

function openSecret(kind: SecretKind): void {
  secretKind.value = kind
  secretOpen.value = true
}

/** From the result sheet: the "more" tab with the newest log expanded. */
function openLogs(): void {
  resultOpen.value = false
  logsFocus.value = true
  tab.value = MORE_TAB
}

// Ticks once a minute so relative times and day counts stay current.
const now = ref(nowUnix())
let clock: number | undefined

function asCommandError(e: unknown): CommandError {
  return e instanceof CommandError ? e : new CommandError('exec', String(e))
}

async function loadStatus(updateDescription: boolean): Promise<boolean> {
  statusLoading.value = true
  try {
    status.value = await readStatus(updateDescription)
    statusCached.value = false
    statusError.value = null
    return true
  } catch (e) {
    statusError.value = asCommandError(e)
    return false
  } finally {
    statusLoading.value = false
    now.value = nowUnix()
  }
}

/** status.json snapshot; used while an issuance runs so `status` is not run concurrently. */
async function loadCachedStatus(): Promise<void> {
  statusLoading.value = true
  try {
    status.value = await readCachedStatus()
    statusCached.value = true
    statusError.value = null
  } catch (e) {
    statusError.value = asCommandError(e)
  } finally {
    statusLoading.value = false
  }
}

// Every finished task (even one that ended while the page was closed) gets one
// `status --update-description`: issue itself never refreshes status.json.
const runner = useIssueRunner(async (result) => {
  await loadStatus(true)
  void showSnackbar({ message: result.code === 0 ? '签发成功，状态已刷新' : '签发失败，原有 keybox 未被替换' })
})

/**
 * `rkp-mini-sync status`. A fresh setup has no `[sync]` table yet: the command fails
 * with `not_initialized`, which the card turns into the initialization choices
 * rather than an error.
 */
async function loadSyncStatus(): Promise<void> {
  if (!available) return
  syncLoading.value = true
  try {
    const { envelope } = await readSyncStatus()
    if (envelope.ok) {
      syncStatus.value = envelope.data
      syncNotInitialized.value = false
      syncError.value = null
    } else if (envelope.error.code === 'not_initialized') {
      syncStatus.value = null
      syncNotInitialized.value = true
      syncError.value = null
    } else {
      syncError.value = syncErrorText(envelope.error)
    }
  } catch (e) {
    syncError.value = asCommandError(e).message
  } finally {
    syncLoading.value = false
  }
}

// One sync task at a time: the runner's state files are the only source of
// truth, so a task started before the page opened still reports here.
const syncRunner = useSyncRunner(async (result) => {
  await loadSyncStatus()
  const ok = result.code === 0 && result.envelope?.ok === true
  void showSnackbar({
    message: ok
      ? result.op === 'rotate'
        ? '口令轮换完成'
        : '推送完成'
      : '同步任务失败，详情见同步卡片',
  })
})

async function onRefresh(): Promise<void> {
  // Auto-issue comes from the issuer status; the push box still needs sync status.
  const [ok] = await Promise.all([loadStatus(true), loadSyncStatus()])
  if (ok) void showSnackbar({ message: '状态已刷新，模块描述已同步' })
}

async function onConfirmIssue(): Promise<void> {
  resultOpen.value = true
  await runner.start()
}

const blockers = computed(() => status.value?.blockers ?? [])

/** Reason the primary action is unavailable, or null when it can run. */
const issueBlockedReason = computed(() => {
  if (runner.busy.value) return null
  if (runner.phase.value === 'stale') return '存在残留的任务锁，请先处理'
  if (!status.value || statusCached.value) return statusLoading.value ? null : '需要先成功读取状态'
  if (blockers.value.length) return blockerLabel(blockers.value[0])
  return null
})
const canIssue = computed(
  () => !runner.busy.value && !statusLoading.value && issueBlockedReason.value === null && status.value !== null,
)

onMounted(async () => {
  clock = window.setInterval(() => (now.value = nowUnix()), 60_000)
  if (!available) return
  await Promise.all([runner.check(), syncRunner.check(), loadSyncStatus()])
  if (runner.busy.value) await loadCachedStatus()
  else if (!status.value && !statusLoading.value) await loadStatus(false)
})

onBeforeUnmount(() => clearInterval(clock))
</script>

<template>
  <div class="app">
    <MiuixTopAppBar title="RKP-Mini · 签发端" />

    <MiuixScrollArea v-if="!available" v-show="tab !== MORE_TAB" class="app__scroll">
      <main class="page">
        <MiuixCard>
          <div class="notice">
            <MiuixText type="title4" weight="medium" color="var(--m-color-error)">未检测到 KernelSU 接口</MiuixText>
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              请在 KernelSU 管理器的模块页中点击本模块的“打开”进入此页面。当前环境无法执行命令。
            </MiuixText>
          </div>
        </MiuixCard>
      </main>
    </MiuixScrollArea>

    <!-- One scroll area per tab (v-show) so each keeps its own scroll position. -->
    <template v-else>
      <MiuixScrollArea v-show="tab === 0" class="app__scroll">
        <main class="page">
          <StatusCard
            :status="status"
            :loading="statusLoading"
            :error="statusError"
            :cached="statusCached"
            :now="now"
            @retry="loadStatus(false)"
            @detail="detailOpen = true"
          />
          <AutoStatusRow
            :status="status"
            :status-loading="statusLoading"
            :status-error="statusError?.message ?? null"
            :sync="syncStatus"
            :sync-not-initialized="syncNotInitialized"
            :sync-loading="syncLoading"
            :sync-error="syncError"
          />
          <template v-if="status">
            <MiuixSmallTitle :text="blockers.length ? `环境检查 · ${blockers.length} 项未通过` : '环境检查'" />
            <CheckCard :status="status" />
          </template>
          <div class="actions">
            <MiuixButton :disabled="statusLoading || runner.busy.value" @click="onRefresh">刷新状态</MiuixButton>
          </div>
        </main>
      </MiuixScrollArea>

      <MiuixScrollArea v-show="tab === 1" class="app__scroll">
        <main class="page">
          <KeyboxSummaryCard :status="status" :loading="statusLoading" :now="now" @detail="detailOpen = true" />
          <IssueCard :runner="runner" :now="now" @open="resultOpen = true" />
          <div class="actions">
            <MiuixText v-if="issueBlockedReason" type="footnote1" color="var(--m-color-error)" align="center">
              {{ issueBlockedReason }}
            </MiuixText>
            <MiuixButton type="primary" :disabled="!canIssue" @click="issueOpen = true">
              {{ runner.busy.value ? '签发进行中…' : '手动签发' }}
            </MiuixButton>
            <MiuixButton @click="autoIssueOpen = true">自动签发设置</MiuixButton>
          </div>
          <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
            签发结果取决于设备固件与 Google RKP 服务，不保证 Play Integrity 通过。
          </MiuixText>
        </main>
      </MiuixScrollArea>

      <MiuixScrollArea v-show="tab === 2" class="app__scroll">
        <main class="page">
          <SenderSyncCard
            :status="syncStatus"
            :not-initialized="syncNotInitialized"
            :loading="syncLoading"
            :error="syncError"
            :runner="syncRunner"
            :now="now"
            @settings="syncSheetOpen = true"
            @share="shareOpen = true"
            @changed="loadSyncStatus()"
          />
        </main>
      </MiuixScrollArea>
    </template>

    <MiuixScrollArea v-show="tab === MORE_TAB" class="app__scroll">
      <main class="page">
        <MoreTab :active="tab === MORE_TAB" :focus-logs="logsFocus" />
      </main>
    </MiuixScrollArea>

    <MiuixNavigationBar :model-value="tab" :items="TABS" @update:model-value="selectTab">
      <template #icon="{ index }">
        <MiuixIcon :icon="TAB_ICONS[index]" />
      </template>
    </MiuixNavigationBar>

    <MiuixSnackbarHost />

    <template v-if="available">
      <IssueSheet
        v-model="issueOpen"
        :keybox-path="status?.keybox.path ?? ''"
        :has-keybox="!!status?.keybox.exists"
        @confirm="onConfirmIssue"
      />
      <ResultSheet v-model="resultOpen" :runner="runner" @logs="openLogs" />
      <AutoIssueSheet v-model="autoIssueOpen" :status="status" @changed="loadStatus(false)" />
      <DetailSheet v-if="status" v-model="detailOpen" :status="status" :now="now" />
      <SenderSyncSheet
        v-model="syncSheetOpen"
        :status="syncStatus"
        :busy="syncRunner.busy.value"
        @secret="openSecret"
        @rotate="rotateOpen = true"
        @changed="loadSyncStatus()"
      />
      <SenderShareSheet v-model="shareOpen" />
      <SenderRotateSheet v-model="rotateOpen" :runner="syncRunner" />
      <SecretSheet
        v-model="secretOpen"
        :kind="secretKind"
        :title="SECRET_SHEET[secretKind].title"
        :hint="SECRET_SHEET[secretKind].hint"
        @saved="loadSyncStatus()"
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
.page :deep(.m-small-title) {
  padding: 4px 16px 0;
}
.notice {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
.footnote {
  padding: 4px 16px 0;
}
/* The snackbar host is teleported to <body> with bottom: 0; keep it above the navigation bar (64px items + 1px divider). */
:global(.m-snackbar-host) {
  bottom: 65px;
}
</style>
