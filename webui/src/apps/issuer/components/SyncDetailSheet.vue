<script setup lang="ts">
// Sync facts already present in `rkp-mini-sync status` (never a secret, keybox
// body or remote credential; the cached `remote_check` is local-only data).
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { daysLeft, formatDateTime } from '../../../shared/lib/format'
import { CHECK_OUTCOME_LABEL, syncErrorText, type SyncStatus } from '../../../shared/lib/sync'

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
  return `${formatDateTime(at)}（${d < 0 ? `已过期 ${-d} 天` : `剩余 ${d} 天`}）`
}

const checkedAt = computed(() => {
  const c = check.value
  if (!c) return '尚未检查'
  return `${CHECK_OUTCOME_LABEL[c.outcome] ?? c.outcome} · ${formatDateTime(c.at)}`
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="同步详情">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText v-if="!status" type="body2" color="var(--m-color-on-surface-variant-summary)">
        尚未读取到同步状态。
      </MiuixText>

      <template v-else>
        <MiuixSmallTitle text="位置与同步流" />
        <div class="sheet-group">
          <MiuixBasicComponent title="位置" :summary="status.location ?? '未配置'" />
          <MiuixBasicComponent title="同步 ID" :summary="status.stream_id" />
          <MiuixBasicComponent title="上次推送版本" :summary="state ? `第 ${state.verified_seq} 版` : '尚未推送'" />
        </div>

        <MiuixSmallTitle text="keybox" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="本机 keybox"
            :summary="local ? `${local.summary} · ${shortSha(local.keybox_sha256)}` : '尚未签发'"
          />
          <MiuixBasicComponent title="本机设备 ID" :summary="local?.device_id ?? '—'" />
          <MiuixBasicComponent title="本机到期" :summary="expiry(local?.leaf_not_after)" />
          <MiuixBasicComponent
            title="云端 keybox"
            :summary="remote ? `第 ${remote.seq} 版 · ${shortSha(remote.keybox_sha256)}` : '尚未检查'"
          />
          <MiuixBasicComponent title="云端设备 ID" :summary="remote?.device_id ?? '—'" />
          <MiuixBasicComponent title="云端到期" :summary="expiry(remote?.keybox_not_after)" />
          <MiuixBasicComponent title="云端检查" :summary="checkedAt" />
        </div>

        <MiuixSmallTitle text="错误" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="上次错误"
            :summary="state?.last_error ? syncErrorText(state.last_error) : '无'"
            :summary-color="state?.last_error ? 'var(--m-color-error)' : undefined"
          />
          <MiuixBasicComponent
            v-if="check?.error"
            title="检查错误"
            :summary="syncErrorText(check.error)"
            summary-color="var(--m-color-error)"
          />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          只显示本地状态与云端检查结果；密码、token 和 keybox 内容不会出现在这里。
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
