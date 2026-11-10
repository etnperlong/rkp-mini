<script setup lang="ts">
// Result of the `run` task (fetch + deploy). Each half keeps its own error:
// the deploy half may have failed while the fetch half already staged a newer
// keybox, and pending approvals are not failures.
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
import StatusPill from '../components/StatusPill.vue'
import { leafLabel, providerLabel, shortHash, stamp } from './display'
import { nowUnix } from '../lib/format'
import {
  TARGET_LABEL,
  syncCodeLabel,
  syncErrorText,
  type DeployPlan,
  type DeployReport,
  type FetchResult,
  type RunResult,
  type TofuFacts,
} from '../lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ result: RunResult | null }>()

const now = ref(nowUnix())

watch(open, (v) => {
  if (v) now.value = nowUnix()
})

const fetchHalf = computed(() => props.result?.fetch ?? null)
const fetchError = computed(() => props.result?.fetch_error ?? null)
const fetchResult = computed(() => {
  const half = fetchHalf.value
  return half && 'changed' in half ? (half as FetchResult) : null
})
const fetchFacts = computed(() => {
  const half = fetchHalf.value
  return half && !('changed' in half) ? (half as TofuFacts) : null
})

const deployHalf = computed(() => props.result?.deploy ?? null)
const deployError = computed(() => props.result?.deploy_error ?? null)
const deployPlan = computed(() => {
  const half = deployHalf.value
  return half && 'gate' in half ? (half as DeployPlan) : null
})
const deployReport = computed(() => {
  const half = deployHalf.value
  return half && 'deployed' in half && 'targets' in half ? (half as DeployReport) : null
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="立即运行结果">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        protocol {{ result?.protocol ?? '—' }} · 拉取与部署分别记录错误；部署被策略拦下不算失败。
      </MiuixText>

      <MiuixSmallTitle text="拉取" />
      <div class="section">
        <MiuixText v-if="fetchError" type="body2" color="var(--m-color-error)">
          {{ syncErrorText(fetchError) }}
        </MiuixText>
        <MiuixText v-if="fetchError?.code === 'first_import_needs_confirmation'" type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          首次拉取需要在“拉取”中核对 TOFU 事实并确认；本次没有暂存或部署任何内容。
        </MiuixText>

        <template v-if="fetchResult">
          <div class="row">
            <span class="row__k">版本</span>
            <span class="row__v">seq {{ fetchResult.seq }} · {{ fetchResult.changed ? '有更新' : '无变化' }}</span>
          </div>
          <div v-if="fetchResult.device_id" class="row">
            <span class="row__k">设备 ID</span>
            <span class="row__v selectable">{{ fetchResult.device_id }}</span>
          </div>
          <div v-if="fetchResult.keybox_sha256" class="row">
            <span class="row__k">keybox</span>
            <span class="row__v mono">{{ shortHash(fetchResult.keybox_sha256) }}</span>
          </div>
          <div v-if="fetchResult.leaf_not_after" class="row">
            <span class="row__k">叶子有效期</span>
            <span class="row__v">{{ leafLabel(fetchResult.leaf_not_after, now) }}</span>
          </div>
          <div v-if="fetchResult.gap" class="row">
            <span class="row__k">版本链</span>
            <span class="row__v">跳过了中间版本（gap）</span>
          </div>
        </template>

        <template v-else-if="fetchFacts">
          <div class="row">
            <span class="row__k">首次拉取</span>
            <span class="row__v">seq {{ fetchFacts.seq }} · {{ providerLabel(fetchFacts.provider) }}</span>
          </div>
          <div class="row">
            <span class="row__k">设备 ID</span>
            <span class="row__v selectable">{{ fetchFacts.device_id }}</span>
          </div>
          <div class="row">
            <span class="row__k">签发时间</span>
            <span class="row__v">{{ stamp(fetchFacts.issued_at) }}</span>
          </div>
          <div class="row">
            <span class="row__k">叶子有效期</span>
            <span class="row__v">{{ leafLabel(fetchFacts.leaf_not_after, now) }}</span>
          </div>
        </template>

        <MiuixText
          v-else-if="!fetchError"
          type="footnote1"
          color="var(--m-color-on-surface-variant-summary)"
        >
          未执行拉取（可能已禁用自动部署或没有可部署内容）。
        </MiuixText>
      </div>

      <MiuixSmallTitle text="部署" />
      <div class="section">
        <MiuixText v-if="deployError" type="body2" color="var(--m-color-error)">
          {{ syncErrorText(deployError) }}
        </MiuixText>

        <template v-if="deployReport">
          <div class="row">
            <span class="row__k">结果</span>
            <span class="row__v">
              {{ deployReport.pending ? '待确认，未写入' : deployReport.deployed ? '已写入目标' : '未写入' }}
            </span>
          </div>
          <div v-if="deployReport.pending" class="row">
            <span class="row__k">待确认</span>
            <span class="row__v">seq {{ deployReport.pending.seq }}（{{ deployReport.pending.reason }}）</span>
          </div>
          <div class="row">
            <span class="row__k">keybox</span>
            <span class="row__v mono">{{ shortHash(deployReport.keybox_sha256) }}</span>
          </div>
          <div v-for="t in deployReport.targets" :key="t.id" class="target">
            <div class="target__head">
              <span>{{ TARGET_LABEL[t.id] ?? t.id }}</span>
              <StatusPill :tone="t.ok ? 'ok' : 'error'" :label="t.ok ? '成功' : '失败'" />
            </div>
            <span class="target__meta">
              reload {{ t.reload }}{{ t.reload_status ? ` → ${t.reload_status}` : '' }}
            </span>
            <span v-if="t.error" class="target__error">
              {{ syncCodeLabel(t.error.code) }}（{{ t.error.code }}）：{{ t.error.message }}
            </span>
          </div>
          <MiuixText v-if="deployReport.pending" type="footnote1" color="var(--app-warning)">
            在“部署”中查看计划并明确批准后才会写入。
          </MiuixText>
        </template>

        <template v-else-if="deployPlan">
          <div class="row">
            <span class="row__k">计划</span>
            <span class="row__v">{{ deployPlan.gate.kind }} · seq {{ deployPlan.staged.seq }}</span>
          </div>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            未执行写入；请在“部署”中核对计划与警告。
          </MiuixText>
        </template>

        <MiuixText
          v-else-if="!deployError"
          type="footnote1"
          color="var(--m-color-on-surface-variant-summary)"
        >
          未执行部署（可能没有 staged keybox 或 auto_deploy 已关闭）。
        </MiuixText>
      </div>

      <MiuixButton @click="open = false">关闭</MiuixButton>
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
