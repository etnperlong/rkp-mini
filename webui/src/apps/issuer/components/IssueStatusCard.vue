<script setup lang="ts">
// Issue tab status, same pattern as SyncStatusCard: state-tinted card with an
// oversized corner icon, three short rows, a warning bar only when something is
// wrong, and the corner status as the refresh control. The running/last issue
// task lives inside the card; tapping the card opens the certificate details.
import { computed } from 'vue'
import { MiuixCard, MiuixIcon, MiuixIconButton, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { Close, Refresh } from 'miuix-vue/icons'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import type { IssueRunner } from '../lib/issueRunner'
import { KEYBOX_STATE_LABEL, KEYBOX_STATE_VISUAL, keyboxState, stageLabel, type StatusData } from '../lib/rkp'
import { daysLeft, formatAgo, formatDate, formatElapsed } from '../../../shared/lib/format'

const props = defineProps<{
  status: StatusData | null
  loading: boolean
  /** Status read failed and nothing usable is shown. */
  error: string | null
  runner: IssueRunner
  now: number
}>()
const emit = defineEmits<{ detail: []; refresh: []; result: [] }>()

const r = props.runner
const state = computed(() => (props.status ? keyboxState(props.status.keybox, props.now) : null))

const rows = computed(() => {
  const s = props.status
  if (!s || !state.value) return []
  const kb = s.keybox
  const na = kb.exists ? kb.not_after : null
  const left = na != null ? daysLeft(na, props.now) : null
  const a = s.auto_issue
  return [
    { k: '有效期至', v: na != null ? formatDate(na) : '—' },
    { k: '剩余天数', v: left == null ? '—' : left < 0 ? '已过期' : `${left} 天` },
    {
      k: '自动签发',
      v: a.days <= 0 ? '未开启' : a.loop_running ? `已开启 · 剩余 ≤ ${a.days} 天时` : '未开启 · 后台进程未运行',
    },
  ]
})

const warnings = computed<string[]>(() => {
  const s = props.status
  if (!s || !state.value) return []
  switch (state.value) {
    case 'expiring':
      return ['证书即将过期，建议立即签发']
    case 'expired':
      return ['证书已过期，请重新签发']
    case 'invalid':
      return [`keybox 无效${s.keybox.error ? `：${s.keybox.error}` : ''}`]
    default:
      return []
  }
})

/** Last finished task, stale lock or failed launch (busy shows in the corner). */
const task = computed<{ text: string; bad: boolean; dismissible: boolean } | null>(() => {
  if (r.busy.value) return null
  if (r.phase.value === 'stale') return { text: '任务锁残留，上次签发可能被中断，点按处理', bad: true, dismissible: false }
  if (r.phase.value === 'launch_failed') return { text: '签发未能启动，点按查看原因', bad: true, dismissible: false }
  const res = r.result.value
  if (!r.showLast.value || !res) return null
  const env = res.envelope
  const detail = env?.ok
    ? `成功 · Device ID ${env.data.device_id}`
    : env
      ? `失败 · ${stageLabel(env.error.stage)}：${env.error.code}`
      : `失败 · 进程异常退出（${res.code}）`
  return { text: `上次签发 ${formatAgo(res.finishedAt, props.now)} · ${detail}`, bad: res.code !== 0, dismissible: true }
})

const cornerText = computed(() => {
  if (r.busy.value) return `签发中 ${formatElapsed(r.elapsedMs.value)}${r.overdue.value ? ' · 结果未知' : ''}`
  return state.value ? KEYBOX_STATE_LABEL[state.value] : ''
})
</script>

<template>
  <MiuixCard
    class="issue-card"
    :class="state && `issue-card--${state}`"
    :press-feedback="status?.keybox.exists ? 'tilt' : 'none'"
    :show-indication="!!status?.keybox.exists"
    @click="status?.keybox.exists && emit('detail')"
  >
    <SvgIcon v-if="state" class="issue__mark" :path="KEYBOX_STATE_VISUAL[state].icon" :color="KEYBOX_STATE_VISUAL[state].color" />
    <div class="issue">
      <div class="issue__head">
        <MiuixText type="title4" weight="medium">当前证书</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <!-- Corner status doubles as the refresh control (re-reads the local status). -->
        <button
          v-else-if="status"
          type="button"
          class="issue__state"
          :disabled="r.busy.value || loading"
          :aria-label="r.busy.value ? cornerText : `${cornerText}，点击刷新状态`"
          @pointerdown.stop
          @click.stop="emit('refresh')"
        >
          <MiuixText type="body2" weight="medium">{{ cornerText }}</MiuixText>
          <MiuixProgressIndicator v-if="r.busy.value || loading" type="infinite" :size="16" />
          <MiuixIcon v-else :icon="Refresh" :size="18" />
        </button>
      </div>

      <MiuixText v-if="!status && error" type="body2" color="var(--m-color-error)">读取状态失败：{{ error }}</MiuixText>
      <MiuixText v-else-if="!status" type="body2" color="var(--m-color-on-surface-variant-summary)">正在读取状态…</MiuixText>

      <template v-else>
        <div class="rows">
          <div v-for="row in rows" :key="row.k" class="row">
            <span class="row__k">{{ row.k }}</span>
            <span class="row__v">{{ row.v }}</span>
          </div>
        </div>

        <div v-if="warnings.length" class="warnings">
          <MiuixText v-for="w in warnings" :key="w" type="footnote1" color="var(--app-warning)">{{ w }}</MiuixText>
        </div>
      </template>

      <div v-if="task" class="task" @pointerdown.stop @click.stop="emit('result')">
        <MiuixText type="body2" :color="task.bad ? 'var(--m-color-error)' : undefined">{{ task.text }}</MiuixText>
        <MiuixIconButton v-if="task.dismissible" aria-label="不再显示" @click.stop="r.dismiss()">
          <MiuixIcon :icon="Close" :size="16" color="var(--m-color-on-surface-variant-actions)" />
        </MiuixIconButton>
      </div>
    </div>
  </MiuixCard>
</template>

<style scoped>
/* MiuixCard paints its background from --m-card-color; the corner icon still
   shows the state where color-mix() is unavailable. */
.issue-card--valid {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .issue-card--expiring {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .issue-card--expired,
  .issue-card--invalid {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* `hidden` still lets focus/scrollIntoView pan the card sideways toward the
   oversized corner icon; `clip` never scrolls. */
.issue-card :deep(.m-card) {
  overflow: clip;
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.issue__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.issue {
  position: relative;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.issue__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.issue__state {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: -6px -8px -6px 0;
  padding: 6px 8px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}
.issue__state:active:not(:disabled) {
  background: var(--m-color-secondary-variant);
}
.issue__state:disabled {
  cursor: default;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 4px 0;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  font-size: 14px;
  line-height: 1.45;
}
.row__k {
  flex: none;
  width: 84px;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  flex: 1;
  min-width: 0;
  word-break: break-all;
}
.warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.task {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  cursor: pointer;
}
</style>
