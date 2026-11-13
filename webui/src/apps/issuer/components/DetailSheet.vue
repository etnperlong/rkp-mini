<script setup lang="ts">
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close, Copy } from 'miuix-vue/icons'
import StatusPill from '../../../shared/components/StatusPill.vue'
import { KEYBOX_STATE_LABEL, keyboxState, type StatusData } from '../lib/rkp'
import { daysLeft, formatDateTime } from '../../../shared/lib/format'
import { copyText } from '../../../shared/lib/clipboard'
import { t } from '../../../shared/lib/i18n'

// Only fields already present in status.json; never keybox/key contents.
const open = defineModel<boolean>({ required: true })
const props = defineProps<{ status: StatusData; now: number }>()

const kb = computed(() => props.status.keybox)
const state = computed(() => keyboxState(kb.value, props.now))
const tone = computed(() =>
  state.value === 'valid' ? 'ok' : state.value === 'expiring' ? 'warning' : state.value === 'missing' ? 'neutral' : 'error',
)
const remaining = computed(() => {
  if (kb.value.not_after == null) return '—'
  const d = daysLeft(kb.value.not_after, props.now)
  return d < 0 ? t('issuer.detailSheet.expiredAgo', -d) : t('issuer.detailSheet.days', d)
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.detailSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div class="head">
        <StatusPill :tone="tone" :label="t(KEYBOX_STATE_LABEL[state])" />
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          {{ t('issuer.detailSheet.checkedAt', { time: formatDateTime(status.generated_at) }) }}
        </MiuixText>
      </div>

      <div class="sheet-group">
        <MiuixBasicComponent
          :title="t('issuer.detailSheet.leafSubject')"
          :summary="kb.leaf_subject || '—'"
          :clickable="!!kb.leaf_subject"
          @click="kb.leaf_subject && copyText(kb.leaf_subject, t('issuer.detailSheet.copySubject'))"
        >
          <template v-if="kb.leaf_subject" #end>
            <MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          :title="t('issuer.detailSheet.chainLength')"
          :summary="t('issuer.detailSheet.chainCount', kb.certificates)"
        />
        <MiuixBasicComponent
          :title="t('issuer.detailSheet.validUntil')"
          :summary="
            kb.not_after != null
              ? t('issuer.detailSheet.validUntilValue', { date: formatDateTime(kb.not_after), utc: kb.not_after_utc })
              : '—'
          "
        />
        <MiuixBasicComponent :title="t('issuer.detailSheet.remaining')" :summary="remaining" />
        <MiuixBasicComponent
          :title="t('issuer.detailSheet.keyMatch')"
          :summary="kb.key_match ? t('issuer.common.yes') : t('issuer.common.no')"
          :summary-color="kb.key_match ? undefined : 'var(--m-color-error)'"
        />
        <MiuixBasicComponent
          v-if="kb.error"
          :title="t('issuer.detailSheet.error')"
          :summary="kb.error"
          summary-color="var(--m-color-error)"
        />
      </div>

      <MiuixSmallTitle :text="t('issuer.detailSheet.pathAndDevice')" />
      <div class="sheet-group">
        <MiuixBasicComponent
          title="keybox"
          :summary="kb.path"
          clickable
          @click="copyText(kb.path, t('issuer.detailSheet.copyPath'))"
        >
          <template #end><MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" /></template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          :title="t('issuer.detailSheet.fingerprint')"
          :summary="status.device.fingerprint || '—'"
          :clickable="!!status.device.fingerprint"
          @click="status.device.fingerprint && copyText(status.device.fingerprint, t('issuer.detailSheet.copyFingerprint'))"
        >
          <template v-if="status.device.fingerprint" #end>
            <MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
      </div>

      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ t('issuer.detailSheet.note') }}
      </MiuixText>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.sheet-body :deep(.m-basic-component__center .m-text:last-child) {
  word-break: break-all;
}
</style>
