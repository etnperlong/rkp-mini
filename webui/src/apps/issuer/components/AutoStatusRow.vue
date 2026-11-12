<script setup lang="ts">
// Two small cards: is unattended issuance / push actually active? The issuer
// now owns auto-issue (`auto_issue` in its status: the days setting plus whether
// its auto-loop.sh is alive). Push still needs sync enabled with current risk
// acceptance, and `loop_running` there now means the same auto-loop.sh.
import { computed } from 'vue'
import { MiuixCard, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import type { SyncStatus } from '../../../shared/lib/sync'
import type { StatusData } from '../lib/rkp'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_WARNING } from '../../../shared/lib/icons'

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

interface Box {
  title: string
  on: boolean
  hint: string
  pending: boolean
}

/** Why the sync loop is not running, or null when it is. */
const loopOff = computed(() => {
  const s = props.sync
  if (!s) return props.syncNotInitialized ? '同步未初始化' : props.syncError ? '读取同步状态失败' : '读取中…'
  if (!s.enabled) return '同步未启用'
  if (!s.risk_ack_current) return '需重新确认风险'
  if (!s.loop_running) return '后台进程未运行'
  return null
})

const boxes = computed<Box[]>(() => {
  const a = props.status?.auto_issue
  let hint = props.statusError ? '未读取到状态' : '读取中…'
  if (a) hint = a.days <= 0 ? '未开启' : a.loop_running ? `剩余 ≤ ${a.days} 天时` : '后台进程未运行'
  return [
    {
      title: '自动签发',
      on: !!a && a.days > 0 && a.loop_running,
      hint,
      pending: props.statusLoading && !props.status,
    },
    {
      title: '自动推送',
      on: loopOff.value === null,
      hint: loopOff.value ?? '约每 12 小时',
      pending: props.syncLoading && !props.sync,
    },
  ]
})
</script>

<template>
  <div class="row">
    <MiuixCard v-for="b in boxes" :key="b.title" class="box">
      <div class="box__body">
        <div class="box__head">
          <MiuixText type="body1" weight="medium">{{ b.title }}</MiuixText>
          <MiuixProgressIndicator v-if="b.pending" type="infinite" :size="20" />
          <SvgIcon
            v-else
            class="box__icon"
            :path="b.on ? ICON_CHECK_CIRCLE : ICON_WARNING"
            :color="b.on ? 'var(--app-ok)' : 'var(--app-warning)'"
          />
        </div>
        <MiuixText type="body2" weight="medium">{{ b.pending ? '读取中' : b.on ? '已启用' : '未启用' }}</MiuixText>
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
