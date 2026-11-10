<script setup lang="ts">
import { computed } from 'vue'
import { IconCheck, MiuixBasicComponent, MiuixCard, MiuixIcon } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { blockerLabel, type StatusData } from '../lib/rkp'
import { truncateMiddle } from '../../../shared/lib/format'

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
    { title: 'Android arm64', ok: d.android_arm64, summary: d.android_arm64 ? '64 位 Android 构建' : blockerLabel('android_arm64_required') },
    { title: 'Root 权限', ok: d.root, summary: d.root ? '以 root 运行' : blockerLabel('root_required') },
    {
      title: '高通平台',
      ok: d.qualcomm,
      summary: socIssue ? blockerLabel(socIssue) : d.qualcomm ? 'Qualcomm SoC' : blockerLabel('qualcomm_required'),
    },
    { title: 'KeyMint 库', ok: d.library_present, summary: '/vendor/lib64/libkeymasterdeviceutils.so' },
    { title: 'smcinvoke 设备', ok: d.device_present, summary: '/dev/smcinvoke' },
    {
      title: '系统指纹',
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
        <span class="mark" :class="row.ok ? 'mark--ok' : 'mark--bad'" :aria-label="row.ok ? '通过' : '未通过'">
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
