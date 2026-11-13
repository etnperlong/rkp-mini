<script setup lang="ts">
// Two small cards: is unattended issuance / push actually active? The issuer
// now owns auto-issue (`auto_issue` in its status: the days setting plus whether
// its auto-loop.sh is alive). Push still needs sync enabled with current risk
// acceptance and `auto_push` on; `loop_running` there means the same auto-loop.sh.
import { computed } from 'vue'
import { MiuixCard, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import type { SyncStatus } from '../../../shared/lib/sync'
import type { StatusData } from '../lib/rkp'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_WARNING } from '../../../shared/lib/icons'
import { t, type MessageKey } from '../../../shared/lib/i18n'

const props = defineProps<{
  status: StatusData | null
  statusLoading: boolean
  /** Read failure message, or null when the last read succeeded. */
  statusError: string | null
  sync: SyncStatus | null
  syncNotInitialized: boolean
  syncLoading: boolean
  syncError: string | null
}>()
/** Tap a box to open the tab that owns its settings (issue = 1, sync = 2). */
const emit = defineEmits<{ open: [tab: number] }>()

interface Box {
  title: MessageKey
  on: boolean
  hint: string
  pending: boolean
  tab: number
}

/** Why the sync loop is not running, or null when it is. */
const loopOff = computed(() => {
  const s = props.sync
  if (!s)
    return props.syncNotInitialized
      ? t('issuer.autoStatusRow.syncNotInitialized')
      : props.syncError
        ? t('issuer.autoStatusRow.syncReadFailed')
        : t('issuer.autoStatusRow.reading')
  if (!s.enabled) return t('issuer.autoStatusRow.syncDisabled')
  if (!s.risk_ack_current) return t('issuer.autoStatusRow.risk')
  if (s.auto_push === false) return t('issuer.autoStatusRow.autoPushOff')
  if (!s.loop_running) return t('issuer.autoStatusRow.loopNotRunning')
  return null
})

const boxes = computed<Box[]>(() => {
  const a = props.status?.auto_issue
  let hint = props.statusError ? t('issuer.autoStatusRow.statusUnread') : t('issuer.autoStatusRow.reading')
  if (a)
    hint =
      a.days <= 0
        ? t('issuer.autoStatusRow.off')
        : a.loop_running
          ? t('issuer.autoStatusRow.withinDays', a.days)
          : t('issuer.autoStatusRow.loopNotRunning')
  return [
    {
      title: 'issuer.autoStatusRow.issueTitle',
      on: !!a && a.days > 0 && a.loop_running,
      hint,
      pending: props.statusLoading && !props.status,
      tab: 1,
    },
    {
      title: 'issuer.autoStatusRow.pushTitle',
      on: loopOff.value === null,
      hint: loopOff.value ?? t('issuer.autoStatusRow.aboutEvery12h'),
      pending: props.syncLoading && !props.sync,
      tab: 2,
    },
  ]
})
</script>

<template>
  <div class="row">
    <MiuixCard
      v-for="b in boxes"
      :key="b.title"
      class="box"
      press-feedback="sink"
      show-indication
      @click="emit('open', b.tab)"
    >
      <div class="box__body">
        <div class="box__head">
          <MiuixText type="body1" weight="medium">{{ t(b.title) }}</MiuixText>
          <MiuixProgressIndicator v-if="b.pending" type="infinite" :size="20" />
          <SvgIcon
            v-else
            class="box__icon"
            :path="b.on ? ICON_CHECK_CIRCLE : ICON_WARNING"
            :color="b.on ? 'var(--app-ok)' : 'var(--app-warning)'"
          />
        </div>
        <MiuixText type="body2" weight="medium">
          {{
            b.pending
              ? t('issuer.autoStatusRow.pending')
              : b.on
                ? t('issuer.autoStatusRow.on')
                : t('issuer.autoStatusRow.offState')
          }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ b.hint }}</MiuixText>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  gap: 12px;
}
.box {
  flex: 1;
  min-width: 0;
}
.box__body {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.box__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 4px;
}
.box__icon {
  width: 22px;
  height: 22px;
  flex: none;
}
</style>
