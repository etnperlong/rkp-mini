<script setup lang="ts">
// Last combined run (`run.sh`). The module publishes one eight-section
// snapshot, not a CLI envelope: each step carries its own exit code, envelope
// and stderr. A step with no code and no envelope did not run; a non-zero code
// without an envelope means it crashed and is never shown as success.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixSmallTitle,
  MiuixText,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import StatusPill, { type PillTone } from '../../../shared/components/StatusPill.vue'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import {
  deployCodeLabel,
  deployErrorText,
  gateTitle,
  leafLabel,
  providerLabel,
  shortHash,
  stamp,
} from '../lib/display'
import { nowUnix } from '../../../shared/lib/format'
import {
  targetLabel,
  warningLabel,
  syncErrorText,
  type DeployPlan,
  type SyncEnvelope,
  type SyncErrorInfo,
  type TofuFacts,
} from '../../../shared/lib/sync'
import { t, type MessageKey } from '../../../shared/lib/i18n'
import {
  isDeployReport,
  isDeploySkipped,
  runDeployOutcome,
  runOutcomeLabel,
  runRebootHint,
  type DeploySkip,
  type RunSteps,
  type StepResult,
} from '../lib/run'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ steps: RunSteps | null }>()

const now = ref(nowUnix())

watch(open, (v) => {
  if (v) now.value = nowUnix()
})

const SKIP_LABEL: Record<DeploySkip, MessageKey> = {
  nothing_staged: 'deployer.runResult.skipNothingStaged',
  auto_deploy_false: 'deployer.runResult.skipAutoDeployOff',
}

type StepView<T> =
  | { kind: 'not_run' }
  | { kind: 'ok'; data: T }
  | { kind: 'error'; error: SyncErrorInfo; data: unknown }
  | { kind: 'abnormal'; code: number }

function stepView<T>(step: StepResult<T>): StepView<T> {
  const env: SyncEnvelope<T> | null = step.envelope
  if (env) {
    return env.ok ? { kind: 'ok', data: env.data } : { kind: 'error', error: env.error, data: env.data }
  }
  // No envelope: empty code means the step did not run; any code (including 0)
  // means it ran and its output is missing, which is never a success.
  return step.code === null ? { kind: 'not_run' } : { kind: 'abnormal', code: step.code }
}

const fetchStep = computed(() => (props.steps ? stepView(props.steps.fetch) : null))
const deployStep = computed(() => (props.steps ? stepView(props.steps.deploy) : null))

function isFetchResult(value: unknown): value is { changed: boolean; seq: number; device_id?: string; keybox_sha256?: string; leaf_not_after?: number; gap?: boolean } {
  return typeof value === 'object' && value !== null && 'changed' in value
}

function isTofu(value: unknown): value is TofuFacts {
  return (
    typeof value === 'object' &&
    value !== null &&
    'plaintext_sha256' in value &&
    'device_id' in value &&
    'issued_at' in value
  )
}

function isDeployPlan(value: unknown): value is DeployPlan {
  return typeof value === 'object' && value !== null && 'gate' in value && 'staged' in value
}

const outcome = computed(() => (props.steps && props.steps.overall === 0 ? runDeployOutcome(props.steps) : null))
const rebootHint = computed(() => (outcome.value ? runRebootHint(outcome.value) : null))

function stepTone(view: StepView<unknown> | null, code: number | null): PillTone {
  if (!view || view.kind === 'not_run') return 'neutral'
  if (view.kind === 'abnormal') return 'error'
  return code === 0 ? 'ok' : 'error'
}

function stepLabel(view: StepView<unknown> | null, code: number | null): string {
  if (!view || view.kind === 'not_run') return t('deployer.runResult.notRun')
  if (view.kind === 'abnormal') {
    return code === 0 ? t('deployer.runResult.noResultExit0') : t('deployer.runResult.abnormalExit', { code })
  }
  return code === 0 ? t('deployer.runResult.okExit0') : t('deployer.runResult.failExit', { code })
}

function skipLabel(value: { skipped: DeploySkip }): string {
  return t(SKIP_LABEL[value.skipped] ?? 'deployer.runResult.skipUnknown', { code: value.skipped })
}

const overallOk = computed(() => props.steps?.overall === 0)
const tofuFromFetch = computed(() => {
  const view = fetchStep.value
  if (!view || view.kind !== 'error') return null
  if (view.error.code !== 'first_import_needs_confirmation' && view.error.code !== 'confirmation_mismatch') {
    return null
  }
  return isTofu(view.data) ? view.data : null
})
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('deployer.runResult.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <template v-if="!steps">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('deployer.runResult.empty') }}
        </MiuixText>
      </template>

      <template v-else>
        <div class="outcome">
          <MiuixText type="body1" :color="outcome?.kind === 'failed' ? 'var(--m-color-error)' : undefined">
            {{ outcome ? runOutcomeLabel(outcome) : t('deployer.runResult.title') }}
          </MiuixText>
          <MiuixText v-if="rebootHint" type="footnote1" color="var(--app-warning)">{{ rebootHint }}</MiuixText>
        </div>
        <div class="row">
          <span class="row__k">{{ t('deployer.runResult.overall') }}</span>
          <StatusPill
            :tone="overallOk ? 'ok' : 'error'"
            :label="overallOk ? t('deployer.runResult.okExit0') : t('deployer.runResult.failExit', { code: steps.overall })"
          />
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.runResult.overallNote') }}
        </MiuixText>

        <MiuixSmallTitle :text="t('deployer.run.labelFetch')" />
        <div class="section">
          <div class="row">
            <span class="row__k">{{ t('deployer.runResult.exitCode') }}</span>
            <StatusPill
              :tone="stepTone(fetchStep, steps.fetch.code)"
              :label="stepLabel(fetchStep, steps.fetch.code)"
            />
          </div>

          <template v-if="fetchStep?.kind === 'not_run'">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.runResult.notRunPeriod') }}</MiuixText>
          </template>

          <template v-else-if="fetchStep?.kind === 'abnormal'">
            <MiuixText v-if="fetchStep.code !== 0" type="body2" color="var(--m-color-error)">
              {{ t('deployer.runResult.stepAbnormal', { code: fetchStep.code }) }}
            </MiuixText>
            <MiuixText v-else type="body2" color="var(--m-color-error)">
              {{ t('deployer.runResult.stepNoJson') }}
            </MiuixText>
          </template>

          <template v-else-if="fetchStep?.kind === 'error'">
            <MiuixText type="body2" color="var(--m-color-error)">
              {{ syncErrorText(fetchStep.error) }}
            </MiuixText>
            <MiuixText
              v-if="fetchStep.error.code === 'first_import_needs_confirmation'"
              type="footnote1"
              color="var(--m-color-on-surface-variant-summary)"
            >
              {{ t('deployer.runResult.tofuNote') }}
            </MiuixText>
            <template v-if="tofuFromFetch">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.firstFetch') }}</span>
                <span class="row__v">seq {{ tofuFromFetch.seq }} · {{ providerLabel(tofuFromFetch.provider) }}</span>
              </div>
              <div class="row">
                <span class="row__k">{{ t('deployer.common.deviceId') }}</span>
                <span class="row__v selectable">{{ tofuFromFetch.device_id }}</span>
              </div>
              <div class="row">
                <span class="row__k">{{ t('deployer.common.issuedAt') }}</span>
                <span class="row__v">{{ stamp(tofuFromFetch.issued_at) }}</span>
              </div>
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.leafValidity') }}</span>
                <span class="row__v">{{ leafLabel(tofuFromFetch.leaf_not_after, now) }}</span>
              </div>
              <div class="row">
                <span class="row__k">keybox</span>
                <span class="row__v mono">{{ shortHash(tofuFromFetch.keybox_sha256) }}</span>
              </div>
            </template>
          </template>

          <template v-else-if="fetchStep?.kind === 'ok'">
            <template v-if="isFetchResult(fetchStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.common.version') }}</span>
                <span class="row__v">
                  seq {{ fetchStep.data.seq }} · {{ fetchStep.data.changed ? t('deployer.runResult.changed') : t('deployer.runResult.unchanged') }}
                </span>
              </div>
              <div v-if="fetchStep.data.device_id" class="row">
                <span class="row__k">{{ t('deployer.common.deviceId') }}</span>
                <span class="row__v selectable">{{ fetchStep.data.device_id }}</span>
              </div>
              <div v-if="fetchStep.data.keybox_sha256" class="row">
                <span class="row__k">keybox</span>
                <span class="row__v mono">{{ shortHash(fetchStep.data.keybox_sha256) }}</span>
              </div>
              <div v-if="fetchStep.data.leaf_not_after" class="row">
                <span class="row__k">{{ t('deployer.runResult.leafValidity') }}</span>
                <span class="row__v">{{ leafLabel(fetchStep.data.leaf_not_after, now) }}</span>
              </div>
              <div v-if="fetchStep.data.gap" class="row">
                <span class="row__k">{{ t('deployer.runResult.chain') }}</span>
                <span class="row__v">{{ t('deployer.runResult.gap') }}</span>
              </div>
            </template>
            <template v-else-if="isTofu(fetchStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.firstFetch') }}</span>
                <span class="row__v">seq {{ fetchStep.data.seq }} · {{ providerLabel(fetchStep.data.provider) }}</span>
              </div>
              <div class="row">
                <span class="row__k">{{ t('deployer.common.deviceId') }}</span>
                <span class="row__v selectable">{{ fetchStep.data.device_id }}</span>
              </div>
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.leafValidity') }}</span>
                <span class="row__v">{{ leafLabel(fetchStep.data.leaf_not_after, now) }}</span>
              </div>
            </template>
          </template>

          <FoldBlock
            v-if="steps.fetch.stderr.trim()"
            :title="t('deployer.runResult.fetchStderr')"
            :text="steps.fetch.stderr"
          />
        </div>

        <MiuixSmallTitle :text="t('deployer.run.labelDeploy')" />
        <div class="section">
          <div class="row">
            <span class="row__k">{{ t('deployer.runResult.exitCode') }}</span>
            <StatusPill
              :tone="stepTone(deployStep, steps.deploy.code)"
              :label="stepLabel(deployStep, steps.deploy.code)"
            />
          </div>

          <template v-if="deployStep?.kind === 'not_run'">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.runResult.notRunPeriod') }}</MiuixText>
          </template>

          <template v-else-if="deployStep?.kind === 'abnormal'">
            <MiuixText v-if="deployStep.code !== 0" type="body2" color="var(--m-color-error)">
              {{ t('deployer.runResult.stepAbnormal', { code: deployStep.code }) }}
            </MiuixText>
            <MiuixText v-else type="body2" color="var(--m-color-error)">
              {{ t('deployer.runResult.stepNoJson') }}
            </MiuixText>
          </template>

          <template v-else-if="deployStep?.kind === 'error'">
            <MiuixText type="body2" color="var(--m-color-error)">
              {{ deployErrorText(deployStep.error) }}
            </MiuixText>
            <template v-if="isDeployReport(deployStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.result') }}</span>
                <span class="row__v">
                  {{
                    deployStep.data.pending
                      ? t('deployer.runResult.pendingNotWritten')
                      : deployStep.data.deployed
                        ? t('deployer.runResult.writtenToTarget')
                        : t('deployer.runResult.notWritten')
                  }}
                </span>
              </div>
              <div
                v-for="tg in deployStep.data.targets"
                :key="tg.id"
                class="target"
              >
                <div class="target__head">
                  <span>{{ targetLabel(tg.id) }}</span>
                  <StatusPill :tone="tg.ok ? 'ok' : 'error'" :label="tg.ok ? t('deployer.common.ok') : t('deployer.common.failed')" />
                </div>
                <span class="target__meta">
                  reload {{ tg.reload }}{{ tg.reload_status ? ` → ${tg.reload_status}` : '' }}
                </span>
                <span v-if="tg.error" class="target__error">
                  {{ t('deployer.deploySheet.targetError', { label: deployCodeLabel(tg.error.code), code: tg.error.code, message: tg.error.message }) }}
                </span>
              </div>
            </template>
            <template v-else-if="isDeployPlan(deployStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.plan') }}</span>
                <span class="row__v">{{ gateTitle(deployStep.data) }} · seq {{ deployStep.data.staged.seq }}</span>
              </div>
            </template>
          </template>

          <template v-else-if="deployStep?.kind === 'ok'">
            <template v-if="isDeploySkipped(deployStep.data)">
              <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
                {{ skipLabel(deployStep.data) }}
              </MiuixText>
            </template>

            <template v-else-if="isDeployReport(deployStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.result') }}</span>
                <span class="row__v">
                  {{
                    deployStep.data.pending
                      ? t('deployer.runResult.pendingNotWritten')
                      : deployStep.data.deployed
                        ? t('deployer.runResult.writtenToTarget')
                        : t('deployer.runResult.notWritten')
                  }}
                </span>
              </div>
              <div v-if="deployStep.data.pending" class="row">
                <span class="row__k">{{ t('deployer.runResult.pending') }}</span>
                <span class="row__v">
                  {{ t('deployer.runResult.pendingValue', { seq: deployStep.data.pending.seq, reason: deployStep.data.pending.reason }) }}
                </span>
              </div>
              <div class="row">
                <span class="row__k">keybox</span>
                <span class="row__v mono">{{ shortHash(deployStep.data.keybox_sha256) }}</span>
              </div>
              <div v-for="tg in deployStep.data.targets" :key="tg.id" class="target">
                <div class="target__head">
                  <span>{{ targetLabel(tg.id) }}</span>
                  <StatusPill :tone="tg.ok ? 'ok' : 'error'" :label="tg.ok ? t('deployer.common.ok') : t('deployer.common.failed')" />
                </div>
                <span class="target__meta">
                  reload {{ tg.reload }}{{ tg.reload_status ? ` → ${tg.reload_status}` : '' }}
                </span>
                <span v-if="tg.error" class="target__error">
                  {{ t('deployer.deploySheet.targetError', { label: deployCodeLabel(tg.error.code), code: tg.error.code, message: tg.error.message }) }}
                </span>
              </div>
              <MiuixText v-if="deployStep.data.pending" type="footnote1" color="var(--app-warning)">
                {{ t('deployer.runResult.approveNote') }}
              </MiuixText>
            </template>

            <template v-else-if="isDeployPlan(deployStep.data)">
              <div class="row">
                <span class="row__k">{{ t('deployer.runResult.plan') }}</span>
                <span class="row__v">{{ gateTitle(deployStep.data) }} · seq {{ deployStep.data.staged.seq }}</span>
              </div>
              <div v-for="w in deployStep.data.warnings" :key="w.code" class="row">
                <span class="row__k">{{ t('deployer.runResult.warning') }}</span>
                <span class="row__v">{{ warningLabel(w.code) ?? w.message }}</span>
              </div>
              <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
                {{ t('deployer.runResult.planNote') }}
              </MiuixText>
            </template>
          </template>

          <FoldBlock
            v-if="steps.deploy.stderr.trim()"
            :title="t('deployer.runResult.deployStderr')"
            :text="steps.deploy.stderr"
          />
        </div>

        <FoldBlock
          v-if="steps.stderr.trim()"
          :title="t('deployer.runResult.orchestrationStderr')"
          :text="steps.stderr"
        />

        <MiuixButton @click="open = false">{{ t('shared.common.close') }}</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.outcome {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.section {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.row {
  display: flex;
  gap: 16px;
  justify-content: space-between;
  font-size: var(--m-text-body2-size);
}
.row__k {
  color: var(--m-color-on-surface-variant-summary);
  flex: none;
}
.row__v {
  text-align: right;
  word-break: break-all;
}
.mono {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
}
.target {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-top: 6px;
  border-top: 1px solid color-mix(in srgb, var(--m-color-on-surface) 8%, transparent);
}
.target__head {
  display: flex;
  justify-content: space-between;
  gap: 10px;
}
.target__meta {
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
}
.target__error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
  word-break: break-word;
}
</style>
