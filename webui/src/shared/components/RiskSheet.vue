<script setup lang="ts">
// First-enable risk confirmation (design §13.2, §19.3). The statement comes
// from `rkp-mini-sync risk` so the text and its version have one source; every item
// is a separate, initially unchecked checkbox, and acceptance names the exact
// version shown. Enabling also starts the module's unattended loop.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { CommandError } from '../lib/command'
import { enableSync, readRisk, startLoop, type RiskStatement } from '../lib/sync'
import { currentLocale, t } from '../lib/i18n'

const open = defineModel<boolean>({ required: true })
const emit = defineEmits<{ enabled: [] }>()

const statement = ref<RiskStatement | null>(null)
const checked = ref<boolean[]>([])
const loading = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)

function message(e: unknown): string {
  return e instanceof CommandError ? e.message : String(e)
}

watch(open, async (v) => {
  if (!v) return
  statement.value = null
  checked.value = []
  error.value = null
  loading.value = true
  try {
    statement.value = await readRisk()
    checked.value = statement.value.items.map(() => false)
  } catch (e) {
    error.value = message(e)
  } finally {
    loading.value = false
  }
})

const allChecked = computed(() => checked.value.length > 0 && checked.value.every(Boolean))

async function accept(): Promise<void> {
  if (!statement.value || !allChecked.value) return
  busy.value = true
  error.value = null
  try {
    await enableSync(statement.value.version)
    await startLoop()
    open.value = false
    emit('enabled')
    void showSnackbar({ message: t('shared.risk.enabled') })
  } catch (e) {
    error.value = message(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('shared.risk.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('shared.risk.intro') }}
      </MiuixText>

      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>

      <div v-else-if="statement" class="sheet-group">
        <!-- The statement text comes only from `rkp-mini-sync risk` (zh + en), never from the
             locale files: Chinese UIs show zh with en below, every other UI shows en. -->
        <MiuixCheckboxPreference
          v-for="(item, i) in statement.items"
          :key="item.id"
          v-model="checked[i]"
          :title="currentLocale().startsWith('zh') ? item.zh : item.en"
          :summary="currentLocale().startsWith('zh') ? item.en : undefined"
        />
      </div>

      <MiuixText v-if="statement?.passphrase_path" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ t('shared.risk.passphraseNote', { path: statement.passphrase_path, version: statement.version }) }}
      </MiuixText>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixButton type="primary" :disabled="!allChecked || busy" @click="accept">
        {{ busy ? t('shared.risk.enabling') : t('shared.risk.accept') }}
      </MiuixButton>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.center {
  display: flex;
  justify-content: center;
  padding: 24px;
}
</style>
