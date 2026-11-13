<script setup lang="ts">
// Shared log sheet: the module's background loop log (logs/<loop>.log, last
// 200 lines) and the page's in-memory debug lines. Apps add their own sections
// through the default slot (the issuer page lists its issue logs there).
// Nothing here reads a keybox or a secret.
import { ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import FoldBlock from './FoldBlock.vue'
import { SYNC_LOG, SYNC_LOG_LABEL, readSyncLog } from '../lib/sync'
import { CommandError } from '../lib/command'
import { debugLog } from '../lib/debug'
import { t } from '../lib/i18n'

const open = defineModel<boolean>({ required: true })
defineProps<{
  /** Footnote under the loop log; defaults to the receiver's wording (`shared.logs.loopHint`). */
  loopHint?: string
}>()

const loopText = ref('')
const loopLoading = ref(false)

function message(e: unknown): string {
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : String(e)
}

async function load(): Promise<void> {
  loopLoading.value = true
  try {
    loopText.value = await readSyncLog()
  } catch (e) {
    loopText.value = message(e)
  } finally {
    loopLoading.value = false
  }
}

watch(open, (v) => {
  if (v) void load()
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('shared.logs.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <slot />

      <MiuixSmallTitle :text="t('shared.logs.loop')" />
      <div class="sheet-group">
        <FoldBlock
          :title="SYNC_LOG_LABEL"
          :summary="loopLoading ? t('shared.common.loading') : SYNC_LOG"
          :text="loopLoading ? t('shared.common.reading') : loopText"
        />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ loopHint ?? t('shared.logs.loopHint') }}
      </MiuixText>

      <MiuixSmallTitle :text="t('shared.logs.debug')" />
      <div class="sheet-group">
        <FoldBlock
          :title="t('shared.logs.debugTitle')"
          :summary="t('shared.logs.debugCount', debugLog.length)"
          :text="debugLog.join('\n')"
        />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ t('shared.logs.debugNote') }}
      </MiuixText>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
</style>
