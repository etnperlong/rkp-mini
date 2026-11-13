<script setup lang="ts">
// Two small cards: are unattended fetch and deploy actually active? Fetch needs
// sync enabled with the current risk accepted, a stored password (prompt mode
// cannot run unattended), one verified remote version, and the loop; deploy
// also needs `auto_deploy`. Tapping a box opens the 部署 tab.
import { computed } from 'vue'
import { MiuixCard, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_WARNING } from '../../../shared/lib/icons'
import { t } from '../../../shared/lib/i18n'
import type { SyncConfig, SyncStatus } from '../../../shared/lib/sync'

const props = defineProps<{
  status: SyncStatus | null
  /** `config show`: only `auto_deploy` is needed here. */
  config: SyncConfig | null
  loading: boolean
  /** Read failure message, or null when the last read succeeded. */
  error: string | null
}>()
const emit = defineEmits<{ open: [tab: number] }>()

interface Box {
  title: string
  on: boolean
  hint: string
  pending: boolean
  tab: number
}

/** Why automatic fetch is off, or null when it runs. */
const autoFetchOff = computed<string | null>(() => {
  const s = props.status
  if (!s) return props.error ? t('deployer.common.readStatusFailed') : t('shared.common.reading')
  if (!s.stream_id) return t('deployer.autoStatus.unimported')
  if (!s.enabled) return t('deployer.common.syncDisabled')
  if (!s.risk_ack_current) return t('deployer.common.riskStale')
  if (s.password_mode !== 'secret-file') return t('deployer.autoStatus.promptMode')
  if (!s.sync_state?.verified_at) return t('deployer.autoStatus.unverified')
  if (!s.loop_running) return t('deployer.common.loopOff')
  return null
})

/** Why automatic deploy is off, or null when it runs. */
const autoDeployOff = computed<string | null>(() => {
  if (autoFetchOff.value) return autoFetchOff.value
  if (!props.config) return t('deployer.autoStatus.readingConfig')
  if (props.config.auto_deploy === false) return t('deployer.common.autoDeployOff')
  return null
})

const boxes = computed<Box[]>(() => [
  {
    title: t('deployer.autoStatus.autoFetch'),
    on: autoFetchOff.value === null,
    hint: autoFetchOff.value ?? t('deployer.autoStatus.checkEvery'),
    pending: props.loading && !props.status,
    tab: 1,
  },
  {
    title: t('deployer.common.autoDeploy'),
    on: autoDeployOff.value === null,
    hint: autoDeployOff.value ?? t('deployer.autoStatus.deployAfterFetch'),
    pending: props.loading && !props.status,
    tab: 1,
  },
])
</script>

<template>
  <div class="row">
    <MiuixCard
      v-for="b in boxes"
      :key="b.title"
      class="box"
      press-feedback="sink"
      show-indication
      @click="emit('open', b.tab)"
    >
      <div class="box__body">
        <div class="box__head">
          <MiuixText type="body1" weight="medium">{{ b.title }}</MiuixText>
          <MiuixProgressIndicator v-if="b.pending" type="infinite" :size="20" />
          <SvgIcon
            v-else
            class="box__icon"
            :path="b.on ? ICON_CHECK_CIRCLE : ICON_WARNING"
            :color="b.on ? 'var(--app-ok)' : 'var(--app-warning)'"
          />
        </div>
        <MiuixText type="body2" weight="medium">{{ b.pending ? t('deployer.autoStatus.readingShort') : b.on ? t('deployer.common.enabled') : t('deployer.autoStatus.disabled') }}</MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ b.hint }}</MiuixText>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  gap: 12px;
}
.box {
  flex: 1;
  min-width: 0;
}
.box__body {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.box__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 4px;
}
.box__icon {
  width: 22px;
  height: 22px;
  flex: none;
}
</style>
