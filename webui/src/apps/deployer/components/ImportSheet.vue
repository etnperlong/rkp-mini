<script setup lang="ts">
// Receiver import (§19.5): paste the rkpsync:// link or its JSON, preview with
// `import --check` (writes nothing), then explicitly confirm with `--yes`.
// The link never contains the passphrase; no camera/scanner path exists.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import ReceiverField from './ReceiverField.vue'
import { t } from '../../../shared/lib/i18n'
import { errorMessage, providerLabel } from '../lib/display'
import {
  importApply,
  importCheck,
  type ImportPreview,
  type ImportResult,
  type LocationInfo,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const emit = defineEmits<{
  imported: [result: ImportResult]
  risk: []
  secret: [kind: 'age' | 'webdav-password']
}>()

const input = ref('')
const checkedInput = ref('')
const preview = ref<ImportPreview | null>(null)
const result = ref<ImportResult | null>(null)
const checking = ref(false)
const applying = ref(false)
const error = ref<string | null>(null)

watch(open, (v) => {
  if (!v) return
  input.value = ''
  checkedInput.value = ''
  preview.value = null
  result.value = null
  error.value = null
})

// Editing the pasted text invalidates a preview of the previous text; the
// confirmed import must always be what was reviewed.
watch(input, (value) => {
  if (preview.value && normalize(value) !== checkedInput.value) preview.value = null
})

/** Pretty-printed JSON is accepted: compact it before it enters a shell argv. */
function normalize(value: string): string {
  const trimmed = value.trim()
  if (!trimmed.startsWith('{')) return trimmed
  try {
    return JSON.stringify(JSON.parse(trimmed))
  } catch {
    return trimmed
  }
}

async function check(): Promise<void> {
  const value = normalize(input.value)
  if (!value) return
  checking.value = true
  error.value = null
  preview.value = null
  result.value = null
  try {
    preview.value = await importCheck(value)
    checkedInput.value = value
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    checking.value = false
  }
}

async function apply(): Promise<void> {
  if (!preview.value || preview.value.blocked) return
  applying.value = true
  error.value = null
  try {
    result.value = await importApply(checkedInput.value)
    emit('imported', result.value)
    void showSnackbar({ message: t('deployer.importSheet.saved') })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    applying.value = false
  }
}

function locationRows(loc: LocationInfo): { label: string; value: string }[] {
  switch (loc.provider) {
    case 'gist':
      return [
        { label: t('deployer.importSheet.urlLabel'), value: loc.url ?? t('deployer.importSheet.urlMissing') },
        { label: 'Gist ID', value: loc.gist_id ?? '—' },
        { label: t('deployer.importSheet.fileName'), value: loc.filename },
      ]
    case 'webdav':
      return [
        { label: 'Endpoint', value: loc.endpoint },
        { label: t('deployer.importSheet.path'), value: loc.path },
        { label: t('deployer.importSheet.user'), value: loc.user ?? t('deployer.importSheet.anonymous') },
      ]
    case 'url':
      return [{ label: 'URL', value: loc.url }]
  }
}

const rows = computed(() => (preview.value ? locationRows(preview.value.location) : []))
const webdavUser = computed(() => {
  const loc = preview.value?.location
  return loc?.provider === 'webdav' ? loc.user : null
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('deployer.importSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('deployer.importSheet.intro') }}
      </MiuixText>

      <div class="sheet-group">
        <ReceiverField
          v-model="input"
          :label="t('deployer.importSheet.inputLabel')"
          placeholder="rkpsync://v1/import?p=…"
          :multiline="true"
          :rows="4"
          :mono="true"
          :disabled="checking || applying"
        />
      </div>

      <MiuixButton type="primary" :disabled="checking || applying || !input.trim()" @click="check">
        {{ checking ? t('deployer.importSheet.checking') : t('deployer.importSheet.check') }}
      </MiuixButton>

      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <template v-if="preview">
        <div class="preview">
          <div class="preview__head">
            <MiuixText type="title4" weight="medium">{{ t('deployer.importSheet.previewTitle') }}</MiuixText>
            <MiuixText type="button" color="var(--m-color-primary)">{{ providerLabel(preview.location.provider) }}</MiuixText>
          </div>

          <div class="rows">
            <div class="row">
              <span class="row__k">{{ t('deployer.common.stream') }}</span>
              <span class="row__v selectable">{{ preview.stream_id }}</span>
            </div>
            <div v-if="preview.label" class="row">
              <span class="row__k">{{ t('deployer.importSheet.label') }}</span>
              <span class="row__v selectable">{{ preview.label }}</span>
            </div>
            <div v-for="r in rows" :key="r.label" class="row row--stack">
              <span class="row__k">{{ r.label }}</span>
              <span class="row__v selectable">{{ r.value }}</span>
            </div>
          </div>

          <div v-if="preview.blocked" class="banner banner--error">
            <MiuixText type="body2" color="var(--m-color-error)">
              {{ t('deployer.importSheet.blocked', { seq: preview.verified_seq ?? 0 }) }}
            </MiuixText>
          </div>
          <div v-else-if="preview.stream_change" class="banner banner--warn">
            <MiuixText type="body2" color="var(--app-warning)">
              {{
                t('deployer.importSheet.streamChange', {
                  from: preview.current_stream_id ?? t('deployer.importSheet.unconfigured'),
                  to: preview.stream_id,
                })
              }}
            </MiuixText>
          </div>
          <div v-else-if="preview.current_stream_id" class="banner banner--info">
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              {{ t('deployer.importSheet.sameStream') }}
            </MiuixText>
          </div>

          <MiuixButton
            type="primary"
            :disabled="applying || preview.blocked"
            @click="apply"
          >
            {{ applying ? t('deployer.importSheet.applying') : t('deployer.importSheet.apply') }}
          </MiuixButton>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
            {{ t('deployer.importSheet.applyNote') }}
          </MiuixText>
        </div>
      </template>

      <template v-if="result">
        <div class="divider"></div>
        <MiuixText type="title4" weight="medium">{{ t('deployer.importSheet.next') }}</MiuixText>
        <ul class="notes">
          <li v-for="note in result.notes" :key="note">{{ note }}</li>
        </ul>
        <div class="sheet-group">
          <MiuixButton v-if="!result.enabled" type="primary" @click="emit('risk')">{{ t('deployer.importSheet.risk') }}</MiuixButton>
          <MiuixButton :type="result.enabled ? 'primary' : 'default'" @click="emit('secret', 'age')">
            {{ t('deployer.importSheet.setPassword') }}
          </MiuixButton>
          <MiuixButton v-if="webdavUser" @click="emit('secret', 'webdav-password')">{{ t('deployer.importSheet.setWebdav') }}</MiuixButton>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.importSheet.passwordNote') }}
        </MiuixText>
        <MiuixButton @click="open = false">{{ t('deployer.importSheet.done') }}</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.preview__head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
}
.rows {
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  overflow: hidden;
}
.row {
  display: flex;
  gap: 16px;
  justify-content: space-between;
  padding: 10px 16px;
  font-size: var(--m-text-body2-size);
}
.row--stack {
  flex-direction: column;
  gap: 4px;
}
.row__k {
  flex: none;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  text-align: right;
  word-break: break-all;
}
.row--stack .row__v {
  text-align: start;
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
}
.banner {
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.banner--error {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
.banner--warn {
  background: color-mix(in srgb, var(--app-warning) 14%, transparent);
}
.banner--info {
  background: var(--m-color-secondary-variant);
}
.divider {
  height: 1px;
  background: var(--m-color-secondary-variant);
  margin: 4px 0;
}
.notes {
  margin: 0;
  padding-inline-start: 20px;
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
  line-height: 1.6;
  word-break: break-word;
}
.sheet-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
}
</style>
