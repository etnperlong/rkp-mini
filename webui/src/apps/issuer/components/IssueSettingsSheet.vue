<script setup lang="ts">
// Issue settings. Auto-issue (issuer-owned `auto_issue_days`) is written by
// `rkp-mini-issuer config set`; the loop itself is `auto-loop.sh`, started here
// whenever auto-issue is on and its pid file says it is not running. The last
// run is read straight from the loop's `tmp/auto-issue.last.*` files (C6).
// The danger zone deletes the local keybox and its artifacts (`remove --yes`).
import { computed, ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixDialog,
  MiuixIcon,
  MiuixIconButton,
  MiuixInput,
  MiuixSmallTitle,
  MiuixSwitchPreference,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import { CommandError } from '../../../shared/lib/command'
import { formatAgo, formatDateTime } from '../../../shared/lib/format'
import {
  AUTO_ISSUE_DEFAULT_DAYS,
  AUTO_ISSUE_WARN_DAYS,
  readAutoIssueLast,
  removeKeybox,
  setAutoIssueDays,
  stageLabel,
  startAutoLoop,
  type AutoIssueLast,
  type StatusData,
} from '../lib/rkp'
import { t } from '../../../shared/lib/i18n'

const open = defineModel<boolean>({ required: true })
/** `busy`: an issuance is running, so nothing may be deleted. */
const props = defineProps<{ status: StatusData | null; busy: boolean }>()
const emit = defineEmits<{ changed: [] }>()

const confirmRemove = ref(false)
const removing = ref(false)

async function remove(): Promise<void> {
  if (removing.value) return
  removing.value = true
  error.value = null
  try {
    const r = await removeKeybox()
    confirmRemove.value = false
    open.value = false
    emit('changed')
    void showSnackbar({
      message: r.removed.length ? t('issuer.issueSettingsSheet.removed') : t('issuer.issueSettingsSheet.nothingRemoved'),
    })
  } catch (e) {
    confirmRemove.value = false
    error.value = message(e)
  } finally {
    removing.value = false
  }
}

const on = ref(false)
const days = ref(String(AUTO_ISSUE_DEFAULT_DAYS))
const saving = ref(false)
const error = ref<string | null>(null)
const last = ref<AutoIssueLast | null>(null)
const lastLoading = ref(false)
const lastError = ref<string | null>(null)

const loopText = computed(() => {
  const s = props.status
  if (!s) return t('issuer.issueSettingsSheet.reading')
  return s.auto_issue?.loop_running ? t('issuer.issueSettingsSheet.loopRunning') : t('issuer.issueSettingsSheet.loopStopped')
})

const lastConsequence = computed(() => {
  const l = last.value
  if (!l) return null
  const env = l.envelope
  if (!env) return { text: t('issuer.rkp.exitUnknown', { code: l.code }), bad: true }
  if (env.ok) {
    if ('skipped' in env.data)
      return { text: t('issuer.issueSettingsSheet.lastSkipped', env.data.days_left), bad: false }
    return { text: t('issuer.issueSettingsSheet.lastIssued', { id: env.data.device_id }), bad: false }
  }
  const e = env.error
  return {
    text:
      e.ta_status === undefined
        ? t('issuer.issueSettingsSheet.lastFailed', { stage: stageLabel(e.stage), code: e.code })
        : t('issuer.issueSettingsSheet.lastFailedTa', { stage: stageLabel(e.stage), code: e.code, ta: e.ta_status }),
    bad: true,
  }
})

const lastAt = computed(() => {
  const at = last.value?.at
  return at ? `${formatAgo(at)} · ${formatDateTime(at)}` : '—'
})

function message(e: unknown): string {
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : String(e)
}

/** Draft mirrors the last read status; 0 days keeps the default ready for switching on. */
function syncDraft(): void {
  const d = props.status?.auto_issue?.days ?? 0
  on.value = d > 0
  days.value = String(d > 0 ? d : AUTO_ISSUE_DEFAULT_DAYS)
}

const tooLong = computed(() => on.value && Number(days.value.trim()) > AUTO_ISSUE_WARN_DAYS)

async function loadLast(): Promise<void> {
  lastLoading.value = true
  lastError.value = null
  try {
    last.value = await readAutoIssueLast()
  } catch (e) {
    last.value = null
    lastError.value = message(e)
  } finally {
    lastLoading.value = false
  }
}

watch(open, (v) => {
  if (!v) return
  syncDraft()
  error.value = null
  void loadLast()
})

// Opened while the status was still loading: fill the draft once it arrives
// (saving is disabled until then, so an unknown value is never overwritten).
watch(
  () => props.status,
  (s, prev) => {
    if (open.value && s && !prev) syncDraft()
  },
)

async function save(): Promise<void> {
  if (saving.value) return
  error.value = null
  const n = on.value ? Number(days.value.trim()) : 0
  if (on.value && (!Number.isInteger(n) || n < 1 || n > 365)) {
    error.value = t('issuer.issueSettingsSheet.daysRange')
    return
  }
  saving.value = true
  try {
    await setAutoIssueDays(n)
    if (n > 0 && !props.status?.auto_issue?.loop_running) await startAutoLoop()
    emit('changed')
    void showSnackbar({ message: t('issuer.issueSettingsSheet.saved') })
  } catch (e) {
    error.value = message(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.issueSettingsSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixSmallTitle :text="t('issuer.issueSettingsSheet.autoIssue')" />
      <div class="sheet-group">
        <MiuixSwitchPreference
          v-model="on"
          :title="t('issuer.issueSettingsSheet.autoIssue')"
          :summary="
            on
              ? t('issuer.issueSettingsSheet.switchSummaryOn', { n: days }, Number(days) || 0)
              : t('issuer.issueSettingsSheet.off')
          "
        />
        <MiuixInput v-if="on" v-model="days" :label="t('issuer.issueSettingsSheet.daysLabel')" single-line />
      </div>
      <MiuixText v-if="tooLong" type="footnote1" color="var(--app-warning)">
        {{ t('issuer.issueSettingsSheet.tooLong', AUTO_ISSUE_WARN_DAYS) }}
      </MiuixText>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ t('issuer.issueSettingsSheet.loopNote') }}
      </MiuixText>

      <div class="sheet-group">
        <MiuixBasicComponent :title="t('issuer.issueSettingsSheet.loopTitle')" :summary="loopText" />
      </div>

      <MiuixSmallTitle :text="t('issuer.issueSettingsSheet.lastCheck')" />
      <div class="sheet-group">
        <MiuixBasicComponent v-if="lastLoading" :title="t('shared.common.loading')" />
        <MiuixBasicComponent
          v-else-if="lastError"
          :title="t('issuer.common.readFailed')"
          :summary="lastError"
          summary-color="var(--m-color-error)"
        />
        <template v-else-if="last">
          <MiuixBasicComponent :title="t('issuer.issueSettingsSheet.time')" :summary="lastAt" />
          <MiuixBasicComponent
            v-if="lastConsequence"
            :title="t('issuer.issueSettingsSheet.result')"
            :summary="lastConsequence.text"
            :summary-color="lastConsequence.bad ? 'var(--m-color-error)' : undefined"
          />
          <FoldBlock v-if="last.stderr.trim()" :title="t('issuer.issueSettingsSheet.stderr')" :text="last.stderr.trim()" />
        </template>
        <MiuixBasicComponent v-else :title="t('issuer.issueSettingsSheet.noRecord')" />
      </div>

      <MiuixButton type="primary" :disabled="saving || !status" @click="save">
        {{ saving ? t('shared.common.saving') : t('shared.common.save') }}
      </MiuixButton>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixSmallTitle :text="t('issuer.issueSettingsSheet.danger')" color="var(--m-color-error)" />
      <div class="sheet-group danger-group">
        <MiuixBasicComponent
          :title="t('issuer.issueSettingsSheet.removeTitle')"
          :summary="
            status?.keybox.exists
              ? t('issuer.issueSettingsSheet.removeSummary')
              : t('issuer.issueSettingsSheet.removeNothing')
          "
          title-color="var(--m-color-error)"
          clickable
          :disabled="busy || removing || !status?.keybox.exists"
          @click="confirmRemove = true"
        />
      </div>
    </div>

    <MiuixDialog
      v-model="confirmRemove"
      :title="t('issuer.issueSettingsSheet.removeDialogTitle')"
      :summary="t('issuer.issueSettingsSheet.removeDialogSummary')"
    >
      <div class="dialog-actions">
        <MiuixButton @click="confirmRemove = false">{{ t('shared.common.cancel') }}</MiuixButton>
        <MiuixButton class="danger-btn" type="primary" :disabled="removing || busy" @click="remove">
          {{ removing ? t('issuer.issueSettingsSheet.removing') : t('issuer.issueSettingsSheet.confirmRemove') }}
        </MiuixButton>
      </div>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.danger-group {
  background: color-mix(in srgb, var(--m-color-error) 8%, var(--app-sheet-group));
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 12px;
}
.danger-btn.m-button--primary:not(.m-button--disabled) {
  background: var(--m-color-error);
  color: #fff;
}
</style>
