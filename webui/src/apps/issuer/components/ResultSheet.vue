<script setup lang="ts">
import { computed } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
} from 'miuix-vue'
import { Close, Copy } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import StatusPill from '../../../shared/components/StatusPill.vue'
import type { IssueRunner } from '../lib/issueRunner'
import { exitLabel, failureHint, stageLabel } from '../lib/rkp'
import { formatDateTime, formatElapsed } from '../../../shared/lib/format'
import { copyText } from '../../../shared/lib/clipboard'
import { t } from '../../../shared/lib/i18n'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ runner: IssueRunner }>()
const emit = defineEmits<{ logs: [] }>()

const r = props.runner
const phase = r.phase
const result = r.result

const success = computed(() => (result.value?.envelope?.ok ? result.value.envelope.data : null))
const failure = computed(() => (result.value?.envelope && !result.value.envelope.ok ? result.value.envelope.error : null))
const hint = computed(() => (failure.value ? failureHint(failure.value) : null))

const title = computed(() => {
  switch (phase.value) {
    case 'starting':
    case 'running':
      return t('issuer.resultSheet.titleRunning')
    case 'stale':
      return t('issuer.resultSheet.titleStale')
    case 'launch_failed':
      return t('issuer.resultSheet.titleLaunchFailed')
    case 'finished':
      return result.value?.code === 0 ? t('issuer.resultSheet.titleSuccess') : t('issuer.resultSheet.titleFailure')
    default:
      return t('issuer.resultSheet.title')
  }
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="title">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <!-- In progress -->
      <template v-if="r.busy.value">
        <div class="progress">
          <MiuixProgressIndicator type="infinite" :size="36" color="var(--m-color-primary)" />
          <MiuixText type="title2" class="elapsed">{{ formatElapsed(r.elapsedMs.value) }}</MiuixText>
          <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)" align="center">
            {{ t('issuer.resultSheet.steps') }}
          </MiuixText>
        </div>
        <MiuixText v-if="r.overdue.value" type="body2" color="var(--app-warning)">
          {{ t('issuer.resultSheet.overdue') }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('issuer.resultSheet.backgroundNote') }}
        </MiuixText>
        <MiuixText v-if="r.probeError.value" type="footnote1" color="var(--m-color-error)">
          {{ t('issuer.resultSheet.probeFailed', { error: r.probeError.value }) }}
        </MiuixText>
      </template>

      <!-- Stale lock -->
      <template v-else-if="phase === 'stale'">
        <MiuixText type="body2">
          {{ t('issuer.resultSheet.staleNote') }}
        </MiuixText>
        <MiuixText v-if="r.startedAt.value" type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          {{ t('issuer.resultSheet.lockCreatedAt', { time: formatDateTime(r.startedAt.value) }) }}
        </MiuixText>
        <MiuixButton type="primary" @click="r.clearStaleLock()">{{ t('issuer.common.clearLock') }}</MiuixButton>
        <MiuixButton @click="emit('logs')">{{ t('issuer.resultSheet.viewLogs') }}</MiuixButton>
      </template>

      <!-- Launch failed -->
      <template v-else-if="phase === 'launch_failed'">
        <MiuixText type="body2">
          {{ t('issuer.resultSheet.launchFailedNote') }}
        </MiuixText>
        <MiuixButton @click="r.check()">{{ t('issuer.resultSheet.recheck') }}</MiuixButton>
      </template>

      <!-- Finished -->
      <template v-else-if="phase === 'finished' && result">
        <div class="head">
          <StatusPill :tone="result.code === 0 ? 'ok' : 'error'" :label="exitLabel(result.code)" />
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            {{ t('issuer.resultSheet.finishedAt', { time: formatDateTime(result.finishedAt), code: result.code }) }}
          </MiuixText>
        </div>

        <div v-if="success" class="sheet-group">
          <MiuixBasicComponent :title="t('issuer.resultSheet.leaf')" :summary="success.leaf_subject || t('issuer.resultSheet.noCn')" />
          <MiuixBasicComponent
            :title="t('issuer.resultSheet.validUntil')"
            :summary="formatDateTime(success.leaf_not_after)"
          />
          <MiuixBasicComponent
            :title="t('issuer.resultSheet.chain')"
            :summary="t('issuer.resultSheet.chainCount', success.chain_certificates)"
          />
          <MiuixBasicComponent
            title="Device ID"
            :summary="success.device_id"
            clickable
            @click="copyText(success.device_id, t('issuer.resultSheet.copyDeviceId'))"
          >
            <template #end><MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" /></template>
          </MiuixBasicComponent>
          <MiuixBasicComponent
            title="TA"
            :summary="
              t('issuer.resultSheet.ta', {
                status: success.ta_status,
                frame: success.frame_layout,
                csr: success.csr_len,
              })
            "
          />
          <MiuixBasicComponent title="keybox" :summary="success.keybox" />
        </div>

        <template v-else-if="failure">
          <div class="sheet-group">
            <MiuixBasicComponent
              :title="t('issuer.resultSheet.stage')"
              :summary="t('issuer.resultSheet.stageValue', { stage: stageLabel(failure.stage), code: failure.stage })"
            />
            <MiuixBasicComponent :title="t('issuer.resultSheet.errorCode')" :summary="failure.code" />
            <MiuixBasicComponent :title="t('issuer.resultSheet.message')" :summary="failure.message" />
            <MiuixBasicComponent
              v-if="failure.ta_status !== undefined"
              title="ta_status"
              :summary="String(failure.ta_status)"
            />
          </div>
          <MiuixText v-if="hint" type="body2" color="var(--m-color-on-surface-variant-summary)">{{ hint }}</MiuixText>
        </template>

        <MiuixText v-else type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('issuer.resultSheet.noOutput') }}
        </MiuixText>

        <MiuixText v-if="result.code !== 0" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('issuer.resultSheet.noReplace') }}
        </MiuixText>

        <div class="sheet-group">
          <FoldBlock v-if="failure?.head_hex" title="head_hex" :text="failure.head_hex" />
          <FoldBlock :title="t('issuer.resultSheet.stderr')" :text="result.stderr.trim()" />
          <FoldBlock :title="t('issuer.resultSheet.stdout')" :text="result.stdout.trim()" />
        </div>
        <MiuixButton @click="emit('logs')">{{ t('issuer.resultSheet.viewIssueLogs') }}</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.progress {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 16px 0 8px;
}
.elapsed {
  font-variant-numeric: tabular-nums;
}
.head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
</style>
