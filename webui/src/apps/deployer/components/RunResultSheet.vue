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
  TARGET_LABEL,
  WARNING_LABEL,
  syncErrorText,
  type DeployPlan,
  type DeployReport,
  type SyncEnvelope,
  type SyncErrorInfo,
  type TofuFacts,
} from '../../../shared/lib/sync'
import type { DeployOutcome, DeploySkip, RunSteps, StepResult } from '../lib/run'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ steps: RunSteps | null }>()

const now = ref(nowUnix())

watch(open, (v) => {
  if (v) now.value = nowUnix()
})

const SKIP_LABEL: Record<DeploySkip, string> = {
  nothing_staged: '没有已验证的 keybox，未部署',
  auto_deploy_false: '自动部署已关闭（auto_deploy=false），未部署',
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

function isDeployReport(value: unknown): value is DeployReport {
  return typeof value === 'object' && value !== null && 'deployed' in value && 'targets' in value
}

function isDeployPlan(value: unknown): value is DeployPlan {
  return typeof value === 'object' && value !== null && 'gate' in value && 'staged' in value
}

function isSkipped(value: unknown): value is { skipped: DeploySkip } {
  return typeof value === 'object' && value !== null && 'skipped' in value
}

function stepTone(view: StepView<unknown> | null, code: number | null): PillTone {
  if (!view || view.kind === 'not_run') return 'neutral'
  if (view.kind === 'abnormal') return 'error'
  return code === 0 ? 'ok' : 'error'
}

function stepLabel(view: StepView<unknown> | null, code: number | null): string {
  if (!view || view.kind === 'not_run') return '未执行'
  if (view.kind === 'abnormal') {
    return code === 0 ? '无结果（exit 0）' : `异常退出（exit ${code}）`
  }
  return code === 0 ? '成功（exit 0）' : `失败（exit ${code}）`
}

function skipLabel(value: { skipped: DeploySkip }): string {
  return SKIP_LABEL[value.skipped] ?? `未部署（${value.skipped}）`
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
  <MiuixBottomSheet v-model="open" title="最近一次组合结果">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <template v-if="!steps">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          还没有可读取的组合任务结果。
        </MiuixText>
      </template>

      <template v-else>
        <div class="row">
          <span class="row__k">总体结果</span>
          <StatusPill
            :tone="overallOk ? 'ok' : 'error'"
            :label="overallOk ? '成功（exit 0）' : `失败（exit ${steps.overall}）`"
          />
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          拉取与部署分别记录退出码；部署被策略跳过不算失败。
        </MiuixText>

        <MiuixSmallTitle text="拉取" />
        <div class="section">
          <div class="row">
            <span class="row__k">退出码</span>
            <StatusPill
              :tone="stepTone(fetchStep, steps.fetch.code)"
              :label="stepLabel(fetchStep, steps.fetch.code)"
            />
          </div>

          <template v-if="fetchStep?.kind === 'not_run'">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">未执行。</MiuixText>
          </template>

          <template v-else-if="fetchStep?.kind === 'abnormal'">
            <MiuixText v-if="fetchStep.code !== 0" type="body2" color="var(--m-color-error)">
              该步异常退出（exit {{ fetchStep.code }}），没有 JSON 结果。
            </MiuixText>
            <MiuixText v-else type="body2" color="var(--m-color-error)">
              该步返回 0 但没有 JSON 结果，不能视为成功。
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
              首次拉取需要核对 TOFU 事实并在“拉取”中确认；本次没有暂存或部署任何内容。
            </MiuixText>
            <template v-if="tofuFromFetch">
              <div class="row">
                <span class="row__k">首次拉取</span>
                <span class="row__v">seq {{ tofuFromFetch.seq }} · {{ providerLabel(tofuFromFetch.provider) }}</span>
              </div>
              <div class="row">
                <span class="row__k">设备 ID</span>
                <span class="row__v selectable">{{ tofuFromFetch.device_id }}</span>
              </div>
              <div class="row">
                <span class="row__k">签发时间</span>
                <span class="row__v">{{ stamp(tofuFromFetch.issued_at) }}</span>
              </div>
              <div class="row">
                <span class="row__k">叶子有效期</span>
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
                <span class="row__k">版本</span>
                <span class="row__v">
                  seq {{ fetchStep.data.seq }} · {{ fetchStep.data.changed ? '有更新' : '无变化' }}
                </span>
              </div>
              <div v-if="fetchStep.data.device_id" class="row">
                <span class="row__k">设备 ID</span>
                <span class="row__v selectable">{{ fetchStep.data.device_id }}</span>
              </div>
              <div v-if="fetchStep.data.keybox_sha256" class="row">
                <span class="row__k">keybox</span>
                <span class="row__v mono">{{ shortHash(fetchStep.data.keybox_sha256) }}</span>
              </div>
              <div v-if="fetchStep.data.leaf_not_after" class="row">
                <span class="row__k">叶子有效期</span>
                <span class="row__v">{{ leafLabel(fetchStep.data.leaf_not_after, now) }}</span>
              </div>
              <div v-if="fetchStep.data.gap" class="row">
                <span class="row__k">版本链</span>
                <span class="row__v">跳过了中间版本（gap）</span>
              </div>
            </template>
            <template v-else-if="isTofu(fetchStep.data)">
              <div class="row">
                <span class="row__k">首次拉取</span>
                <span class="row__v">seq {{ fetchStep.data.seq }} · {{ providerLabel(fetchStep.data.provider) }}</span>
              </div>
              <div class="row">
                <span class="row__k">设备 ID</span>
                <span class="row__v selectable">{{ fetchStep.data.device_id }}</span>
              </div>
              <div class="row">
                <span class="row__k">叶子有效期</span>
                <span class="row__v">{{ leafLabel(fetchStep.data.leaf_not_after, now) }}</span>
              </div>
            </template>
          </template>

          <FoldBlock
            v-if="steps.fetch.stderr.trim()"
            title="拉取错误输出"
            :text="steps.fetch.stderr"
          />
        </div>

        <MiuixSmallTitle text="部署" />
        <div class="section">
          <div class="row">
            <span class="row__k">退出码</span>
            <StatusPill
              :tone="stepTone(deployStep, steps.deploy.code)"
              :label="stepLabel(deployStep, steps.deploy.code)"
            />
          </div>

          <template v-if="deployStep?.kind === 'not_run'">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">未执行。</MiuixText>
          </template>

          <template v-else-if="deployStep?.kind === 'abnormal'">
            <MiuixText v-if="deployStep.code !== 0" type="body2" color="var(--m-color-error)">
              该步异常退出（exit {{ deployStep.code }}），没有 JSON 结果。
            </MiuixText>
            <MiuixText v-else type="body2" color="var(--m-color-error)">
              该步返回 0 但没有 JSON 结果，不能视为成功。
            </MiuixText>
          </template>

          <template v-else-if="deployStep?.kind === 'error'">
            <MiuixText type="body2" color="var(--m-color-error)">
              {{ deployErrorText(deployStep.error) }}
            </MiuixText>
            <template v-if="isDeployReport(deployStep.data)">
              <div class="row">
                <span class="row__k">结果</span>
                <span class="row__v">
                  {{ deployStep.data.pending ? '待确认，未写入' : deployStep.data.deployed ? '已写入目标' : '未写入' }}
                </span>
              </div>
              <div
                v-for="t in deployStep.data.targets"
                :key="t.id"
                class="target"
              >
                <div class="target__head">
                  <span>{{ TARGET_LABEL[t.id] ?? t.id }}</span>
                  <StatusPill :tone="t.ok ? 'ok' : 'error'" :label="t.ok ? '成功' : '失败'" />
                </div>
                <span class="target__meta">
                  reload {{ t.reload }}{{ t.reload_status ? ` → ${t.reload_status}` : '' }}
                </span>
                <span v-if="t.error" class="target__error">
                  {{ deployCodeLabel(t.error.code) }}（{{ t.error.code }}）：{{ t.error.message }}
                </span>
              </div>
            </template>
            <template v-else-if="isDeployPlan(deployStep.data)">
              <div class="row">
                <span class="row__k">计划</span>
                <span class="row__v">{{ gateTitle(deployStep.data) }} · seq {{ deployStep.data.staged.seq }}</span>
              </div>
            </template>
          </template>

          <template v-else-if="deployStep?.kind === 'ok'">
            <template v-if="isSkipped(deployStep.data)">
              <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
                {{ skipLabel(deployStep.data) }}
              </MiuixText>
            </template>

            <template v-else-if="isDeployReport(deployStep.data)">
              <div class="row">
                <span class="row__k">结果</span>
                <span class="row__v">
                  {{ deployStep.data.pending ? '待确认，未写入' : deployStep.data.deployed ? '已写入目标' : '未写入' }}
                </span>
              </div>
              <div v-if="deployStep.data.pending" class="row">
                <span class="row__k">待确认</span>
                <span class="row__v">
                  seq {{ deployStep.data.pending.seq }}（{{ deployStep.data.pending.reason }}）
                </span>
              </div>
              <div class="row">
                <span class="row__k">keybox</span>
                <span class="row__v mono">{{ shortHash(deployStep.data.keybox_sha256) }}</span>
              </div>
              <div v-for="t in deployStep.data.targets" :key="t.id" class="target">
                <div class="target__head">
                  <span>{{ TARGET_LABEL[t.id] ?? t.id }}</span>
                  <StatusPill :tone="t.ok ? 'ok' : 'error'" :label="t.ok ? '成功' : '失败'" />
                </div>
                <span class="target__meta">
                  reload {{ t.reload }}{{ t.reload_status ? ` → ${t.reload_status}` : '' }}
                </span>
                <span v-if="t.error" class="target__error">
                  {{ deployCodeLabel(t.error.code) }}（{{ t.error.code }}）：{{ t.error.message }}
                </span>
              </div>
              <MiuixText v-if="deployStep.data.pending" type="footnote1" color="var(--app-warning)">
                在“部署”中查看计划并明确批准后才会写入。
              </MiuixText>
            </template>

            <template v-else-if="isDeployPlan(deployStep.data)">
              <div class="row">
                <span class="row__k">计划</span>
                <span class="row__v">{{ gateTitle(deployStep.data) }} · seq {{ deployStep.data.staged.seq }}</span>
              </div>
              <div v-for="w in deployStep.data.warnings" :key="w.code" class="row">
                <span class="row__k">警告</span>
                <span class="row__v">{{ WARNING_LABEL[w.code] ?? w.message }}</span>
              </div>
              <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
                未执行写入；请在“部署”中核对计划与警告。
              </MiuixText>
            </template>
          </template>

          <FoldBlock
            v-if="steps.deploy.stderr.trim()"
            title="部署错误输出"
            :text="steps.deploy.stderr"
          />
        </div>

        <FoldBlock
          v-if="steps.stderr.trim()"
          title="编排诊断"
          :text="steps.stderr"
        />

        <MiuixButton @click="open = false">关闭</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
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
