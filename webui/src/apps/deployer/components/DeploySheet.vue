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
import { deployApply, readDeployPlan } from '../lib/deploy'
import { nowUnix } from '../../../shared/lib/format'
import {
  TARGET_LABEL,
  WARNING_LABEL,
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
  if (applying.value) return '部署中…'
  switch (gateKind.value) {
    case 'same':
      return '写入（幂等修复）'
    case 'first_deploy':
      return '确认首次部署'
    case 'allowed':
      return '部署新版本'
    case 'blocked':
      return '仍然部署新身份'
    default:
      return '部署'
  }
})

const omkWarning = computed(() => plan.value?.warnings.find((w) => w.code === 'omk_key_retirement') ?? null)
const otherWarnings = computed(() => (plan.value?.warnings ?? []).filter((w) => w.code !== 'omk_key_retirement'))

function warningLabel(w: PlanWarning): string {
  return WARNING_LABEL[w.code] ?? w.code
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
      report.value = call.envelope.data
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
  <MiuixBottomSheet v-model="open" title="部署验证后的 keybox">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>

      <template v-else-if="loadError">
        <MiuixText type="body1" color="var(--m-color-error)">无法读取部署计划：{{ loadError }}</MiuixText>
        <MiuixButton @click="load">重试</MiuixButton>
      </template>

      <template v-else-if="plan">
        <div v-if="mismatch" class="banner banner--error">
          <MiuixText type="body2" color="var(--m-color-error)">
            暂存的 keybox 在核对后发生了变化（confirmation_mismatch），本次没有写入。计划已重新读取，请重新核对后再确认。
          </MiuixText>
        </div>

        <div class="gate">
          <div class="gate__head">
            <MiuixText type="title4" weight="medium">{{ gateTitle(plan) }}</MiuixText>
            <StatusPill :tone="gateTone(plan)" :label="gateKind === 'same' ? '幂等' : gateKind === 'first_deploy' ? '首次' : gateKind === 'allowed' ? '可部署' : '需批准'" />
          </div>
          <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">{{ gateDetail(plan) }}</MiuixText>
        </div>

        <div class="transition">
          <div class="transition__side">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">已部署</MiuixText>
            <span class="mono selectable">{{ shortHash(plan.old_keybox_sha256) }}</span>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              {{ plan.old_keybox_sha256 ? `有效期至 ${leafLabel(plan.old_leaf_not_after, now)}` : '无（尚未部署）' }}
            </MiuixText>
          </div>
          <span class="transition__arrow">→</span>
          <div class="transition__side">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">待部署 seq {{ plan.staged.seq }}</MiuixText>
            <span class="mono selectable">{{ shortHash(plan.staged.keybox_sha256) }}</span>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              有效期至 {{ leafLabel(plan.staged.leaf_not_after, now) }}
            </MiuixText>
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
              {{ plan.staged.device_id }} · 暂存于 {{ stamp(plan.staged.staged_at) }}
            </MiuixText>
          </div>
        </div>

        <div v-if="omkWarning" class="warn warn--danger">
          <MiuixText type="body2" weight="medium" color="var(--m-color-error)">{{ warningLabel(omkWarning) }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-error)">{{ omkWarning.message }}</MiuixText>
        </div>
        <div v-for="w in otherWarnings" :key="w.code" class="warn warn--warn">
          <MiuixText type="body2" weight="medium" color="var(--app-warning)">{{ warningLabel(w) }}</MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ w.message }}</MiuixText>
        </div>

        <MiuixText type="title4" weight="medium">目标（只来自本地配置）</MiuixText>
        <div v-if="!plan.targets.length" class="empty">未配置部署目标；先在“部署目标”里添加。</div>
        <div v-else class="targets">
          <div v-for="t in plan.targets" :key="t.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ TARGET_LABEL[t.id] ?? t.id }}</span>
              <span class="target__meta">{{ t.owner }} · {{ t.mode }} · reload {{ t.reload }}</span>
            </div>
            <span class="target__path selectable">{{ t.path }}</span>
          </div>
        </div>

        <template v-if="needsOmkAck">
          <div class="sheet-group">
            <MiuixCheckboxPreference
              v-model="ackOmk"
              title="我了解 OMK 会删除绑定旧 keybox 的客户端密钥"
              summary="retire_stale_keybox_bound_keys（默认 true）会在 keybox 变化后让受影响应用重新生成密钥。"
            />
          </div>
        </template>

        <div v-if="applyError" class="banner banner--error">
          <MiuixText type="body2" color="var(--m-color-error)">{{ applyError }}</MiuixText>
        </div>

        <MiuixButton type="primary" :disabled="!canApply" @click="apply">{{ actionLabel }}</MiuixButton>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          批准值绑定到上面这份 staged keybox 的内容哈希；若后台循环在确认前换了版本，本次写入会被拒绝并重新显示计划。
        </MiuixText>

        <template v-if="report">
          <div class="divider"></div>
          <MiuixText type="title4" weight="medium">部署结果</MiuixText>
          <div v-if="report.pending" class="banner banner--warn">
            <MiuixText type="body2" color="var(--app-warning)">
              未部署：seq {{ report.pending.seq }} 被暂存为待确认（{{ report.pending.reason }}），没有写入任何目标。
            </MiuixText>
          </div>
          <MiuixText v-else type="body2" :color="report.deployed ? undefined : 'var(--m-color-error)'">
            {{ report.deployed ? '已写入' : '未写入任何目标' }} · seq {{ report.seq }} ·
            <span class="mono">{{ shortHash(report.keybox_sha256) }}</span>
          </MiuixText>
          <div v-if="report.targets.length" class="targets">
            <div v-for="t in report.targets" :key="t.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ TARGET_LABEL[t.id] ?? t.id }}</span>
                <StatusPill :tone="t.ok ? 'ok' : 'error'" :label="t.ok ? '成功' : '失败'" />
              </div>
              <span class="target__path selectable">{{ t.path }}</span>
              <span class="target__meta">reload {{ t.reload }}{{ t.reload_status ? ` → ${t.reload_status}` : '' }}</span>
              <span v-if="t.error" class="target__error">
                {{ deployCodeLabel(t.error.code) }}（{{ t.error.code }}）：{{ t.error.message }}
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
