<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixRadioButtonPreference,
  MiuixSmallTitle,
  MiuixText,
  useTheme,
  type ThemeMode,
} from 'miuix-vue'
import { ChevronForward, Close } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import {
  BIN,
  CONFIG,
  DATA,
  LOGS,
  MODULE_DIR,
  listLogs,
  readConfig,
  readLog,
  readModuleProp,
  readVersion,
} from '../lib/rkp'
import { CommandError } from '../../../shared/lib/command'
import { saveTheme } from '../../../shared/lib/theme'
import { debugLog } from '../../../shared/lib/debug'
import { formatDateTime } from '../../../shared/lib/format'

const open = defineModel<boolean>({ required: true })
/** Open with the newest log expanded (from the result sheet). */
const props = defineProps<{ focusLogs: boolean }>()

const { mode } = useTheme()
const THEMES: { mode: ThemeMode; title: string }[] = [
  { mode: 'system', title: '跟随系统' },
  { mode: 'light', title: '浅色' },
  { mode: 'dark', title: '深色' },
]

const config = ref<string | null>(null)
const configError = ref<string | null>(null)
const logs = ref<string[]>([])
const logsLoading = ref(false)
const logsError = ref<string | null>(null)
const selectedLog = ref<string | null>(null)
const logText = ref('')
const logLoading = ref(false)
const binVersion = ref('…')
const moduleVersion = ref('…')

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
  const [cfg, list, ver, prop] = await Promise.allSettled([readConfig(), listLogs(), readVersion(), readModuleProp()])

  if (cfg.status === 'fulfilled') {
    config.value = cfg.value
    configError.value = null
  } else {
    config.value = null
    configError.value = message(cfg.reason)
  }
  if (list.status === 'fulfilled') logs.value = list.value
  else logsError.value = message(list.reason)
  logsLoading.value = false
  binVersion.value = ver.status === 'fulfilled' ? ver.value : `读取失败：${message(ver.reason)}`
  moduleVersion.value =
    prop.status === 'fulfilled' ? `${prop.value.version}（${prop.value.versionCode}）` : `读取失败：${message(prop.reason)}`

  if (props.focusLogs && logs.value.length) await toggleLog(logs.value[0])
}

watch(open, (v) => {
  if (v) void load()
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="设置">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixSmallTitle text="外观" />
      <div class="sheet-group">
        <MiuixRadioButtonPreference
          v-for="t in THEMES"
          :key="t.mode"
          :title="t.title"
          :model-value="mode === t.mode"
          @select="saveTheme(t.mode)"
        />
      </div>

      <MiuixSmallTitle text="签发日志" />
      <div class="sheet-group">
        <MiuixBasicComponent v-if="logsLoading" title="正在读取…">
          <template #end><MiuixProgressIndicator type="infinite" :size="20" /></template>
        </MiuixBasicComponent>
        <MiuixBasicComponent v-else-if="logsError" title="读取失败" :summary="logsError" summary-color="var(--m-color-error)" />
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
        仅显示每份日志的最后 200 行；数量由 log_retention 控制。
      </MiuixText>

      <MiuixSmallTitle text="配置（只读）" />
      <div class="sheet-group">
        <FoldBlock v-if="config !== null" title="config.toml" :summary="CONFIG" :text="config" />
        <MiuixBasicComponent v-else :title="configError ? '未读取到配置' : '正在读取…'" :summary="configError ?? CONFIG" />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        WebUI 不提供配置编辑。如需修改，请以 root 编辑上述文件后点“刷新状态”。
      </MiuixText>

      <MiuixSmallTitle text="关于" />
      <div class="sheet-group">
        <MiuixBasicComponent title="模块版本" :summary="moduleVersion" />
        <MiuixBasicComponent title="二进制" :summary="binVersion" />
        <MiuixBasicComponent title="二进制路径" :summary="BIN" />
        <MiuixBasicComponent title="数据目录" :summary="DATA" />
        <MiuixBasicComponent title="模块目录" :summary="MODULE_DIR" />
        <FoldBlock title="调试日志" :summary="`${debugLog.length} 条`" :text="debugLog.join('\n')" />
      </div>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
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
