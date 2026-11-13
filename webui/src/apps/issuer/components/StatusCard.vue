<script setup lang="ts">
import { computed } from 'vue'
import { MiuixCard, MiuixIcon, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import CardStateButton from '../../../shared/components/CardStateButton.vue'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { CommandError } from '../../../shared/lib/command'
import type { IssueRunner } from '../lib/issueRunner'
import { KEYBOX_STATE_LABEL, KEYBOX_STATE_VISUAL, keyboxState, type StatusData } from '../lib/rkp'
import { daysLeft, formatAgo, formatDateTime, formatElapsed } from '../../../shared/lib/format'
import { t, type MessageKey } from '../../../shared/lib/i18n'

const props = defineProps<{
  status: StatusData | null
  loading: boolean
  error: CommandError | null
  /** Shown data came from status.json because an issuance is running. */
  cached: boolean
  runner: IssueRunner
  now: number
}>()

const emit = defineEmits<{ detail: []; refresh: [] }>()

const ERROR_KIND: Record<CommandError['kind'], MessageKey> = {
  exec: 'issuer.statusCard.kind.exec',
  exit: 'issuer.statusCard.kind.exit',
  parse: 'issuer.statusCard.kind.parse',
}

const r = props.runner
const state = computed(() => (props.status ? keyboxState(props.status.keybox, props.now) : null))
const kb = computed(() => props.status?.keybox ?? null)
const days = computed(() => (kb.value?.not_after != null ? daysLeft(kb.value.not_after, props.now) : null))

/** Same corner as the issue/sync cards: state label, or the running issuance. */
const cornerText = computed(() => {
  if (r.busy.value)
    return r.overdue.value
      ? t('issuer.statusCard.runningOverdue', { elapsed: formatElapsed(r.elapsedMs.value) })
      : t('issuer.statusCard.running', { elapsed: formatElapsed(r.elapsedMs.value) })
  return state.value ? t(KEYBOX_STATE_LABEL[state.value]) : t('issuer.statusCard.readStateFailed')
})
</script>

<template>
  <!-- KernelSU Miuix home style: state-tinted card with an oversized icon clipped at the bottom-right corner. -->
  <MiuixCard
    class="status-card"
    :class="state && `status-card--${state}`"
    :press-feedback="kb?.exists ? 'tilt' : 'none'"
    :show-indication="!!kb?.exists"
    @click="kb?.exists && emit('detail')"
  >
    <SvgIcon v-if="state" class="status__mark" :path="KEYBOX_STATE_VISUAL[state].icon" :color="KEYBOX_STATE_VISUAL[state].color" />
    <div class="status">
      <div class="status__head">
        <MiuixText type="title4" weight="medium">{{ t('issuer.statusCard.title') }}</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <!-- Corner status doubles as the refresh control (re-reads status and the sync status). -->
        <CardStateButton
          v-else-if="status || error"
          :label="cornerText"
          :busy="r.busy.value || loading"
          :disabled="false"
          @refresh="emit('refresh')"
        />
      </div>

      <!-- Read failure with no data to fall back on -->
      <div v-if="error && !status" class="warnings">
        <MiuixText type="body2" color="var(--m-color-error)">
          {{ t('issuer.statusCard.readFailed', { kind: t(ERROR_KIND[error.kind]) }) }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ error.message }}</MiuixText>
      </div>

      <template v-else-if="status && kb && state">
        <div v-if="state === 'valid' || state === 'expiring'" class="status__hero">
          <span class="status__days">{{ days }}</span>
          <MiuixText type="body1">{{ t('issuer.statusCard.daysLeft', days ?? 0) }}</MiuixText>
        </div>
        <MiuixText v-else-if="state === 'expired'" type="title3" color="var(--m-color-error)">
          {{ t('issuer.statusCard.expired') }}
        </MiuixText>
        <MiuixText v-else-if="state === 'invalid'" type="title3" color="var(--m-color-error)">
          {{ t('issuer.statusCard.invalid') }}
        </MiuixText>
        <MiuixText v-else type="title3">{{ t('issuer.statusCard.missing') }}</MiuixText>
        <MiuixText v-if="kb.not_after != null" type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{
            state === 'expired'
              ? t('issuer.statusCard.expiredAt', { time: formatDateTime(kb.not_after) })
              : t('issuer.statusCard.validUntil', { time: formatDateTime(kb.not_after) })
          }}
        </MiuixText>
        <MiuixText v-if="state === 'invalid' && kb.error" type="body2" color="var(--m-color-error)">
          {{ kb.error }}
        </MiuixText>

        <div v-if="kb.exists" class="status__stats">
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('issuer.statusCard.chain') }}</MiuixText>
            <MiuixText type="body1" weight="medium">{{ t('issuer.statusCard.chainCount', kb.certificates) }}</MiuixText>
          </div>
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
              {{ t('issuer.statusCard.keyMatch') }}
            </MiuixText>
            <MiuixText type="body1" weight="medium" :color="kb.key_match ? undefined : 'var(--m-color-error)'">
              {{ kb.key_match ? t('issuer.common.yes') : t('issuer.common.no') }}
            </MiuixText>
          </div>
          <MiuixIcon class="stat__more" :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{
            cached
              ? t('issuer.statusCard.cachedCheckedAt', { ago: formatAgo(status.generated_at, now) })
              : t('issuer.statusCard.checkedAt', { ago: formatAgo(status.generated_at, now) })
          }}
        </MiuixText>
        <div v-if="error" class="warnings">
          <MiuixText type="footnote1" color="var(--m-color-error)">
            {{
              t('issuer.statusCard.refreshFailed', {
                kind: t(ERROR_KIND[error.kind]),
                message: error.message,
              })
            }}
          </MiuixText>
        </div>
      </template>

      <MiuixText v-else-if="loading" type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('shared.common.loading') }}
      </MiuixText>
    </div>

    <div v-if="error?.detail" class="status__fold" @pointerdown.stop @click.stop>
      <FoldBlock :title="t('issuer.statusCard.errorDetail')" :text="error.detail" />
    </div>
  </MiuixCard>
</template>

<style scoped>
/* MiuixCard paints its background from --m-card-color. Without color-mix() the
   warning/error cards keep the default surface; the corner icon still shows the state. */
.status-card--valid {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .status-card--expiring {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .status-card--expired,
  .status-card--invalid {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* `hidden` still lets focus/scrollIntoView pan the card sideways toward the
   oversized corner icon; `clip` never scrolls. */
.status-card :deep(.m-card) {
  overflow: clip;
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.status__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.status,
.status ~ * {
  position: relative;
}
.status {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.status__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.status__hero {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.status__days {
  font-size: 44px;
  font-weight: 600;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}
.status__stats {
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
  .status__stats {
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
