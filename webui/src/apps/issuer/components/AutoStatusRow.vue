<script setup lang="ts">
// Two small cards: is unattended issuance / push actually active? Both run only
// inside sync-loop.sh (≈12 h: `issue --if-expiring-days N`, then `push --auto`),
// which runs only while sync is enabled with current risk acceptance. The issuer
// itself never schedules, so auto-issue without the loop is off.
import { computed } from 'vue'
import { MiuixCard, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import type { SyncStatus } from '../../../shared/lib/sync'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_WARNING } from '../../../shared/lib/icons'

const props = defineProps<{
  status: SyncStatus | null
  notInitialized: boolean
  loading: boolean
  error: string | null
}>()

interface Box {
  title: string
  on: boolean
  hint: string
}

/** Why the loop is not running, or null when it is. */
const loopOff = computed(() => {
  const s = props.status
  if (!s) return props.notInitialized ? '同步未初始化' : props.error ? '读取同步状态失败' : '读取中…'
  if (!s.enabled) return '同步未启用'
  if (!s.risk_ack_current) return '需重新确认风险'
  if (!s.loop_running) return '后台循环未运行'
  return null
})

const boxes = computed<Box[]>(() => {
  const days = props.status?.auto_issue_days ?? 0
  return [
    {
      title: '自动签发',
      on: loopOff.value === null && days > 0,
      hint: loopOff.value ?? (days > 0 ? `剩余 ≤ ${days} 天时` : '未设置提前天数'),
    },
    { title: '自动推送', on: loopOff.value === null, hint: loopOff.value ?? '约每 12 小时' },
  ]
})

const pending = computed(() => props.loading && !props.status)
</script>

<template>
  <div class="row">
    <MiuixCard v-for="b in boxes" :key="b.title" class="box">
      <div class="box__body">
        <div class="box__head">
          <MiuixText type="body1" weight="medium">{{ b.title }}</MiuixText>
          <MiuixProgressIndicator v-if="pending" type="infinite" :size="20" />
          <SvgIcon
            v-else
            class="box__icon"
            :path="b.on ? ICON_CHECK_CIRCLE : ICON_WARNING"
            :color="b.on ? 'var(--app-ok)' : 'var(--app-warning)'"
          />
        </div>
        <MiuixText type="body2" weight="medium">{{ pending ? '读取中' : b.on ? '已启用' : '未启用' }}</MiuixText>
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
