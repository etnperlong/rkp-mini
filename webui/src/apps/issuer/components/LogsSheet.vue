<script setup lang="ts">
// Issue logs (logs/issue-<unix>.log) on the shared log sheet, which owns the
// background loop log and the page's debug lines. Re-read on every open.
import { ref, watch } from 'vue'
import { MiuixBasicComponent, MiuixIcon, MiuixProgressIndicator, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import SharedLogsSheet from '../../../shared/components/LogsSheet.vue'
import { LOGS, listLogs, readLog } from '../lib/rkp'
import { CommandError } from '../../../shared/lib/command'
import { formatDateTime } from '../../../shared/lib/format'

const open = defineModel<boolean>({ required: true })
/** Expand the newest issue log once loaded (opened from the result sheet). */
const props = defineProps<{ focusIssue: boolean }>()

const logs = ref<string[]>([])
const logsLoading = ref(false)
const logsError = ref<string | null>(null)
const selectedLog = ref<string | null>(null)
const logText = ref('')
const logLoading = ref(false)

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

/** issue-<unix>.log → local start time. */
function logTitle(name: string): string {
  const unix = Number(/^issue-(\d+)\.log$/.exec(name)?.[1])
  return Number.isFinite(unix) ? formatDateTime(unix) : name
}

async function toggleLog(name: string): Promise<void> {
  if (selectedLog.value === name) {
    selectedLog.value = null
    return
  }
  selectedLog.value = name
  logText.value = ''
  logLoading.value = true
  try {
    logText.value = await readLog(name)
  } catch (e) {
    logText.value = message(e)
  } finally {
    logLoading.value = false
  }
}

async function load(): Promise<void> {
  selectedLog.value = null
  logsLoading.value = true
  logsError.value = null
  try {
    logs.value = await listLogs()
    if (props.focusIssue && logs.value.length) await toggleLog(logs.value[0])
  } catch (e) {
    logs.value = []
    logsError.value = message(e)
  } finally {
    logsLoading.value = false
  }
}

watch(open, (v) => {
  if (v) void load()
})
</script>

<template>
  <SharedLogsSheet
    v-model="open"
    loop-hint="自动签发与自动推送的记录（最后 200 行），只含 provider、seq、hash 与错误码，不含密码或 token。"
  >
    <MiuixSmallTitle text="签发日志" />
    <div class="sheet-group">
      <MiuixBasicComponent v-if="logsLoading" title="正在读取…">
        <template #end><MiuixProgressIndicator type="infinite" :size="20" /></template>
      </MiuixBasicComponent>
      <MiuixBasicComponent
        v-else-if="logsError"
        title="读取失败"
        :summary="logsError"
        summary-color="var(--m-color-error)"
      />
      <MiuixBasicComponent v-else-if="!logs.length" title="暂无日志" :summary="LOGS" />
      <template v-for="name in logs" v-else :key="name">
        <MiuixBasicComponent :title="logTitle(name)" :summary="name" clickable @click="toggleLog(name)">
          <template #end>
            <span class="chevron" :class="{ 'chevron--open': selectedLog === name }">
              <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
            </span>
          </template>
        </MiuixBasicComponent>
        <pre v-if="selectedLog === name" class="log">{{ logLoading ? '读取中…' : logText || '（空）' }}</pre>
      </template>
    </div>
    <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
      每次签发一份，仅显示最后 200 行；保留数量由 log_retention 控制。
    </MiuixText>
  </SharedLogsSheet>
</template>

<style scoped>
.chevron {
  display: inline-flex;
  transform: rotate(90deg);
  transition: transform 0.2s ease;
}
.chevron--open {
  transform: rotate(-90deg);
}
.log {
  margin: 0 16px 16px;
  padding: 12px;
  max-height: 50vh;
  overflow: auto;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  user-select: text;
}
</style>
