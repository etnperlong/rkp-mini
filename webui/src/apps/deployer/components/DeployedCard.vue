<script setup lang="ts">
// 状态 tab: the keybox currently deployed on this device. Everything comes from
// the local `rkp-mini-sync status` read; no keybox body, secret or remote
// credential is ever rendered. The corner status doubles as a local re-read.
import { computed } from 'vue'
import { MiuixCard, MiuixIcon, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import CardStateButton from '../../../shared/components/CardStateButton.vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_ERROR, ICON_HELP, ICON_WARNING } from '../../../shared/lib/icons'
import { daysLeft, formatAgo, formatDateTime, formatElapsed } from '../../../shared/lib/format'
import { targetLabel, type SyncOp, type SyncRunner, type SyncStatus, type TargetState } from '../../../shared/lib/sync'
import { t, type MessageKey } from '../../../shared/lib/i18n'
import { EXPIRING_DAYS, deployCodeLabel, pendingTitle } from '../lib/display'

const props = defineProps<{
  status: SyncStatus | null
  /** `status` failed with `not_initialized`: the `[sync]` table does not exist yet. */
  notInitialized: boolean
  loading: boolean
  /** Read failure with no usable status. */
  error: string | null
  runner: SyncRunner
  /** Op started from this page; null when the task predates the page. */
  busyOp: SyncOp | null
  now: number
}>()
const emit = defineEmits<{ detail: []; refresh: [] }>()

const r = props.runner

type CardState = 'unimported' | 'pending' | 'failed' | 'undeployed' | 'expired' | 'expiring' | 'valid'

const STATE_LABEL: Record<CardState, MessageKey> = {
  unimported: 'deployer.deployedCard.stateUnimported',
  pending: 'deployer.deployedCard.statePending',
  failed: 'deployer.deployedCard.stateFailed',
  undeployed: 'deployer.deployedCard.stateUndeployed',
  expired: 'deployer.deployedCard.stateExpired',
  expiring: 'deployer.deployedCard.stateExpiring',
  valid: 'deployer.deployedCard.stateValid',
}

/** Tint/icon per state; the icon paths come from shared/lib/icons.ts. */
const STATE_VISUAL: Record<CardState, { color: string; icon: string }> = {
  valid: { color: 'var(--app-ok)', icon: ICON_CHECK_CIRCLE },
  expiring: { color: 'var(--app-warning)', icon: ICON_WARNING },
  pending: { color: 'var(--app-warning)', icon: ICON_WARNING },
  expired: { color: 'var(--m-color-error)', icon: ICON_ERROR },
  failed: { color: 'var(--m-color-error)', icon: ICON_ERROR },
  undeployed: { color: 'var(--m-color-on-surface-variant-actions)', icon: ICON_HELP },
  unimported: { color: 'var(--m-color-on-surface-variant-actions)', icon: ICON_HELP },
}

const HEADLINE: Record<CardState, MessageKey | null> = {
  unimported: 'deployer.deployedCard.unimported',
  pending: 'deployer.deployedCard.headlinePending',
  failed: 'deployer.deployedCard.stateFailed',
  undeployed: 'deployer.deployedCard.headlineUndeployed',
  expired: 'deployer.deployedCard.headlineExpired',
  expiring: null,
  valid: null,
}

const deployState = computed(() => props.status?.deploy_state ?? null)
const targets = computed<TargetState[]>(() => deployState.value?.targets ?? [])
const deployed = computed(() => targets.value.filter((t) => !!t.deployed_sha256))
const failures = computed(() => targets.value.filter((t) => !!t.last_error))
const pending = computed(() => deployState.value?.pending ?? null)
const notAfter = computed(() => deployState.value?.deployed_leaf_not_after ?? null)
const days = computed(() => (notAfter.value != null ? daysLeft(notAfter.value, props.now) : null))

const state = computed<CardState | null>(() => {
  const s = props.status
  if (!s) return null
  if (props.notInitialized || !s.stream_id) return 'unimported'
  if (pending.value) return 'pending'
  if (failures.value.length) return 'failed'
  if (!deployed.value.length) return 'undeployed'
  if (days.value != null && days.value < 0) return 'expired'
  if (days.value != null && days.value <= EXPIRING_DAYS) return 'expiring'
  return 'valid'
})

const canDetail = computed(() => deployed.value.length > 0)
const headline = computed(() => {
  const key = state.value ? HEADLINE[state.value] : null
  return key ? t(key) : ''
})

const BUSY_LABEL: Record<string, MessageKey> = {
  check: 'deployer.common.busyCheck',
  fetch: 'deployer.common.busyFetch',
  run: 'deployer.common.busyRun',
}
const cornerText = computed(() => {
  if (r.busy.value) {
    const label = t(BUSY_LABEL[props.busyOp ?? 'run'] ?? 'deployer.common.busyRun')
    const elapsed = formatElapsed(r.elapsedMs.value)
    return r.overdue.value
      ? t('deployer.common.busyUnknown', { label, elapsed })
      : t('deployer.common.busyTimer', { label, elapsed })
  }
  if (props.notInitialized) return t(STATE_LABEL.unimported)
  return state.value ? t(STATE_LABEL[state.value]) : t('deployer.deployedCard.readFailed')
})
</script>

<template>
  <!-- KernelSU Miuix home style: state-tinted card with an oversized icon clipped at the bottom-right corner. -->
  <MiuixCard
    class="card"
    :class="state && `card--${state}`"
    :press-feedback="canDetail ? 'tilt' : 'none'"
    :show-indication="canDetail"
    @click="canDetail && emit('detail')"
  >
    <SvgIcon
      v-if="state"
      class="card__mark"
      :path="STATE_VISUAL[state].icon"
      :color="STATE_VISUAL[state].color"
    />
    <div class="deployed">
      <div class="deployed__head">
        <MiuixText type="title4" weight="medium">{{ t('deployer.deployedCard.title') }}</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <CardStateButton
          v-else-if="status || error || notInitialized"
          :label="cornerText"
          :busy="r.busy.value || loading"
          :disabled="loading"
          @refresh="emit('refresh')"
        />
      </div>

      <div v-if="error && !status" class="warnings">
        <MiuixText type="body2" color="var(--m-color-error)">{{ t('deployer.common.readStatusFailedDetail', { error }) }}</MiuixText>
      </div>

      <template v-else-if="notInitialized">
        <MiuixText type="title3">{{ t('deployer.deployedCard.unimported') }}</MiuixText>
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('deployer.deployedCard.unimportedHint') }}
        </MiuixText>
      </template>

      <template v-else-if="status && state">
        <div v-if="state === 'valid' || state === 'expiring'" class="hero">
          <span class="hero__days">{{ days }}</span>
          <MiuixText type="body1">{{ t('deployer.deployedCard.daysLeft', days ?? 0) }}</MiuixText>
        </div>
        <MiuixText
          v-else
          type="title3"
          :color="state === 'expired' || state === 'failed' ? 'var(--m-color-error)' : undefined"
        >
          {{ headline }}
        </MiuixText>

        <MiuixText v-if="notAfter != null" type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{
            days != null && days < 0
              ? t('deployer.deployedCard.expiredOn', { date: formatDateTime(notAfter) })
              : t('deployer.deployedCard.validUntil', { date: formatDateTime(notAfter) })
          }}
        </MiuixText>

        <MiuixText v-if="state === 'failed' && failures[0]" type="body2" color="var(--m-color-error)">
          {{
            t('deployer.deployedCard.targetFailed', {
              target: targetLabel(failures[0].id),
              reason: deployCodeLabel(failures[0].last_error?.code ?? ''),
            })
          }}
        </MiuixText>
        <MiuixText v-else-if="state === 'pending' && pending" type="body2" color="var(--app-warning)">
          {{ t('deployer.deployedCard.pendingVersion', { seq: pending.seq, reason: pendingTitle(pending.reason) }) }}
        </MiuixText>
        <MiuixText v-else-if="state === 'unimported'" type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('deployer.deployedCard.unimportedHint') }}
        </MiuixText>
        <MiuixText v-else-if="state === 'undeployed'" type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('deployer.deployedCard.undeployedHint') }}
        </MiuixText>

        <div v-if="deployed.length" class="stats">
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.deployedCard.deployedVersion') }}</MiuixText>
            <MiuixText type="body1" weight="medium">{{ t('deployer.common.versionSeq', { seq: deployState?.deployed_seq ?? '—' }) }}</MiuixText>
          </div>
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.common.deployTargets') }}</MiuixText>
            <MiuixText type="body1" weight="medium">{{ deployed.length }}/{{ targets.length }}</MiuixText>
          </div>
          <MiuixIcon class="stat__more" :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{
            deployState?.deployed_at
              ? t('deployer.deployedCard.deployedAt', { time: formatAgo(deployState.deployed_at, now) })
              : t('deployer.deployedCard.localOnly')
          }}
        </MiuixText>
      </template>

      <MiuixText v-else type="body2" color="var(--m-color-on-surface-variant-summary)">{{ t('shared.common.loading') }}</MiuixText>
    </div>
  </MiuixCard>
</template>

<style scoped>
/* MiuixCard paints its background from --m-card-color; without color-mix() the
   warning/error cards keep the default surface and the corner icon still shows
   the state. */
.card--valid {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .card--expiring,
  .card--pending {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .card--expired,
  .card--failed {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* `hidden` still lets focus/scrollIntoView pan the card sideways toward the
   oversized corner icon; `clip` never scrolls. */
.card :deep(.m-card) {
  overflow: clip;
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.card__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.deployed,
.deployed ~ * {
  position: relative;
}
.deployed {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.deployed__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.hero {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.hero__days {
  font-size: 44px;
  font-weight: 600;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}
.stats {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 8px 0 4px;
  padding: 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
/* Translucent so the tinted card shows through. */
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .stats {
    background: color-mix(in srgb, var(--m-color-surface-container) 55%, transparent);
  }
}
.stat {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.stat__more {
  flex: none;
}
.warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
</style>
