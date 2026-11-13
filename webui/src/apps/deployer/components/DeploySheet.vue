<script setup lang="ts">
// Deployment review (§11.5, §19.3, §19.5). Loads `deploy plan` (read-only),
// shows the identity gate, warnings and local targets, then sends an approval
// bound to exactly the reviewed staged keybox SHA-256.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import StatusPill from '../../../shared/components/StatusPill.vue'
import {
  deployCodeLabel,
  deployErrorText,
  errorMessage,
  gateDetail,
  gateTitle,
  gateTone,
  leafLabel,
  shortHash,
  stamp,
} from '../lib/display'
import { deployApply, readDeployPlan, reportWrote } from '../lib/deploy'
import { nowUnix } from '../../../shared/lib/format'
import { t } from '../../../shared/lib/i18n'
import {
  targetLabel,
  warningLabel,
  type DeployPlan,
  type DeployReport,
  type PlanWarning,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const emit = defineEmits<{ changed: [] }>()

const plan = ref<DeployPlan | null>(null)
const report = ref<DeployReport | null>(null)
const loading = ref(false)
const applying = ref(false)
const loadError = ref<string | null>(null)
const applyError = ref<string | null>(null)
const mismatch = ref(false)
const ackOmk = ref(false)

const now = ref(nowUnix())

watch(open, (v) => {
  if (!v) return
  now.value = nowUnix()
  void load()
})

async function fetchPlan(): Promise<void> {
  try {
    const call = await readDeployPlan()
    if (!call.envelope.ok) {
      loadError.value = deployErrorText(call.envelope.error)
      plan.value = null
    } else {
      plan.value = call.envelope.data
      loadError.value = null
    }
  } catch (e) {
    loadError.value = errorMessage(e)
    plan.value = null
  }
}

async function load(): Promise<void> {
  loading.value = true
  report.value = null
  loadError.value = null
  applyError.value = null
  mismatch.value = false
  ackOmk.value = false
  try {
    await fetchPlan()
  } finally {
    loading.value = false
  }
}

const gateKind = computed(() => plan.value?.gate.kind ?? null)
const needsOmkAck = computed(
  () => gateKind.value === 'blocked' && (plan.value?.targets.some((t) => t.id === 'omk') ?? false),
)
const canApply = computed(() => !!plan.value && !applying.value && (!needsOmkAck.value || ackOmk.value))

const actionLabel = computed(() => {
  if (applying.value) return t('deployer.deploySheet.applying')
  switch (gateKind.value) {
    case 'same':
      return t('deployer.deploySheet.actionSame')
    case 'first_deploy':
      return t('deployer.deploySheet.actionFirstDeploy')
    case 'allowed':
      return t('deployer.deploySheet.actionAllowed')
    case 'blocked':
      return t('deployer.deploySheet.actionBlocked')
    default:
      return t('deployer.deploySheet.actionDefault')
  }
})

const omkWarning = computed(() => plan.value?.warnings.find((w) => w.code === 'omk_key_retirement') ?? null)
/** OhMyKeymint would reject the staged keybox and overwrite it; the omk target is skipped. */
const omkIncompatible = computed(() => plan.value?.warnings.find((w) => w.code === 'omk_incompatible') ?? null)
const otherWarnings = computed(() =>
  (plan.value?.warnings ?? []).filter((w) => w.code !== 'omk_key_retirement' && w.code !== 'omk_incompatible'),
)

function warningText(w: PlanWarning): string {
  return warningLabel(w.code) ?? w.code
}

/** Honest report even when only some targets failed (`ok:false` + data). */
function asReport(value: unknown): DeployReport | null {
  if (typeof value !== 'object' || value === null) return null
  const report = value as Partial<DeployReport>
  return Array.isArray(report.targets) ? (value as DeployReport) : null
}

async function apply(): Promise<void> {
  const p = plan.value
  if (!p || !canApply.value) return
  const sha = p.staged.keybox_sha256
  applying.value = true
  applyError.value = null
  mismatch.value = false
  report.value = null
  try {
    const approval =
      p.gate.kind === 'first_deploy'
        ? { confirmFirst: sha }
        : p.gate.kind === 'blocked'
          ? { allowIdentityChange: sha }
          : {}
    const call = await deployApply(approval)
    if (call.envelope.ok) {
      const result = call.envelope.data
      report.value = result
      void showSnackbar({
        message: reportWrote(result)
          ? t('deployer.deploySheet.appliedReboot')
          : result.pending
            ? t('deployer.run.pending')
            : t('deployer.run.unchanged'),
      })
      emit('changed')
      return
    }
    if (call.envelope.error.code === 'confirmation_mismatch') {
      mismatch.value = true
      ackOmk.value = false
      await fetchPlan()
      return
    }
    const data = asReport(call.envelope.data)
    if (data) report.value = data
    applyError.value = deployErrorText(call.envelope.error)
    emit('changed')
  } catch (e) {
    applyError.value = errorMessage(e)
  } finally {
    applying.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('deployer.deploySheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>

      <template v-else-if="loadError">
        <MiuixText type="body1" color="var(--m-color-error)">{{ t('deployer.deploySheet.loadFailed', { error: loadError }) }}</MiuixText>
        <MiuixButton @click="load">{{ t('deployer.deploySheet.retry') }}</MiuixButton>
      </template>

      <template v-else-if="plan">
        <div v-if="mismatch" class="banner banner--error">
          <MiuixText type="body2" color="var(--m-color-error)">
            {{ t('deployer.deploySheet.mismatch') }}
          </MiuixText>
        </div>

        <div class="gate">
          <div class="gate__head">
            <MiuixText type="title4" weight="medium">{{ gateTitle(plan) }}</MiuixText>
            <StatusPill
              :tone="gateTone(plan)"
              :label="
                gateKind === 'same'
                  ? t('deployer.deploySheet.pillSame')
                  : gateKind === 'first_deploy'
                    ? t('deployer.deploySheet.pillFirstDeploy')
                    : gateKind === 'allowed'
                      ? t('deployer.deploySheet.pillAllowed')
                      : t('deployer.deploySheet.pillBlocked')
              "
            />
          </div>
          <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">{{ gateDetail(plan) }}</MiuixText>
        </div>

        <div class="transition">
          <div class="transition__side">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.deploySheet.deployedSide') }}</MiuixText>
            <span class="mono selectable">{{ shortHash(plan.old_keybox_sha256) }}</span>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              {{
                plan.old_keybox_sha256
                  ? t('deployer.deploySheet.validUntil', { date: leafLabel(plan.old_leaf_not_after, now) })
                  : t('deployer.deploySheet.noneDeployed')
              }}
            </MiuixText>
          </div>
          <span class="transition__arrow">→</span>
          <div class="transition__side">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.deploySheet.stagedSeq', { seq: plan.staged.seq }) }}</MiuixText>
            <span class="mono selectable">{{ shortHash(plan.staged.keybox_sha256) }}</span>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              {{ t('deployer.deploySheet.validUntil', { date: leafLabel(plan.staged.leaf_not_after, now) }) }}
            </MiuixText>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              {{ t('deployer.deploySheet.stagedMeta', { device: plan.staged.device_id, time: stamp(plan.staged.staged_at) }) }}
            </MiuixText>
          </div>
        </div>

        <div v-if="omkIncompatible" class="warn warn--danger">
          <MiuixText type="body2" weight="medium" color="var(--m-color-error)">{{ warningText(omkIncompatible) }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-error)">{{ omkIncompatible.message }}</MiuixText>
        </div>
        <div v-if="omkWarning" class="warn warn--danger">
          <MiuixText type="body2" weight="medium" color="var(--m-color-error)">{{ warningText(omkWarning) }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-error)">{{ omkWarning.message }}</MiuixText>
        </div>
        <div v-for="w in otherWarnings" :key="w.code" class="warn warn--warn">
          <MiuixText type="body2" weight="medium" color="var(--app-warning)">{{ warningText(w) }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ w.message }}</MiuixText>
        </div>

        <MiuixText type="title4" weight="medium">{{ t('deployer.deploySheet.targetsTitle') }}</MiuixText>
        <div v-if="!plan.targets.length" class="empty">{{ t('deployer.deploySheet.noTargets') }}</div>
        <div v-else class="targets">
          <div v-for="tg in plan.targets" :key="tg.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ targetLabel(tg.id) }}</span>
              <span class="target__meta">{{ tg.owner }} · {{ tg.mode }} · reload {{ tg.reload }}</span>
            </div>
            <span class="target__path selectable">{{ tg.path }}</span>
          </div>
        </div>

        <template v-if="needsOmkAck">
          <div class="sheet-group">
            <MiuixCheckboxPreference
              v-model="ackOmk"
              :title="t('deployer.deploySheet.omkAckTitle')"
              :summary="t('deployer.deploySheet.omkAckSummary')"
            />
          </div>
        </template>

        <div v-if="applyError" class="banner banner--error">
          <MiuixText type="body2" color="var(--m-color-error)">{{ applyError }}</MiuixText>
        </div>

        <MiuixButton type="primary" :disabled="!canApply" @click="apply">{{ actionLabel }}</MiuixButton>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.deploySheet.approvalNote') }}
        </MiuixText>

        <template v-if="report">
          <div class="divider"></div>
          <MiuixText type="title4" weight="medium">{{ t('deployer.deploySheet.resultTitle') }}</MiuixText>
          <div v-if="report.pending" class="banner banner--warn">
            <MiuixText type="body2" color="var(--app-warning)">
              {{
                t('deployer.deploySheet.resultPending', {
                  seq: report.pending.seq,
                  reason: report.pending.reason,
                })
              }}
            </MiuixText>
          </div>
          <MiuixText v-else type="body2" :color="report.deployed ? undefined : 'var(--m-color-error)'">
            {{ report.deployed ? t('deployer.deploySheet.written') : t('deployer.deploySheet.notWritten') }} · seq {{ report.seq }} ·
            <span class="mono">{{ shortHash(report.keybox_sha256) }}</span>
          </MiuixText>
          <div v-if="report.targets.length" class="targets">
            <div v-for="tg in report.targets" :key="tg.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ targetLabel(tg.id) }}</span>
                <StatusPill :tone="tg.ok ? 'ok' : 'error'" :label="tg.ok ? t('deployer.common.ok') : t('deployer.common.failed')" />
              </div>
              <span class="target__path selectable">{{ tg.path }}</span>
              <span class="target__meta">reload {{ tg.reload }}{{ tg.reload_status ? ` → ${tg.reload_status}` : '' }}</span>
              <span v-if="tg.error" class="target__error">
                {{ t('deployer.deploySheet.targetError', { label: deployCodeLabel(tg.error.code), code: tg.error.code, message: tg.error.message }) }}
              </span>
            </div>
          </div>
        </template>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.center {
  display: flex;
  justify-content: center;
  padding: 24px;
}
.gate {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.gate__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.transition {
  display: flex;
  align-items: stretch;
  gap: 10px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.transition__side {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.transition__arrow {
  align-self: center;
  color: var(--m-color-on-surface-variant-actions);
  font-size: 20px;
}
.mono {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  word-break: break-all;
}
.warn {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.warn--danger {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
.warn--warn {
  background: color-mix(in srgb, var(--app-warning) 14%, transparent);
}
.targets {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.target {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 10px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.target__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.target__name {
  font-weight: 500;
}
.target__meta {
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
}
.target__path {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  color: var(--m-color-on-surface-variant-summary);
}
.target__error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
  word-break: break-word;
}
.empty {
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-body2-size);
}
.banner {
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.banner--error {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
.banner--warn {
  background: color-mix(in srgb, var(--app-warning) 14%, transparent);
}
.divider {
  height: 1px;
  background: var(--m-color-secondary-variant);
  margin: 4px 0;
}
</style>
