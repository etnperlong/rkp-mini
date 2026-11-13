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
import LanguagePicker from '../../../shared/components/LanguagePicker.vue'
import { CommandError } from '../../../shared/lib/command'
import { t, type MessageKey } from '../../../shared/lib/i18n'
import { saveTheme } from '../../../shared/lib/theme'
import { DEPLOY_CONFIG, DEPLOY_DATA, readDeployConfig, readDeployVersion, readModuleProp } from '../lib/deploy'

const props = defineProps<{ active: boolean }>()
const emit = defineEmits<{ logs: [] }>()

const { mode } = useTheme()
const THEMES: { mode: ThemeMode; title: MessageKey }[] = [
  { mode: 'system', title: 'deployer.more.themeSystem' },
  { mode: 'light', title: 'deployer.more.themeLight' },
  { mode: 'dark', title: 'deployer.more.themeDark' },
]

const config = ref<string | null>(null)
const configError = ref<string | null>(null)
const binVersion = ref('…')
const moduleVersion = ref('…')

function message(e: unknown): string {
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : String(e)
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
  binVersion.value = ver.status === 'fulfilled' ? ver.value : t('deployer.more.readFailed', { error: message(ver.reason) })
  moduleVersion.value =
    prop.status === 'fulfilled'
      ? t('deployer.more.moduleVersionValue', { version: prop.value.version, code: prop.value.versionCode })
      : t('deployer.more.readFailed', { error: message(prop.reason) })
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
  <MiuixSmallTitle :text="t('deployer.more.appearance')" />
  <MiuixCard>
    <MiuixRadioButtonPreference
      v-for="th in THEMES"
      :key="th.mode"
      :title="t(th.title)"
      :model-value="mode === th.mode"
      @select="saveTheme(th.mode)"
    />
    <LanguagePicker />
  </MiuixCard>

  <MiuixSmallTitle :text="t('deployer.more.logs')" />
  <MiuixCard>
    <MiuixBasicComponent
      :title="t('deployer.more.viewLogs')"
      :summary="t('deployer.more.viewLogsSummary')"
      clickable
      @click="emit('logs')"
    >
      <template #end>
        <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
      </template>
    </MiuixBasicComponent>
  </MiuixCard>

  <MiuixSmallTitle :text="t('deployer.more.configReadOnly')" />
  <MiuixCard>
    <FoldBlock v-if="config !== null" title="config.toml" :summary="DEPLOY_CONFIG" :text="config" />
    <MiuixBasicComponent
      v-else
      :title="configError ? t('deployer.more.configUnreadable') : t('shared.common.loading')"
      :summary="configError ?? DEPLOY_CONFIG"
    />
  </MiuixCard>
  <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
    {{ t('deployer.more.configNote') }}
  </MiuixText>

  <MiuixSmallTitle :text="t('deployer.more.about')" />
  <MiuixCard>
    <MiuixBasicComponent :title="t('deployer.more.moduleVersion')" :summary="moduleVersion" />
    <MiuixBasicComponent :title="t('deployer.more.binaryVersion')" :summary="binVersion" />
    <MiuixBasicComponent :title="t('deployer.more.dataDir')" :summary="DEPLOY_DATA" />
  </MiuixCard>
</template>

<style scoped>
.footnote {
  padding: 0 16px;
}
</style>
