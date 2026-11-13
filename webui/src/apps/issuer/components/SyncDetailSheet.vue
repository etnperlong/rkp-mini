<script setup lang="ts">
// Sync facts already present in `rkp-mini-sync status` (never a secret, keybox
// body or remote credential; the cached `remote_check` is local-only data).
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { daysLeft, formatDateTime } from '../../../shared/lib/format'
import { checkOutcomeLabel, syncErrorText, type SyncStatus } from '../../../shared/lib/sync'
import { t } from '../../../shared/lib/i18n'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ status: SyncStatus | null; now: number }>()

const local = computed(() => props.status?.local_keybox ?? null)
const state = computed(() => props.status?.sync_state ?? null)
const check = computed(() => props.status?.remote_check ?? null)
const remote = computed(() => check.value?.remote ?? null)

function shortSha(value: string | null | undefined): string {
  return value ? value.slice(0, 12) : '—'
}

function expiry(at: number | null | undefined): string {
  if (at == null) return '—'
  const d = daysLeft(at, props.now)
  return d < 0
    ? t('issuer.syncDetailSheet.expiredAgo', { date: formatDateTime(at), n: -d }, -d)
    : t('issuer.syncDetailSheet.remainDays', { date: formatDateTime(at), n: d }, d)
}

const checkedAt = computed(() => {
  const c = check.value
  if (!c) return t('issuer.syncDetailSheet.notChecked')
  return t('issuer.syncDetailSheet.checkedAt', {
    label: checkOutcomeLabel(c.outcome) ?? c.outcome,
    time: formatDateTime(c.at),
  })
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.syncDetailSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText v-if="!status" type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('issuer.syncDetailSheet.noStatus') }}
      </MiuixText>

      <template v-else>
        <MiuixSmallTitle :text="t('issuer.syncDetailSheet.locationSection')" />
        <div class="sheet-group">
          <MiuixBasicComponent
            :title="t('issuer.syncDetailSheet.location')"
            :summary="status.location ?? t('issuer.syncDetailSheet.unconfigured')"
          />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.streamId')" :summary="status.stream_id" />
          <MiuixBasicComponent
            :title="t('issuer.syncDetailSheet.lastPushed')"
            :summary="
              state ? t('issuer.syncDetailSheet.version', state.verified_seq) : t('issuer.syncDetailSheet.notPushed')
            "
          />
        </div>

        <MiuixSmallTitle text="keybox" />
        <div class="sheet-group">
          <MiuixBasicComponent
            :title="t('issuer.syncDetailSheet.localKeybox')"
            :summary="
              local
                ? t('issuer.syncDetailSheet.localSummary', {
                    summary: local.summary,
                    sha: shortSha(local.keybox_sha256),
                  })
                : t('issuer.syncDetailSheet.notIssued')
            "
          />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.localDeviceId')" :summary="local?.device_id ?? '—'" />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.localExpiry')" :summary="expiry(local?.leaf_not_after)" />
          <MiuixBasicComponent
            :title="t('issuer.syncDetailSheet.remoteKeybox')"
            :summary="
              remote
                ? t(
                    'issuer.syncDetailSheet.remoteSummary',
                    { n: remote.seq, sha: shortSha(remote.keybox_sha256) },
                    remote.seq,
                  )
                : t('issuer.syncDetailSheet.notChecked')
            "
          />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.remoteDeviceId')" :summary="remote?.device_id ?? '—'" />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.remoteExpiry')" :summary="expiry(remote?.keybox_not_after)" />
          <MiuixBasicComponent :title="t('issuer.syncDetailSheet.remoteCheck')" :summary="checkedAt" />
        </div>

        <MiuixSmallTitle :text="t('issuer.syncDetailSheet.errorSection')" />
        <div class="sheet-group">
          <MiuixBasicComponent
            :title="t('issuer.syncDetailSheet.lastError')"
            :summary="state?.last_error ? syncErrorText(state.last_error) : t('issuer.syncDetailSheet.none')"
            :summary-color="state?.last_error ? 'var(--m-color-error)' : undefined"
          />
          <MiuixBasicComponent
            v-if="check?.error"
            :title="t('issuer.syncDetailSheet.checkError')"
            :summary="syncErrorText(check.error)"
            summary-color="var(--m-color-error)"
          />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('issuer.syncDetailSheet.note') }}
        </MiuixText>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.sheet-body :deep(.m-basic-component__center .m-text:last-child) {
  word-break: break-all;
}
</style>
