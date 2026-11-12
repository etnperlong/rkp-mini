<script setup lang="ts">
// Remote sync (design §13.3, §19.5), issuer status-tab style. Everything shown
// comes from the local `rkp-mini-sync status` read together with its cached
// `check-remote` record: the page never contacts the remote on its own, and
// never reads or renders a keybox, passphrase, token or WebDAV password.
import { computed, ref } from 'vue'
import {
  MiuixButton,
  MiuixCard,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import CardStateButton from '../../../shared/components/CardStateButton.vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { KEYBOX_STATE_VISUAL } from '../lib/rkp'
import { formatAgo, formatElapsed } from '../../../shared/lib/format'
import { CommandError } from '../../../shared/lib/command'
import {
  CHECK_OUTCOME_LABEL,
  initSender,
  syncCodeLabel,
  syncErrorText,
  type PushResult,
  type RemoteCheck,
  type SecretStatus,
  type SyncOp,
  type SyncRunner,
  type SyncStatus,
} from '../../../shared/lib/sync'

const props = defineProps<{
  status: SyncStatus | null
  /** `status` failed with `not_initialized`: the `[sync]` table does not exist yet. */
  notInitialized: boolean
  loading: boolean
  /** Read failure with no usable status. */
  error: string | null
  runner: SyncRunner
  now: number
  /** Op started from this page; null when the task predates the page. */
  busyOp: SyncOp | null
  /** A check may start now (no task running, no stale lock). */
  canRefresh: boolean
}>()
const emit = defineEmits<{ detail: []; changed: []; refresh: [] }>()

const r = props.runner
const initBusy = ref<'gist' | 'webdav' | null>(null)
const initError = ref<string | null>(null)

const PROVIDER: Record<string, string> = { gist: 'GitHub Gist', webdav: 'WebDAV', url: '公开 HTTPS' }

type Tone = 'ok' | 'warning' | 'error' | 'neutral'
/** Card tint/icon per tone; reuses the issuer status states so both cards match. */
const TONE_VISUAL: Record<Tone, { color: string; icon: string }> = {
  ok: KEYBOX_STATE_VISUAL.valid,
  warning: KEYBOX_STATE_VISUAL.expiring,
  error: KEYBOX_STATE_VISUAL.expired,
  neutral: KEYBOX_STATE_VISUAL.missing,
}
const TONE_LABEL: Record<Tone, string> = { ok: '已同步', warning: '需处理', error: '异常', neutral: '未检查' }
const BUSY_LABEL: Record<SyncOp, string> = {
  check: '检查中',
  push: '推送中',
  delete: '删除中',
  rotate: '重置密码中',
  fetch: '拉取中',
  run: '同步中',
}

/** Header text while a task runs; replaces the tone label in the same corner. */
const busyText = computed(() => {
  const label = props.busyOp ? BUSY_LABEL[props.busyOp] : '同步中'
  return `${label} ${formatElapsed(r.elapsedMs.value)}${r.overdue.value ? ' · 结果未知' : ''}`
})

/** Why automatic push is off, or null when it runs. */
const autoPushOff = computed<string | null>(() => {
  const s = props.status
  if (!s) return null
  if (!s.enabled) return '同步未启用'
  if (!s.risk_ack_current) return '需重新确认风险'
  if (s.auto_push === false) return '已在设置中关闭'
  if (!s.loop_running) return '后台循环未运行'
  return null
})

const warnings = computed<string[]>(() => {
  const s = props.status
  if (!s) return []
  const out: string[] = []

  const check = s.remote_check
  if (check && check.outcome !== 'in_sync') {
    const detail =
      check.outcome === 'error' && check.error
        ? syncErrorText(check.error)
        : (CHECK_OUTCOME_LABEL[check.outcome] ?? check.outcome)
    out.push(`云端检查：${detail} · ${formatAgo(check.at, props.now)}`)
  }

  const secret = (value: SecretStatus | undefined, name: string): void => {
    if (!value?.present) out.push(`${name} 尚未设置`)
    else if (!value.ok) out.push(`${name} 文件权限不正确`)
  }
  if (s.password_mode === 'secret-file') secret(s.secrets.age, '密码')
  if (s.provider === 'gist') secret(s.secrets.gist_token, 'Gist token')
  else if (s.provider === 'webdav') secret(s.secrets.webdav_password, 'WebDAV 密码')

  if (!s.risk_ack_current) out.push('风险说明已更新，需要重新确认')
  const last = s.sync_state?.last_error
  if (last) out.push(`上次同步失败：${syncCodeLabel(last.code)} · ${formatAgo(last.at, props.now)}`)
  return out
})

const tone = computed<Tone>(() => {
  const s = props.status
  if (!s) return 'neutral'
  if (s.remote_check?.outcome === 'error' || s.sync_state?.last_error) return 'error'
  if (warnings.value.length) return 'warning'
  if (s.remote_check?.outcome === 'in_sync' && s.enabled) return 'ok'
  return 'neutral'
})

const rows = computed(() => {
  const s = props.status
  if (!s) return []
  const pushed = s.sync_state?.verified_at ?? null
  return [
    { k: '存储方式', v: PROVIDER[s.provider] ?? s.provider },
    { k: '同步时间', v: pushed ? formatAgo(pushed, props.now) : '尚未推送' },
    { k: '自动推送', v: autoPushOff.value ? `未开启 · ${autoPushOff.value}` : '已开启' },
  ]
})

const lastTask = computed<{ text: string; bad: boolean } | null>(() => {
  if (!r.showLast.value) return null
  const result = r.result.value
  if (!result) return null
  const env = result.envelope
  if (!env) return { text: `任务异常退出（${result.code}），未产生结果`, bad: result.code !== 0 }
  if (!env.ok) return { text: syncErrorText(env.error), bad: true }
  if (result.op === 'push') {
    const data = env.data as PushResult
    return {
      text: data.noop ? `第 ${data.seq} 版内容未变化，未重新上传` : `已推送第 ${data.seq} 版`,
      bad: false,
    }
  }
  if (result.op === 'rotate') return { text: '密码已重置，接收端需要改成新密码', bad: false }
  if (result.op === 'check') {
    const data = env.data as RemoteCheck
    return { text: `云端检查：${CHECK_OUTCOME_LABEL[data.outcome] ?? data.outcome}`, bad: false }
  }
  if (result.op === 'delete') {
    const data = env.data as { deleted?: boolean }
    return { text: data.deleted ? '云端文件已删除' : '云端没有可删除的文件', bad: false }
  }
  return { text: '同步任务完成', bad: false }
})

async function startInit(provider: 'gist' | 'webdav'): Promise<void> {
  if (initBusy.value) return
  initBusy.value = provider
  initError.value = null
  try {
    await initSender(provider)
    void showSnackbar({ message: '已初始化同步流，请设置密码后启用' })
    emit('changed')
  } catch (e) {
    initError.value = e instanceof CommandError ? e.message : String(e)
  } finally {
    initBusy.value = null
  }
}
</script>

<template>
  <!-- KernelSU Miuix home style: state-tinted card with an oversized icon clipped at the bottom-right corner. -->
  <MiuixCard
    class="sync-card"
    :class="status && `sync-card--${tone}`"
    :press-feedback="status ? 'tilt' : 'none'"
    :show-indication="!!status"
    @click="status && emit('detail')"
  >
    <SvgIcon
      v-if="status"
      class="sync__mark"
      :path="TONE_VISUAL[tone].icon"
      :color="TONE_VISUAL[tone].color"
    />
    <div class="sync">
      <div class="sync__head">
        <MiuixText type="title4" weight="medium">远程同步</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <!-- Corner status doubles as the refresh control: tap runs check-remote. -->
        <CardStateButton
          v-else-if="status"
          :label="r.busy.value ? busyText : TONE_LABEL[tone]"
          :busy="r.busy.value"
          :disabled="!canRefresh"
          @refresh="emit('refresh')"
        />
      </div>

      <!-- Never initialized: create the [sync] table with a provider. -->
      <template v-if="notInitialized">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          把当前 keybox 加密后上传到你自己的存储，供其他设备拉取。先选择存储方式；之后设置密码并接受风险说明才能启用。
        </MiuixText>
        <div class="init">
          <MiuixButton type="primary" :disabled="initBusy !== null" @pointerdown.stop @click.stop="startInit('gist')">
            {{ initBusy === 'gist' ? '初始化中…' : '使用 GitHub Gist' }}
          </MiuixButton>
          <MiuixButton :disabled="initBusy !== null" @pointerdown.stop @click.stop="startInit('webdav')">
            {{ initBusy === 'webdav' ? '初始化中…' : '使用 WebDAV' }}
          </MiuixButton>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          Gist 需要只勾选 <code>gist</code> 权限的 token；WebDAV 建议使用可单独撤销的应用密码。密码不会出现在 URL 或二维码中。
        </MiuixText>
        <MiuixText v-if="initError" type="footnote1" color="var(--m-color-error)">{{ initError }}</MiuixText>
      </template>

      <MiuixText v-else-if="error && !status" type="body2" color="var(--m-color-error)">
        读取同步状态失败：{{ error }}
      </MiuixText>

      <template v-else-if="status">
        <div class="rows">
          <div v-for="row in rows" :key="row.k" class="row">
            <span class="row__k">{{ row.k }}</span>
            <span class="row__v">{{ row.v }}</span>
          </div>
        </div>

        <div v-if="warnings.length" class="warnings">
          <MiuixText v-for="w in warnings" :key="w" type="footnote1" color="var(--app-warning)">{{ w }}</MiuixText>
        </div>

        <div v-if="!r.busy.value && lastTask" class="task" @pointerdown.stop @click.stop>
          <MiuixText type="body2" :color="lastTask.bad ? 'var(--m-color-error)' : undefined">{{ lastTask.text }}</MiuixText>
          <MiuixIconButton aria-label="不再显示" @click.stop="r.dismiss()">
            <MiuixIcon :icon="Close" :size="16" color="var(--m-color-on-surface-variant-actions)" />
          </MiuixIconButton>
        </div>
        <div v-else-if="r.phase.value === 'launch_failed'" class="task" @pointerdown.stop @click.stop>
          <MiuixText type="body2" color="var(--m-color-error)">同步任务未能启动，请检查 rkp-mini-sync 是否可执行。</MiuixText>
        </div>
        <div v-else-if="r.phase.value === 'stale'" class="task" @pointerdown.stop @click.stop>
          <MiuixText type="body2" color="var(--m-color-error)">存在残留的同步任务锁，上次任务可能被中断。</MiuixText>
          <MiuixButton :disabled="r.busy.value" @click.stop="r.clearStaleLock()">清除锁</MiuixButton>
        </div>
      </template>

      <MiuixText v-else type="body2" color="var(--m-color-on-surface-variant-summary)">正在读取同步状态…</MiuixText>
    </div>
  </MiuixCard>
</template>

<style scoped>
/* MiuixCard paints its background from --m-card-color; the corner icon still
   shows the state where color-mix() is unavailable. */
.sync-card--ok {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .sync-card--warning {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .sync-card--error {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* `hidden` still lets focus/scrollIntoView pan the card sideways toward the
   oversized corner icon; `clip` never scrolls. */
.sync-card :deep(.m-card) {
  overflow: clip;
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.sync__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.sync,
.sync ~ * {
  position: relative;
}
.sync {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.sync__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 4px 0;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  font-size: 14px;
  line-height: 1.45;
}
.row__k {
  flex: none;
  width: 84px;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  flex: 1;
  min-width: 0;
  word-break: break-all;
}
.warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.init {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
.task {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
code {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
}
</style>
