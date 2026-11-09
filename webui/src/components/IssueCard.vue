<script setup lang="ts">
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixCard, MiuixIcon, MiuixIconButton, MiuixProgressIndicator } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import StatusPill from './StatusPill.vue'
import type { IssueRunner } from '../lib/runner'
import { stageLabel } from '../lib/rkp'
import { formatAgo, formatElapsed } from '../lib/format'

// Running task, stale lock, or the last finished result (until dismissed).
const props = defineProps<{ runner: IssueRunner; now: number }>()
const emit = defineEmits<{ open: [] }>()

const r = props.runner

const view = computed(() => {
  const phase = r.phase.value
  if (r.busy.value) {
    return {
      title: '签发进行中',
      summary: `已用时 ${formatElapsed(r.elapsedMs.value)}${r.overdue.value ? ' · 结果未知' : ''}`,
      pill: null,
      dismissible: false,
    }
  }
  if (phase === 'stale') {
    return { title: '任务锁残留', summary: '上次签发可能被中断，点按处理', pill: null, dismissible: false }
  }
  if (phase === 'launch_failed') {
    return { title: '签发未能启动', summary: '点按查看原因', pill: null, dismissible: false }
  }
  const res = r.result.value
  if (!r.showLast.value || !res) return null
  const env = res.envelope
  const detail = env?.ok
    ? `Device ID ${env.data.device_id}`
    : env
      ? `${stageLabel(env.error.stage)}：${env.error.code}`
      : `进程异常退出（${res.code}）`
  return {
    title: '上次签发',
    summary: `${formatAgo(res.finishedAt, props.now)} · ${detail}`,
    pill: res.code === 0 ? ({ tone: 'ok', label: '成功' } as const) : ({ tone: 'error', label: '失败' } as const),
    dismissible: true,
  }
})
</script>

<template>
  <MiuixCard v-if="view">
    <MiuixBasicComponent :title="view.title" :summary="view.summary" clickable @click="emit('open')">
      <template #start>
        <MiuixProgressIndicator v-if="r.busy.value" type="infinite" :size="22" color="var(--m-color-primary)" />
      </template>
      <template #end>
        <StatusPill v-if="view.pill" :tone="view.pill.tone" :label="view.pill.label" />
        <MiuixIconButton v-if="view.dismissible" aria-label="不再显示" @click.stop="r.dismiss()">
          <MiuixIcon :icon="Close" :size="18" color="var(--m-color-on-surface-variant-actions)" />
        </MiuixIconButton>
      </template>
    </MiuixBasicComponent>
  </MiuixCard>
</template>
