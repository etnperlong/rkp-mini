<script setup lang="ts">
// Share sheet: renders the `rkp-mini-sync share` URI and its QR matrix locally.
// The URI carries location + stream metadata only (`contains_password: false`);
// the age passphrase must be sent over a separate, secure channel.
import { computed, ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixButton, MiuixIcon, MiuixIconButton, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import QrCode from './QrCode.vue'
import { copyText } from '../../../shared/lib/clipboard'
import { truncateMiddle } from '../../../shared/lib/format'
import { CommandError } from '../../../shared/lib/command'
import { share, type ShareResult } from '../../../shared/lib/sync'
import { providerLabel } from '../lib/rkp'
import { t } from '../../../shared/lib/i18n'

const open = defineModel<boolean>({ required: true })

const result = ref<ShareResult | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
/** `not_published`: `share` has nothing to point at until the first push. */
const needsPush = ref(false)

/** Error code from the JSON envelope stored in CommandError.detail. */
function errorCode(e: unknown): string | null {
  if (!(e instanceof CommandError) || !e.detail) return null
  try {
    const parsed = JSON.parse(e.detail) as { code?: unknown }
    return typeof parsed.code === 'string' ? parsed.code : null
  } catch {
    return null
  }
}

async function load(): Promise<void> {
  result.value = null
  error.value = null
  needsPush.value = false
  loading.value = true
  try {
    const shared = await share()
    result.value = shared
    if (shared.contains_password !== false) error.value = t('issuer.senderShareSheet.passwordField')
  } catch (e) {
    if (errorCode(e) === 'not_published') {
      needsPush.value = true
      error.value = t('issuer.senderShareSheet.notPublished')
    } else {
      error.value = e instanceof CommandError ? e.message : String(e)
    }
  } finally {
    loading.value = false
  }
}

watch(open, (v) => {
  if (v) void load()
})

/** Location fields from the decoded share JSON, for the receiver preview. */
const location = computed(() => {
  const data = result.value?.json
  if (!data || typeof data !== 'object') return null
  const provider = typeof data.provider === 'string' ? data.provider : null
  const at =
    typeof data.url === 'string'
      ? data.url
      : typeof data.endpoint === 'string' && typeof data.path === 'string'
        ? `${data.endpoint}${data.path}`
        : null
  const stream = typeof data.stream_id === 'string' ? data.stream_id : null
  const label = typeof data.label === 'string' ? data.label : null
  return provider || at || stream ? { provider, at, stream, label } : null
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.senderShareSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText v-if="loading" type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('issuer.senderShareSheet.reading') }}
      </MiuixText>

      <template v-else-if="result">
        <QrCode :rows="result.qr.rows" :label="t('issuer.senderShareSheet.qrLabel')" />
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)" align="center">
          {{ t('issuer.senderShareSheet.qrNote') }}
        </MiuixText>

        <div class="uri selectable">{{ result.uri }}</div>
        <MiuixButton @click="copyText(result.uri, t('issuer.senderShareSheet.copyUri'))">
          {{ t('issuer.senderShareSheet.copy') }}
        </MiuixButton>

        <div v-if="location" class="facts">
          <div v-if="location.provider" class="fact">
            <span class="fact__k">{{ t('issuer.senderShareSheet.provider') }}</span>
            <span class="fact__v">{{ providerLabel(location.provider) }}</span>
          </div>
          <div v-if="location.at" class="fact">
            <span class="fact__k">{{ t('issuer.senderShareSheet.location') }}</span>
            <span class="fact__v selectable">{{ location.at }}</span>
          </div>
          <div v-if="location.stream" class="fact">
            <span class="fact__k">{{ t('issuer.senderShareSheet.stream') }}</span>
            <span class="fact__v">{{ truncateMiddle(location.stream, 12) }}</span>
          </div>
          <div v-if="location.label" class="fact">
            <span class="fact__k">{{ t('issuer.senderShareSheet.label') }}</span>
            <span class="fact__v">{{ location.label }}</span>
          </div>
        </div>

        <div class="notice">
          <MiuixText type="body2" weight="medium" color="var(--app-warning)">{{ t('issuer.senderShareSheet.noPasswordTitle') }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            {{ t('issuer.senderShareSheet.noPasswordNote') }}
          </MiuixText>
        </div>
      </template>

      <template v-else-if="needsPush">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('issuer.senderShareSheet.needsPush') }}
        </MiuixText>
      </template>

      <MiuixText v-else-if="error" type="body2" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixButton v-if="error" :disabled="loading" @click="load">{{ t('issuer.senderShareSheet.retry') }}</MiuixButton>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.uri {
  padding: 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  line-height: 1.5;
}
.facts {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px;
  border-radius: 12px;
  background: var(--app-sheet-group);
}
.fact {
  display: flex;
  gap: 12px;
  font-size: 13px;
  line-height: 1.45;
}
.fact__k {
  flex: none;
  width: 76px;
  color: var(--m-color-on-surface-variant-summary);
}
.fact__v {
  flex: 1;
  min-width: 0;
  word-break: break-all;
}
.notice {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  background: color-mix(in srgb, var(--app-warning) 12%, transparent);
}
</style>
