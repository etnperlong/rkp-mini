<script setup lang="ts">
// 状态 tab: the builtin deploy targets whose root module is installed, with
// its enabled flag, whether it is a configured deploy target and the last
// deploy result. Read-only: management lives in 部署设置.
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixCard, MiuixProgressIndicator } from 'miuix-vue'
import { targetLabel, type SyncStatus, type TargetState } from '../../../shared/lib/sync'
import { t } from '../../../shared/lib/i18n'
import type { BuiltinTargetInfo } from '../lib/deploy'
import { deployCodeLabel } from '../lib/display'

const props = defineProps<{
  /** `rkp-mini-deployer targets`; null while loading or after a detection failure. */
  builtins: BuiltinTargetInfo[] | null
  error: string | null
  status: SyncStatus | null
  loading: boolean
}>()

const installed = computed(() => (props.builtins ?? []).filter((b) => b.module.installed))

const configured = computed(() => {
  const targets = props.status?.targets
  return new Map<string, string>(
    Array.isArray(targets) ? targets.map((t): [string, string] => [t.id, t.path]) : [],
  )
})

const results = computed(
  () => new Map<string, TargetState>((props.status?.deploy_state?.targets ?? []).map((t) => [t.id, t])),
)

const rows = computed(() =>
  installed.value.map((b) => {
    const state = results.value.get(b.id)
    const parts = [
      b.module.enabled ? t('deployer.common.enabled') : t('deployer.modules.stopped'),
      configured.value.has(b.id) ? t('deployer.common.deployTargets') : t('deployer.modules.notTarget'),
    ]
    const path = state?.path ?? configured.value.get(b.id)
    if (path) parts.push(path)
    if (state?.last_error) parts.push(t('deployer.modules.lastFailed', { reason: deployCodeLabel(state.last_error.code) }))
    else if (state?.deployed_sha256) parts.push(t('deployer.common.deployedKeybox'))
    else if (configured.value.has(b.id)) parts.push(t('deployer.common.notDeployedKeybox'))
    return {
      id: b.id,
      title: targetLabel(b.id),
      summary: parts.join(' · '),
      bad: !!state?.last_error,
    }
  }),
)
</script>

<template>
  <MiuixCard>
    <MiuixBasicComponent v-if="loading && builtins === null" :title="t('deployer.modules.detecting')">
      <template #end><MiuixProgressIndicator type="infinite" :size="20" /></template>
    </MiuixBasicComponent>
    <MiuixBasicComponent
      v-else-if="builtins === null"
      :title="t('deployer.modules.detectFailed')"
      :summary="error ?? t('deployer.modules.unknownError')"
      summary-color="var(--m-color-error)"
    />
    <MiuixBasicComponent
      v-else-if="!installed.length"
      :title="t('deployer.modules.noneInstalled')"
      summary="OhMyKeymint / TEESimulator / TrickyStore"
    />
    <template v-else>
      <MiuixBasicComponent
        v-for="row in rows"
        :key="row.id"
        :title="row.title"
        :summary="row.summary"
        :summary-color="row.bad ? 'var(--m-color-error)' : undefined"
      />
    </template>
  </MiuixCard>
</template>
