<script setup lang="ts">
// 更多 tab: theme, the log sheet, the read-only receiver configuration and the
// module/binary facts. Reloads every time the tab becomes active.
import { ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixCard,
  MiuixIcon,
  MiuixRadioButtonPreference,
  MiuixSmallTitle,
  MiuixText,
  useTheme,
  type ThemeMode,
} from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import { CommandError } from '../../../shared/lib/command'
import { saveTheme } from '../../../shared/lib/theme'
import { DEPLOY_CONFIG, DEPLOY_DATA, readDeployConfig, readDeployVersion, readModuleProp } from '../lib/deploy'

const props = defineProps<{ active: boolean }>()
const emit = defineEmits<{ logs: [] }>()

const { mode } = useTheme()
const THEMES: { mode: ThemeMode; title: string }[] = [
  { mode: 'system', title: '跟随系统' },
  { mode: 'light', title: '浅色' },
  { mode: 'dark', title: '深色' },
]

const config = ref<string | null>(null)
const configError = ref<string | null>(null)
const binVersion = ref('…')
const moduleVersion = ref('…')

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

async function load(): Promise<void> {
  const [cfg, ver, prop] = await Promise.allSettled([readDeployConfig(), readDeployVersion(), readModuleProp()])

  if (cfg.status === 'fulfilled') {
    config.value = cfg.value
    configError.value = null
  } else {
    config.value = null
    configError.value = message(cfg.reason)
  }
  binVersion.value = ver.status === 'fulfilled' ? ver.value : `读取失败：${message(ver.reason)}`
  moduleVersion.value =
    prop.status === 'fulfilled' ? `${prop.value.version}（${prop.value.versionCode}）` : `读取失败：${message(prop.reason)}`
}

watch(
  () => props.active,
  (v) => {
    if (v) void load()
  },
  { immediate: true },
)
</script>

<template>
  <MiuixSmallTitle text="外观" />
  <MiuixCard>
    <MiuixRadioButtonPreference
      v-for="t in THEMES"
      :key="t.mode"
      :title="t.title"
      :model-value="mode === t.mode"
      @select="saveTheme(t.mode)"
    />
  </MiuixCard>

  <MiuixSmallTitle text="日志" />
  <MiuixCard>
    <MiuixBasicComponent title="查看日志" summary="后台日志、调试日志" clickable @click="emit('logs')">
      <template #end>
        <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
      </template>
    </MiuixBasicComponent>
  </MiuixCard>

  <MiuixSmallTitle text="配置（只读）" />
  <MiuixCard>
    <FoldBlock v-if="config !== null" title="config.toml" :summary="DEPLOY_CONFIG" :text="config" />
    <MiuixBasicComponent v-else :title="configError ? '未读取到配置' : '正在读取…'" :summary="configError ?? DEPLOY_CONFIG" />
  </MiuixCard>
  <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
    WebUI 不提供配置编辑。如需修改，请以 root 编辑上述文件后在“状态”页刷新。
  </MiuixText>

  <MiuixSmallTitle text="关于" />
  <MiuixCard>
    <MiuixBasicComponent title="模块版本" :summary="moduleVersion" />
    <MiuixBasicComponent title="二进制版本" :summary="binVersion" />
    <MiuixBasicComponent title="数据目录" :summary="DEPLOY_DATA" />
  </MiuixCard>
</template>

<style scoped>
.footnote {
  padding: 0 16px;
}
</style>
