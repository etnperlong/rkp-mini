<script setup lang="ts">
// Receiver status card (§19.5): enablement/risk, provider + location, verified
// sequence, staged and deployed keyboxes, pending gates, per-target state,
// loop and secret presence. Never renders keybox or secret contents.
import { computed } from 'vue'
import { MiuixButton, MiuixCard, MiuixProgressIndicator, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import StatusPill from '../../../shared/components/StatusPill.vue'
import {
  deployCodeLabel,
  leafLabel,
  pendingDetail,
  pendingTitle,
  providerLabel,
  secretText,
  secretTone,
  shortHash,
  stamp,
  targetText,
  targetTone,
} from '../lib/display'
import {
  TARGET_LABEL,
  type SecretKind,
  type SyncConfig,
  type SyncStatus,
  type TargetState,
} from '../../../shared/lib/sync'

const props = defineProps<{
  status: SyncStatus | null
  config: SyncConfig | null
  loading: boolean
  error: string | null
  notInitialized: boolean
  busy: boolean
  now: number
}>()

const emit = defineEmits<{
  import: []
  risk: []
  deploy: []
  retry: []
  secret: [kind: SecretKind]
}>()

const stream = computed(() => props.status?.stream_id ?? '')
const syncState = computed(() => props.status?.sync_state ?? null)
const deployState = computed(() => props.status?.deploy_state ?? null)
const staged = computed(() => props.status?.staged ?? null)
const pending = computed(() => deployState.value?.pending ?? null)
const riskOutdated = computed(() => !!props.status && !props.status.risk_ack_current)
const disabled = computed(() => !!props.status && !props.status.enabled)
const needsImport = computed(() => props.notInitialized || (!!props.status && !stream.value))

const tone = computed(() => {
  if (needsImport.value) return 'neutral'
  if (!props.status) return 'neutral'
  if (disabled.value) return 'warning'
  if (riskOutdated.value) return 'error'
  return 'ok'
})
const pill = computed(() => {
  if (needsImport.value) return '未导入'
  if (!props.status) return props.loading ? '读取中' : '读取失败'
  if (disabled.value) return '已禁用'
  if (riskOutdated.value) return '需重新确认'
  return '已启用'
})

const configuredTargets = computed(() => {
  const targets = props.status?.targets
  return Array.isArray(targets) ? targets : []
})
const targetConfigError = computed(() => {
  const targets = props.status?.targets
  return targets && !Array.isArray(targets) ? targets.error : null
})

const targetRows = computed(() => {
  const states = new Map<string, TargetState>((deployState.value?.targets ?? []).map((t) => [t.id, t]))
  const ids = new Set<string>([
    ...configuredTargets.value.map((t) => t.id),
    ...(deployState.value?.targets ?? []).map((t) => t.id),
  ])
  return [...ids].map((id) => {
    const state = states.get(id)
    return {
      id,
      label: TARGET_LABEL[id] ?? id,
      path: state?.path ?? configuredTargets.value.find((t) => t.id === id)?.path ?? '—',
      state: state ?? null,
    }
  })
})

const webdavNeeded = computed(
  () => props.config?.provider === 'webdav' && !!props.config.webdav_user,
)

function errorLine(code: string, message: string): string {
  return `${deployCodeLabel(code)}（${code}）${message ? `：${message}` : ''}`
}
</script>

<template>
  <MiuixCard>
    <div class="card">
      <div class="card__head">
        <MiuixText type="title4" weight="medium">同步状态</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <StatusPill v-else :tone="tone" :label="pill" />
      </div>

      <MiuixText v-if="error && !status" type="body1" color="var(--m-color-error)">
        读取同步状态失败：{{ error }}
      </MiuixText>
      <div v-if="error && !status" class="retry">
        <MiuixButton :disabled="loading" @click="emit('retry')">重试</MiuixButton>
      </div>

      <template v-if="needsImport">
        <MiuixText type="title3">尚未导入同步链接</MiuixText>
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          导入发送端分享的 rkpsync:// 链接后，才能验证并部署远端 keybox。链接不含 age 口令。
        </MiuixText>
        <MiuixButton type="primary" @click="emit('import')">导入同步链接</MiuixButton>
      </template>

      <template v-else-if="status">
        <div v-if="disabled" class="banner banner--warn">
          <MiuixText type="body2" color="var(--app-warning)">
            同步已禁用：不会自动拉取或部署；已部署的 keybox 会保留。禁用不等于撤销。
          </MiuixText>
          <MiuixButton @click="emit('risk')">启用同步（需风险确认）</MiuixButton>
        </div>
        <div v-else-if="riskOutdated" class="banner banner--error">
          <MiuixText type="body2" color="var(--m-color-error)">
            风险说明已更新到 v{{ status.risk_version }}，需要重新确认；后台循环在下一次唤醒时会退出。
          </MiuixText>
          <MiuixButton @click="emit('risk')">重新确认风险说明</MiuixButton>
        </div>

        <div class="rows">
          <div class="row">
            <span class="row__k">存储方式</span>
            <span class="row__v">{{ providerLabel(status.provider) }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">位置</span>
            <span class="row__v selectable">{{ status.location ?? '未配置' }}</span>
          </div>
          <div class="row">
            <span class="row__k">标签</span>
            <span class="row__v selectable">{{ config?.label || '—' }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">同步流</span>
            <span class="row__v selectable">{{ stream }}</span>
          </div>
          <div class="row">
            <span class="row__k">后台循环</span>
            <StatusPill :tone="status.loop_running ? 'ok' : 'neutral'" :label="status.loop_running ? '运行中' : '未运行'" />
          </div>
        </div>

        <MiuixSmallTitle text="验证与暂存" />
        <div class="rows">
          <template v-if="syncState">
            <div class="row">
              <span class="row__k">已验证 seq</span>
              <span class="row__v">{{ syncState.verified_seq }}</span>
            </div>
            <div class="row">
              <span class="row__k">验证时间</span>
              <span class="row__v">{{ stamp(syncState.verified_at) }}</span>
            </div>
            <div v-if="syncState.device_id" class="row">
              <span class="row__k">设备 ID</span>
              <span class="row__v selectable">{{ syncState.device_id }}</span>
            </div>
            <div v-if="syncState.keybox_not_after" class="row">
              <span class="row__k">已验证 keybox 到期</span>
              <span class="row__v">{{ leafLabel(syncState.keybox_not_after, now) }}</span>
            </div>
            <div v-if="syncState.last_error" class="row row--stack">
              <span class="row__k">上次错误（{{ stamp(syncState.last_error.at) }}）</span>
              <span class="row__v row__v--error">
                {{ errorLine(syncState.last_error.code, syncState.last_error.message) }}
              </span>
            </div>
          </template>
          <div v-else class="row">
            <span class="row__k">已验证版本</span>
            <span class="row__v">尚未验证任何版本</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">待部署 keybox</span>
            <template v-if="staged">
              <span class="row__v">seq {{ staged.seq }} · {{ shortHash(staged.keybox_sha256) }}</span>
              <span class="row__v row__v--sub">{{ staged.device_id }} · 到期 {{ leafLabel(staged.leaf_not_after, now) }}</span>
            </template>
            <span v-else class="row__v">没有暂存的 keybox</span>
          </div>
        </div>

        <div v-if="pending" class="banner banner--warn">
          <MiuixText type="body2" weight="medium" color="var(--app-warning)">
            {{ pendingTitle(pending.reason) }} · seq {{ pending.seq }}
          </MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            {{ pendingDetail(pending.reason) }}
          </MiuixText>
          <MiuixButton @click="emit('deploy')">查看部署计划</MiuixButton>
        </div>

        <MiuixSmallTitle text="已部署" />
        <div class="rows">
          <template v-if="deployState?.deployed_seq">
            <div class="row">
              <span class="row__k">seq</span>
              <span class="row__v">{{ deployState.deployed_seq }}</span>
            </div>
            <div class="row row--stack">
              <span class="row__k">keybox</span>
              <span class="row__v">{{ shortHash(deployState.deployed_keybox_sha256) }}</span>
            </div>
            <div class="row">
              <span class="row__k">部署时间</span>
              <span class="row__v">{{ stamp(deployState.deployed_at) }}</span>
            </div>
            <div class="row">
              <span class="row__k">到期</span>
              <span class="row__v">{{ leafLabel(deployState.deployed_leaf_not_after, now) }}</span>
            </div>
          </template>
          <div v-else class="row">
            <span class="row__k">已部署 keybox</span>
            <span class="row__v">尚未部署</span>
          </div>
        </div>

        <MiuixSmallTitle text="部署目标" />
        <div v-if="targetConfigError" class="banner banner--error">
          <MiuixText type="footnote1" color="var(--m-color-error)">目标配置错误：{{ targetConfigError }}</MiuixText>
        </div>
        <div v-else-if="!targetRows.length" class="empty">尚未配置部署目标（部署不会写入任何位置）。</div>
        <div v-else class="targets">
          <div v-for="t in targetRows" :key="t.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ t.label }}</span>
              <StatusPill :tone="targetTone(t.state ?? undefined)" :label="targetText(t.state ?? undefined)" />
            </div>
            <span class="target__path selectable">{{ t.path }}</span>
            <span v-if="t.state?.reload_status" class="target__meta">reload {{ t.state.reload }} → {{ t.state.reload_status }}</span>
            <span v-if="t.state?.last_error" class="target__error">
              {{ errorLine(t.state.last_error.code, t.state.last_error.message) }}
            </span>
          </div>
        </div>

        <MiuixSmallTitle text="密钥" />
        <div class="secrets">
          <div class="secret">
            <div class="secret__head">
              <span class="target__name">age 口令</span>
              <StatusPill :tone="secretTone(status.secrets.age)" :label="secretText(status.secrets.age)" />
            </div>
            <MiuixText v-if="status.password_mode === 'prompt'" type="footnote1" color="var(--app-warning)">
              口令模式为 prompt：自动运行不可用，每次手动拉取都需要输入口令。
            </MiuixText>
            <MiuixButton
              v-if="status.password_mode === 'secret-file'"
              :disabled="busy"
              @click="emit('secret', 'age')"
            >
              {{ status.secrets.age?.present ? '更换 age 口令' : '设置 age 口令' }}
            </MiuixButton>
          </div>
          <div v-if="webdavNeeded" class="secret">
            <div class="secret__head">
              <span class="target__name">WebDAV 应用密码</span>
              <StatusPill :tone="secretTone(status.secrets.webdav_password)" :label="secretText(status.secrets.webdav_password)" />
            </div>
            <MiuixButton :disabled="busy" @click="emit('secret', 'webdav-password')">
              {{ status.secrets.webdav_password?.present ? '更换 WebDAV 密码' : '设置 WebDAV 密码' }}
            </MiuixButton>
          </div>
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          跨设备使用等于 keybox 克隆/冒充，不是合法的 RKP provisioning；只做了结构与密钥匹配校验，未验证证书签名、根信任与吊销。
        </MiuixText>
      </template>
    </div>
  </MiuixCard>
</template>

<style scoped>
.card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px;
}
.card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  gap: 12px;
}
.card :deep(.m-small-title) {
  padding: 6px 0 0;
}
.rows {
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  overflow: hidden;
}
.row {
  display: flex;
  gap: 16px;
  justify-content: space-between;
  align-items: center;
  padding: 9px 14px;
  font-size: var(--m-text-body2-size);
  border-top: 1px solid color-mix(in srgb, var(--m-color-on-surface) 6%, transparent);
}
.row:first-child {
  border-top: none;
}
.row--stack {
  flex-direction: column;
  align-items: stretch;
  gap: 3px;
}
.row__k {
  flex: none;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  text-align: right;
  word-break: break-all;
}
.row--stack .row__v {
  text-align: start;
}
.row__v--sub {
  color: var(--m-color-on-surface-variant-actions);
  font-size: var(--m-text-footnote1-size);
}
.row__v--error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
}
.banner {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.banner--warn {
  background: color-mix(in srgb, var(--app-warning) 14%, transparent);
}
.banner--error {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
.empty {
  padding: 10px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
}
.targets,
.secrets {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.target,
.secret {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.target__head,
.secret__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.target__name {
  font-weight: 500;
}
.target__path {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  color: var(--m-color-on-surface-variant-summary);
}
.target__meta {
  color: var(--m-color-on-surface-variant-actions);
  font-size: var(--m-text-footnote1-size);
}
.target__error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
  word-break: break-word;
}
.retry {
  display: flex;
}
</style>
