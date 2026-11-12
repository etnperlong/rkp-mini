<script setup lang="ts">
// Deployed-keybox facts already present in `rkp-mini-sync status`: version,
// hash prefix, device id, expiry, per-target path + reload status, staged
// version and any pending approval. Never a keybox body or a secret.
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { TARGET_LABEL, type SyncStatus, type TargetState } from '../../../shared/lib/sync'
import { deployCodeLabel, leafLabel, pendingTitle, shortHash, stamp } from '../lib/display'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ status: SyncStatus | null; now: number }>()

const deployState = computed(() => props.status?.deploy_state ?? null)
const staged = computed(() => props.status?.staged ?? null)
const pending = computed(() => deployState.value?.pending ?? null)

/** Configured targets (their real path, e.g. a custom teesim name) plus recorded state. */
const targets = computed(() => {
  const stated = new Map<string, TargetState>(
    (deployState.value?.targets ?? []).map((t): [string, TargetState] => [t.id, t]),
  )
  const configured = Array.isArray(props.status?.targets) ? props.status.targets : []
  const ids = new Set<string>([...configured.map((t) => t.id), ...stated.keys()])
  return [...ids].map((id) => {
    const state = stated.get(id)
    return {
      id,
      path: state?.path ?? configured.find((t) => t.id === id)?.path ?? '—',
      reloadStatus: state?.reload_status ?? null,
      deployed: !!state?.deployed_sha256,
      lastError: state?.last_error ?? null,
    }
  })
})

function targetSummary(path: string, reloadStatus: string | null, deployed: boolean): string {
  if (!deployed) return `${path}（尚未部署）`
  return `${path}（reload ${reloadStatus ?? '—'}）`
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="部署详情">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText v-if="!deployState" type="body2" color="var(--m-color-on-surface-variant-summary)">
        还没有部署记录。
      </MiuixText>

      <template v-else>
        <MiuixSmallTitle text="已部署 keybox" />
        <div class="sheet-group">
          <MiuixBasicComponent title="版本" :summary="`第 ${deployState.deployed_seq ?? '—'} 版`" />
          <MiuixBasicComponent title="内容哈希" :summary="shortHash(deployState.deployed_keybox_sha256)" />
          <MiuixBasicComponent title="设备 ID" :summary="status?.sync_state?.device_id ?? '—'" />
          <MiuixBasicComponent title="到期" :summary="leafLabel(deployState.deployed_leaf_not_after, now)" />
          <MiuixBasicComponent title="部署时间" :summary="stamp(deployState.deployed_at)" />
        </div>

        <MiuixSmallTitle text="部署目标" />
        <div class="sheet-group">
          <MiuixBasicComponent
            v-for="t in targets"
            :key="t.id"
            :title="TARGET_LABEL[t.id] ?? t.id"
            :summary="targetSummary(t.path, t.reloadStatus, t.deployed)"
            :summary-color="t.lastError ? 'var(--m-color-error)' : undefined"
          />
        </div>

        <template v-if="staged">
          <MiuixSmallTitle text="已暂存的版本" />
          <div class="sheet-group">
            <MiuixBasicComponent title="版本" :summary="`第 ${staged.seq} 版`" />
            <MiuixBasicComponent title="内容哈希" :summary="shortHash(staged.keybox_sha256)" />
            <MiuixBasicComponent title="到期" :summary="leafLabel(staged.leaf_not_after, now)" />
            <MiuixBasicComponent title="暂存时间" :summary="stamp(staged.staged_at)" />
          </div>
        </template>

        <template v-if="pending">
          <MiuixSmallTitle text="等待确认" />
          <div class="sheet-group">
            <MiuixBasicComponent title="版本" :summary="`第 ${pending.seq} 版`" />
            <MiuixBasicComponent title="原因" :summary="pendingTitle(pending.reason)" />
            <MiuixBasicComponent title="到期" :summary="leafLabel(pending.leaf_not_after, now)" />
            <MiuixBasicComponent title="时间" :summary="stamp(pending.at)" />
          </div>
        </template>

        <MiuixText v-if="targets.some((t) => t.lastError)" type="footnote1" color="var(--m-color-error)">
          最近一次部署失败：
          <template v-for="t in targets.filter((x) => x.lastError)" :key="t.id">
            {{ TARGET_LABEL[t.id] ?? t.id }} {{ deployCodeLabel(t.lastError?.code ?? '') }}；
          </template>
        </MiuixText>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          只显示本机部署状态与哈希前缀；keybox 内容、密码和云端凭据不会出现在这里。
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
