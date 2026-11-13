<script setup lang="ts">
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
import { CONFIG, DATA, readConfig, readModuleProp, readVersion } from '../lib/rkp'
import { CommandError } from '../../../shared/lib/command'
import { saveTheme } from '../../../shared/lib/theme'
import { t, type MessageKey } from '../../../shared/lib/i18n'

/** Reloads every time the tab becomes active. */
const props = defineProps<{ active: boolean }>()
const emit = defineEmits<{ logs: [] }>()

const { mode } = useTheme()
const THEMES: { mode: ThemeMode; title: MessageKey }[] = [
  { mode: 'system', title: 'issuer.more.themeSystem' },
  { mode: 'light', title: 'issuer.more.themeLight' },
  { mode: 'dark', title: 'issuer.more.themeDark' },
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
  const [cfg, ver, prop] = await Promise.allSettled([readConfig(), readVersion(), readModuleProp()])

  if (cfg.status === 'fulfilled') {
    config.value = cfg.value
    configError.value = null
  } else {
    config.value = null
    configError.value = message(cfg.reason)
  }
  binVersion.value = ver.status === 'fulfilled' ? ver.value : t('issuer.more.readFailed', { error: message(ver.reason) })
  moduleVersion.value =
    prop.status === 'fulfilled'
      ? t('issuer.more.moduleVersionValue', { version: prop.value.version, code: prop.value.versionCode })
      : t('issuer.more.readFailed', { error: message(prop.reason) })
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
  <MiuixSmallTitle :text="t('issuer.more.appearance')" />
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

  <MiuixSmallTitle :text="t('issuer.more.logs')" />
  <MiuixCard>
    <MiuixBasicComponent
      :title="t('issuer.more.viewLogs')"
      :summary="t('issuer.more.viewLogsSummary')"
      clickable
      @click="emit('logs')"
    >
      <template #end>
        <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
      </template>
    </MiuixBasicComponent>
  </MiuixCard>

  <MiuixSmallTitle :text="t('issuer.more.configReadOnly')" />
  <MiuixCard>
    <FoldBlock v-if="config !== null" title="config.toml" :summary="CONFIG" :text="config" />
    <MiuixBasicComponent
      v-else
      :title="configError ? t('issuer.more.configUnreadable') : t('shared.common.loading')"
      :summary="configError ?? CONFIG"
    />
  </MiuixCard>
  <MiuixText class="footnote" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
    {{ t('issuer.more.configNote') }}
  </MiuixText>

  <MiuixSmallTitle :text="t('issuer.more.about')" />
  <MiuixCard>
    <MiuixBasicComponent :title="t('issuer.more.moduleVersion')" :summary="moduleVersion" />
    <MiuixBasicComponent :title="t('issuer.more.binaryVersion')" :summary="binVersion" />
    <MiuixBasicComponent :title="t('issuer.more.dataDir')" :summary="DATA" />
  </MiuixCard>
</template>

<style scoped>
.footnote {
  padding: 0 16px;
}
</style>
