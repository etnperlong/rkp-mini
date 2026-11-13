<script setup lang="ts">
// Deployed-keybox facts already present in `rkp-mini-sync status`: version,
// hash prefix, device id, expiry, per-target path + reload status, staged
// version and any pending approval. Never a keybox body or a secret.
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { targetLabel, type SyncStatus, type TargetState } from '../../../shared/lib/sync'
import { t } from '../../../shared/lib/i18n'
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
  if (!deployed) return t('deployer.deployedDetail.notDeployed', { path })
  return t('deployer.deployedDetail.reload', { path, status: reloadStatus ?? '—' })
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('deployer.deployedDetail.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText v-if="!deployState" type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('deployer.deployedDetail.empty') }}
      </MiuixText>

      <template v-else>
        <MiuixSmallTitle :text="t('deployer.common.deployedKeybox')" />
        <div class="sheet-group">
          <MiuixBasicComponent :title="t('deployer.common.version')" :summary="t('deployer.common.versionSeq', { seq: deployState.deployed_seq ?? '—' })" />
          <MiuixBasicComponent :title="t('deployer.common.contentHash')" :summary="shortHash(deployState.deployed_keybox_sha256)" />
          <MiuixBasicComponent :title="t('deployer.common.deviceId')" :summary="status?.sync_state?.device_id ?? '—'" />
          <MiuixBasicComponent :title="t('deployer.deployedDetail.due')" :summary="leafLabel(deployState.deployed_leaf_not_after, now)" />
          <MiuixBasicComponent :title="t('deployer.deployedDetail.deployedAt')" :summary="stamp(deployState.deployed_at)" />
        </div>

        <MiuixSmallTitle :text="t('deployer.common.deployTargets')" />
        <div class="sheet-group">
          <MiuixBasicComponent
            v-for="tg in targets"
            :key="tg.id"
            :title="targetLabel(tg.id)"
            :summary="targetSummary(tg.path, tg.reloadStatus, tg.deployed)"
            :summary-color="tg.lastError ? 'var(--m-color-error)' : undefined"
          />
        </div>

        <template v-if="staged">
          <MiuixSmallTitle :text="t('deployer.deployedDetail.staged')" />
          <div class="sheet-group">
            <MiuixBasicComponent :title="t('deployer.common.version')" :summary="t('deployer.common.versionSeq', { seq: staged.seq })" />
            <MiuixBasicComponent :title="t('deployer.common.contentHash')" :summary="shortHash(staged.keybox_sha256)" />
            <MiuixBasicComponent :title="t('deployer.deployedDetail.due')" :summary="leafLabel(staged.leaf_not_after, now)" />
            <MiuixBasicComponent :title="t('deployer.deployedDetail.stagedAt')" :summary="stamp(staged.staged_at)" />
          </div>
        </template>

        <template v-if="pending">
          <MiuixSmallTitle :text="t('deployer.deployedDetail.pending')" />
          <div class="sheet-group">
            <MiuixBasicComponent :title="t('deployer.common.version')" :summary="t('deployer.common.versionSeq', { seq: pending.seq })" />
            <MiuixBasicComponent :title="t('deployer.deployedDetail.reason')" :summary="pendingTitle(pending.reason)" />
            <MiuixBasicComponent :title="t('deployer.deployedDetail.due')" :summary="leafLabel(pending.leaf_not_after, now)" />
            <MiuixBasicComponent :title="t('deployer.deployedDetail.time')" :summary="stamp(pending.at)" />
          </div>
        </template>

        <MiuixText v-if="targets.some((x) => x.lastError)" type="footnote1" color="var(--m-color-error)">
          {{ t('deployer.deployedDetail.lastFailed') }}
          <template v-for="tg in targets.filter((x) => x.lastError)" :key="tg.id">
            {{ t('deployer.deployedDetail.lastFailedItem', { target: targetLabel(tg.id), reason: deployCodeLabel(tg.lastError?.code ?? '') }) }}
          </template>
        </MiuixText>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.deployedDetail.footnote') }}
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
