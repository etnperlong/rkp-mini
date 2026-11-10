<script setup lang="ts">
import { computed } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
} from 'miuix-vue'
import { Close, Copy } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import StatusPill from '../../../shared/components/StatusPill.vue'
import type { IssueRunner } from '../lib/issueRunner'
import { exitLabel, failureHint, stageLabel } from '../lib/rkp'
import { formatDateTime, formatElapsed } from '../../../shared/lib/format'
import { copyText } from '../../../shared/lib/clipboard'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ runner: IssueRunner }>()
const emit = defineEmits<{ logs: [] }>()

const r = props.runner
const phase = r.phase
const result = r.result

const success = computed(() => (result.value?.envelope?.ok ? result.value.envelope.data : null))
const failure = computed(() => (result.value?.envelope && !result.value.envelope.ok ? result.value.envelope.error : null))
const hint = computed(() => (failure.value ? failureHint(failure.value) : null))

const title = computed(() => {
  switch (phase.value) {
    case 'starting':
    case 'running':
      return '正在签发'
    case 'stale':
      return '任务状态异常'
    case 'launch_failed':
      return '启动失败'
    case 'finished':
      return result.value?.code === 0 ? '签发成功' : '签发失败'
    default:
      return '签发'
  }
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="title">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <!-- In progress -->
      <template v-if="r.busy.value">
        <div class="progress">
          <MiuixProgressIndicator type="infinite" :size="36" color="var(--m-color-primary)" />
          <MiuixText type="title2" class="elapsed">{{ formatElapsed(r.elapsedMs.value) }}</MiuixText>
          <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)" align="center">
            生成密钥 → 获取挑战 → TA 签名 → 证书服务 → 写入
          </MiuixText>
        </div>
        <MiuixText v-if="r.overdue.value" type="body2" color="var(--app-warning)">
          已超过 2 分钟仍未结束，结果未知。任务仍可能在网络或 TA 超时后结束，请稍后重新打开或查看日志。
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          签发在后台独立进行，无法中途取消。可以关闭此页面，重新打开后可查看结果。
        </MiuixText>
        <MiuixText v-if="r.probeError.value" type="footnote1" color="var(--m-color-error)">
          读取任务状态失败：{{ r.probeError.value }}
        </MiuixText>
      </template>

      <!-- Stale lock -->
      <template v-else-if="phase === 'stale'">
        <MiuixText type="body2">
          检测到超过 10 分钟的任务锁且没有结果，上次签发进程可能被系统终止。确认没有签发在进行后可以清除锁。
        </MiuixText>
        <MiuixText v-if="r.startedAt.value" type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          锁创建于 {{ formatDateTime(r.startedAt.value) }}
        </MiuixText>
        <MiuixButton type="primary" @click="r.clearStaleLock()">清除锁</MiuixButton>
        <MiuixButton @click="emit('logs')">查看日志</MiuixButton>
      </template>

      <!-- Launch failed -->
      <template v-else-if="phase === 'launch_failed'">
        <MiuixText type="body2">
          后台任务没有启动（未创建任务锁）。可能是 shell 缺少 setsid/nohup，或模块 webroot 不完整。可在设置的调试日志中查看详情。
        </MiuixText>
        <MiuixButton @click="r.check()">重新检查</MiuixButton>
      </template>

      <!-- Finished -->
      <template v-else-if="phase === 'finished' && result">
        <div class="head">
          <StatusPill :tone="result.code === 0 ? 'ok' : 'error'" :label="exitLabel(result.code)" />
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            结束于 {{ formatDateTime(result.finishedAt) }} · 退出码 {{ result.code }}
          </MiuixText>
        </div>

        <div v-if="success" class="sheet-group">
          <MiuixBasicComponent title="叶子证书" :summary="success.leaf_subject || '（无 CN）'" />
          <MiuixBasicComponent title="有效期至" :summary="formatDateTime(success.leaf_not_after)" />
          <MiuixBasicComponent title="证书链" :summary="`${success.chain_certificates} 张`" />
          <MiuixBasicComponent title="Device ID" :summary="success.device_id" clickable @click="copyText(success.device_id, ' Device ID')">
            <template #end><MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" /></template>
          </MiuixBasicComponent>
          <MiuixBasicComponent title="TA" :summary="`status ${success.ta_status} · 帧 ${success.frame_layout} · CSR ${success.csr_len} 字节`" />
          <MiuixBasicComponent title="keybox" :summary="success.keybox" />
        </div>

        <template v-else-if="failure">
          <div class="sheet-group">
            <MiuixBasicComponent title="阶段" :summary="`${stageLabel(failure.stage)}（${failure.stage}）`" />
            <MiuixBasicComponent title="错误码" :summary="failure.code" />
            <MiuixBasicComponent title="信息" :summary="failure.message" />
            <MiuixBasicComponent v-if="failure.ta_status !== undefined" title="ta_status" :summary="String(failure.ta_status)" />
          </div>
          <MiuixText v-if="hint" type="body2" color="var(--m-color-on-surface-variant-summary)">{{ hint }}</MiuixText>
        </template>

        <MiuixText v-else type="body2" color="var(--m-color-on-surface-variant-summary)">
          进程没有输出结果（可能被系统终止）。请查看下方错误输出与日志。
        </MiuixText>

        <MiuixText v-if="result.code !== 0" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          签发失败时不会替换原有 keybox。
        </MiuixText>

        <div class="sheet-group">
          <FoldBlock v-if="failure?.head_hex" title="head_hex" :text="failure.head_hex" />
          <FoldBlock title="错误输出（stderr）" :text="result.stderr.trim()" />
          <FoldBlock title="原始输出（stdout）" :text="result.stdout.trim()" />
        </div>
        <MiuixButton @click="emit('logs')">查看签发日志</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.progress {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 16px 0 8px;
}
.elapsed {
  font-variant-numeric: tabular-nums;
}
.head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
</style>
