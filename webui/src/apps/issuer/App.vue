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
import IssueStatusCard from './components/IssueStatusCard.vue'
import IssueSheet from './components/IssueSheet.vue'
import IssueSettingsSheet from './components/IssueSettingsSheet.vue'
import ResultSheet from './components/ResultSheet.vue'
import DetailSheet from './components/DetailSheet.vue'
import MoreTab from './components/MoreTab.vue'
import LogsSheet from './components/LogsSheet.vue'
import SenderSyncSheet from './components/SenderSyncSheet.vue'
import SenderShareSheet from './components/SenderShareSheet.vue'
import SenderRotateDialog from './components/SenderRotateDialog.vue'
import SyncStatusCard from './components/SyncStatusCard.vue'
import SyncDetailSheet from './components/SyncDetailSheet.vue'
import RiskSheet from '../../shared/components/RiskSheet.vue'
import SecretDialog from '../../shared/components/SecretDialog.vue'
import { bridgeAvailable } from '../../shared/lib/ksu'
import { CommandError } from '../../shared/lib/command'
import { blockerLabel, readCachedStatus, readStatus, type StatusData } from './lib/rkp'
import { useIssueRunner } from './lib/issueRunner'
import {
  checkOutcomeLabel,
  readSyncStatus,
  syncErrorText,
  syncTaskArgs,
  useSyncRunner,
  type CheckOutcome,
  type SecretKind,
  type SyncOp,
  type SyncStatus,
} from '../../shared/lib/sync'
import { nowUnix } from '../../shared/lib/format'
import { initTheme } from '../../shared/lib/theme'
import { t, type MessageKey } from '../../shared/lib/i18n'

initTheme()

const available = bridgeAvailable()

const status = ref<StatusData | null>(null)
const statusError = ref<CommandError | null>(null)
const statusLoading = ref(false)
const statusCached = ref(false)

const issueOpen = ref(false)
const issueSettingsOpen = ref(false)
const resultOpen = ref(false)
const detailOpen = ref(false)

// Bottom navigation. The active tab is plain state, never history: the WebView's
// back key must keep closing the page (design §2.3).
const TABS = computed(() => [
  { label: t('issuer.app.tabStatus') },
  { label: t('issuer.app.tabIssue') },
  { label: t('issuer.app.tabSync') },
  { label: t('issuer.app.tabMore') },
])
const TAB_ICONS = [Info, Lock, UploadCloud, More]
const MORE_TAB = 3
const tab = ref(0)
const logsOpen = ref(false)
const logsFocusIssue = ref(false)

/** 更多 → 日志, or the result sheet (newest issue log expanded). */
function openLogs(focusIssue: boolean): void {
  resultOpen.value = false
  logsFocusIssue.value = focusIssue
  logsOpen.value = true
}

// Remote sync (rkp-mini-sync). The status read is local-only; the card starts a
// detached `push` task and no other sync command runs while it is busy.
const syncStatus = ref<SyncStatus | null>(null)
const syncNotInitialized = ref(false)
const syncError = ref<string | null>(null)
const syncLoading = ref(false)
const syncSheetOpen = ref(false)
const syncDetailOpen = ref(false)
const syncRiskOpen = ref(false)
const shareOpen = ref(false)
const rotateOpen = ref(false)
const secretOpen = ref(false)
const secretKind = ref<SecretKind>('age')

const SECRET_DIALOG: Record<SecretKind, { title: MessageKey; hint: MessageKey }> = {
  age: {
    title: 'issuer.app.secretDialog.age.title',
    hint: 'issuer.app.secretDialog.age.hint',
  },
  'gist-token': {
    title: 'issuer.app.secretDialog.gistToken.title',
    hint: 'issuer.app.secretDialog.gistToken.hint',
  },
  'webdav-password': {
    title: 'issuer.app.secretDialog.webdavPassword.title',
    hint: 'issuer.app.secretDialog.webdavPassword.hint',
  },
}

function openSecret(kind: SecretKind): void {
  secretKind.value = kind
  secretOpen.value = true
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
  void showSnackbar({ message: result.code === 0 ? t('issuer.app.issueSuccess') : t('issuer.app.issueFailed') })
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
  let message = t('issuer.app.syncFailed')
  const env = result.envelope
  if (result.code === 0 && env !== null && env.ok) {
    const data = env.data as Record<string, unknown>
    if (result.op === 'check') {
      message = t('issuer.app.syncCheck', {
        outcome: checkOutcomeLabel(data.outcome as CheckOutcome) ?? t('issuer.app.syncCheckDone'),
      })
    } else if (result.op === 'rotate') {
      message = t('issuer.app.syncRotated')
    } else if (result.op === 'delete') {
      message = data.deleted === true ? t('issuer.app.syncDeleted') : t('issuer.app.syncNothingDeleted')
    } else {
      message = t('issuer.app.syncPushed')
    }
  }
  void showSnackbar({ message })
})

/** Op started from this page (null after a reload mid-task): only for button labels. */
const startedOp = ref<SyncOp | null>(null)

/** A sync tab button starts one runner task; the runner rejects a second one. */
async function startSyncTask(op: SyncOp): Promise<void> {
  startedOp.value = op
  try {
    await syncRunner.start(syncTaskArgs(op))
  } catch (e) {
    void showSnackbar({ message: e instanceof Error ? e.message : String(e) })
  }
}

const canRunSync = computed(
  () => syncStatus.value !== null && !syncRunner.busy.value && syncRunner.phase.value !== 'stale',
)
const canPushSync = computed(() => {
  const s = syncStatus.value
  return canRunSync.value && s !== null && s.enabled && s.risk_ack_current
})
/** `启用` replaces 立即推送 until the stream is enabled with the current risk text. */
const syncNeedsEnable = computed(() => {
  const s = syncStatus.value
  return s !== null && (!s.enabled || !s.risk_ack_current)
})

async function onRefresh(): Promise<void> {
  // Auto-issue comes from the issuer status; the push box still needs sync status.
  const [ok] = await Promise.all([loadStatus(true), loadSyncStatus()])
  if (ok) void showSnackbar({ message: t('issuer.app.refreshed') })
}

async function onConfirmIssue(): Promise<void> {
  resultOpen.value = true
  await runner.start()
}

const blockers = computed(() => status.value?.blockers ?? [])

/** Reason the primary action is unavailable, or null when it can run. */
const issueBlockedReason = computed(() => {
  if (runner.busy.value) return null
  if (runner.phase.value === 'stale') return t('issuer.app.staleLockBlocked')
  if (!status.value || statusCached.value) return statusLoading.value ? null : t('issuer.app.needStatus')
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
    <MiuixTopAppBar :title="t('issuer.app.title')" />

    <MiuixScrollArea v-if="!available" v-show="tab !== MORE_TAB" class="app__scroll">
      <main class="page">
        <MiuixCard>
          <div class="notice">
            <MiuixText type="title4" weight="medium" color="var(--m-color-error)">{{ t('issuer.app.noBridgeTitle') }}</MiuixText>
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              {{ t('issuer.app.noBridgeBody') }}
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
            :runner="runner"
            :now="now"
            @refresh="onRefresh"
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
            @open="tab = $event"
          />
          <template v-if="status">
            <MiuixSmallTitle
              :text="
                blockers.length
                  ? t('issuer.app.envCheckFailed', blockers.length)
                  : t('issuer.app.envCheck')
              "
            />
            <CheckCard :status="status" />
          </template>
        </main>
      </MiuixScrollArea>

      <MiuixScrollArea v-show="tab === 1" class="app__scroll">
        <main class="page">
          <IssueStatusCard
            :status="status"
            :loading="statusLoading"
            :error="statusError?.message ?? null"
            :runner="runner"
            :now="now"
            @detail="detailOpen = true"
            @refresh="onRefresh"
            @result="resultOpen = true"
          />
          <div class="actions">
            <MiuixText v-if="issueBlockedReason" type="footnote1" color="var(--m-color-error)" align="center">
              {{ issueBlockedReason }}
            </MiuixText>
            <MiuixButton type="primary" :disabled="!canIssue" @click="issueOpen = true">{{ t('issuer.app.issueNow') }}</MiuixButton>
            <MiuixButton @click="issueSettingsOpen = true">{{ t('issuer.app.issueSettings') }}</MiuixButton>
          </div>
          <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)" align="center">
            {{ t('issuer.app.issueFootnote') }}
          </MiuixText>
        </main>
      </MiuixScrollArea>

      <MiuixScrollArea v-show="tab === 2" class="app__scroll">
        <main class="page">
          <SyncStatusCard
            :status="syncStatus"
            :not-initialized="syncNotInitialized"
            :loading="syncLoading"
            :error="syncError"
            :runner="syncRunner"
            :now="now"
            :busy-op="startedOp"
            :can-refresh="canRunSync"
            @detail="syncDetailOpen = true"
            @changed="loadSyncStatus()"
            @refresh="startSyncTask('check')"
          />
          <div v-if="syncStatus" class="actions">
            <MiuixButton v-if="syncNeedsEnable" type="primary" @click="syncRiskOpen = true">{{ t('issuer.app.enable') }}</MiuixButton>
            <MiuixButton v-else type="primary" :disabled="!canPushSync" @click="startSyncTask('push')">
              {{ t('issuer.app.pushNow') }}
            </MiuixButton>
            <MiuixButton :disabled="syncRunner.busy.value || !syncStatus?.enabled" @click="shareOpen = true">
              {{ t('issuer.app.share') }}
            </MiuixButton>
            <MiuixButton :disabled="syncRunner.busy.value" @click="syncSheetOpen = true">{{ t('issuer.app.syncSettings') }}</MiuixButton>
          </div>
        </main>
      </MiuixScrollArea>
    </template>

    <MiuixScrollArea v-show="tab === MORE_TAB" class="app__scroll">
      <main class="page">
        <MoreTab :active="tab === MORE_TAB" @logs="openLogs(false)" />
      </main>
    </MiuixScrollArea>

    <MiuixNavigationBar :model-value="tab" :items="TABS" @update:model-value="tab = $event">
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
      <ResultSheet v-model="resultOpen" :runner="runner" @logs="openLogs(true)" />
      <IssueSettingsSheet
        v-model="issueSettingsOpen"
        :status="status"
        :busy="runner.busy.value"
        @changed="loadStatus(false)"
      />
      <DetailSheet v-if="status" v-model="detailOpen" :status="status" :now="now" />
      <SyncDetailSheet v-model="syncDetailOpen" :status="syncStatus" :now="now" />
      <SenderSyncSheet
        v-model="syncSheetOpen"
        :status="syncStatus"
        :busy="syncRunner.busy.value"
        @secret="openSecret"
        @rotate="(startedOp = 'rotate'), (rotateOpen = true)"
        @delete="startSyncTask('delete')"
        @changed="loadSyncStatus()"
      />
      <SenderShareSheet v-model="shareOpen" />
      <SenderRotateDialog v-model="rotateOpen" :runner="syncRunner" />
      <RiskSheet v-model="syncRiskOpen" @enabled="loadSyncStatus()" />
      <SecretDialog
        v-model="secretOpen"
        :kind="secretKind"
        :title="t(SECRET_DIALOG[secretKind].title)"
        :hint="t(SECRET_DIALOG[secretKind].hint)"
        @saved="loadSyncStatus()"
      />
    </template>
    <LogsSheet v-model="logsOpen" :focus-issue="logsFocusIssue" />
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
