<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  MiuixButton,
  MiuixCard,
  MiuixIcon,
  MiuixIconButton,
  MiuixScrollArea,
  MiuixSmallTitle,
  MiuixSnackbarHost,
  MiuixText,
  MiuixTopAppBar,
  showSnackbar,
} from 'miuix-vue'
import { Settings } from 'miuix-vue/icons'
import StatusCard from './components/StatusCard.vue'
import CheckCard from './components/CheckCard.vue'
import IssueCard from './components/IssueCard.vue'
import IssueSheet from './components/IssueSheet.vue'
import ResultSheet from './components/ResultSheet.vue'
import DetailSheet from './components/DetailSheet.vue'
import SettingsSheet from './components/SettingsSheet.vue'
import { bridgeAvailable } from './lib/ksu'
import { CommandError, blockerLabel, readCachedStatus, readStatus, type StatusData } from './lib/rkp'
import { useIssueRunner } from './lib/runner'
import { nowUnix } from './lib/format'
import { initTheme } from './lib/theme'

initTheme()

const available = bridgeAvailable()

const status = ref<StatusData | null>(null)
const statusError = ref<CommandError | null>(null)
const statusLoading = ref(false)
const statusCached = ref(false)

const issueOpen = ref(false)
const resultOpen = ref(false)
const detailOpen = ref(false)
const settingsOpen = ref(false)
const settingsFocusLogs = ref(false)

function openSettings(focusLogs: boolean): void {
  settingsFocusLogs.value = focusLogs
  resultOpen.value = false
  settingsOpen.value = true
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

async function onRefresh(): Promise<void> {
  if (await loadStatus(true)) void showSnackbar({ message: '状态已刷新，模块描述已同步' })
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
  await runner.check()
  if (runner.busy.value) await loadCachedStatus()
  else if (!status.value && !statusLoading.value) await loadStatus(false)
})

onBeforeUnmount(() => clearInterval(clock))
</script>

<template>
  <div class="app">
    <MiuixTopAppBar title="RKP Mini">
      <template #actions>
        <MiuixIconButton aria-label="设置" @click="openSettings(false)">
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
              请在 KernelSU 管理器的模块页中点击本模块的“打开”进入此页面。当前环境无法执行命令。
            </MiuixText>
          </div>
        </MiuixCard>

        <template v-else>
          <StatusCard
            :status="status"
            :loading="statusLoading"
            :error="statusError"
            :cached="statusCached"
            :now="now"
            @retry="loadStatus(false)"
            @detail="detailOpen = true"
          />

          <IssueCard :runner="runner" :now="now" @open="resultOpen = true" />

          <template v-if="status">
            <MiuixSmallTitle :text="blockers.length ? `环境检查 · ${blockers.length} 项未通过` : '环境检查'" />
            <CheckCard :status="status" />
          </template>

          <div class="actions">
            <MiuixText v-if="issueBlockedReason" type="footnote1" color="var(--m-color-error)" align="center">
              {{ issueBlockedReason }}
            </MiuixText>
            <MiuixButton type="primary" :disabled="!canIssue" @click="issueOpen = true">
              {{ runner.busy.value ? '签发进行中…' : '签发新证书' }}
            </MiuixButton>
            <MiuixButton :disabled="statusLoading || runner.busy.value" @click="onRefresh">刷新状态</MiuixButton>
          </div>
        </template>

        <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
          签发结果取决于设备固件与 Google RKP 服务，不保证 Play Integrity 通过。
        </MiuixText>
      </main>
    </MiuixScrollArea>

    <MiuixSnackbarHost />

    <template v-if="available">
      <IssueSheet
        v-model="issueOpen"
        :keybox-path="status?.keybox.path ?? ''"
        :has-keybox="!!status?.keybox.exists"
        @confirm="onConfirmIssue"
      />
      <ResultSheet v-model="resultOpen" :runner="runner" @logs="openSettings(true)" />
      <DetailSheet v-if="status" v-model="detailOpen" :status="status" :now="now" />
    </template>
    <SettingsSheet v-model="settingsOpen" :focus-logs="settingsFocusLogs" />
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
</style>
