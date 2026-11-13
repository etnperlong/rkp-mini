<script setup lang="ts">
import { computed } from 'vue'
import { IconCheck, MiuixBasicComponent, MiuixCard, MiuixIcon } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { blockerLabel, type StatusData } from '../lib/rkp'
import { truncateMiddle } from '../../../shared/lib/format'
import { t } from '../../../shared/lib/i18n'

const props = defineProps<{ status: StatusData }>()

interface Row {
  title: string
  ok: boolean
  summary: string
}

const rows = computed<Row[]>(() => {
  const d = props.status.device
  const b = props.status.blockers
  const socIssue = b.find((x) => x === 'soc_unknown' || x === 'soc_conflicting')
  return [
    {
      title: t('issuer.checkCard.android'),
      ok: d.android_arm64,
      summary: d.android_arm64 ? t('issuer.checkCard.androidOk') : blockerLabel('android_arm64_required'),
    },
    {
      title: t('issuer.checkCard.root'),
      ok: d.root,
      summary: d.root ? t('issuer.checkCard.rootOk') : blockerLabel('root_required'),
    },
    {
      title: t('issuer.checkCard.qualcomm'),
      ok: d.qualcomm,
      summary: socIssue
        ? blockerLabel(socIssue)
        : d.qualcomm
          ? t('issuer.checkCard.qualcommOk')
          : blockerLabel('qualcomm_required'),
    },
    { title: t('issuer.checkCard.keymint'), ok: d.library_present, summary: '/vendor/lib64/libkeymasterdeviceutils.so' },
    { title: t('issuer.checkCard.smcinvoke'), ok: d.device_present, summary: '/dev/smcinvoke' },
    {
      title: t('issuer.checkCard.fingerprint'),
      ok: d.fingerprint !== '',
      summary: d.fingerprint ? truncateMiddle(d.fingerprint, 22) : blockerLabel('fingerprint_missing'),
    },
  ]
})
</script>

<template>
  <MiuixCard>
    <MiuixBasicComponent
      v-for="row in rows"
      :key="row.title"
      :title="row.title"
      :summary="row.summary"
      :summary-color="row.ok ? undefined : 'var(--m-color-error)'"
    >
      <template #end>
        <span
          class="mark"
          :class="row.ok ? 'mark--ok' : 'mark--bad'"
          :aria-label="row.ok ? t('issuer.checkCard.passed') : t('issuer.checkCard.failed')"
        >
          <IconCheck v-if="row.ok" />
          <MiuixIcon v-else :icon="Close" :size="16" weight="medium" />
        </span>
      </template>
    </MiuixBasicComponent>
  </MiuixCard>
</template>

<style scoped>
.mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--m-radius-full);
}
.mark :deep(svg) {
  width: 16px;
  height: 16px;
}
.mark--ok {
  background: var(--m-color-tertiary-container);
  color: var(--m-color-on-tertiary-container);
}
.mark--bad {
  background: var(--m-color-error-container);
  color: var(--m-color-error);
}
</style>
