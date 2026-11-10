<script setup lang="ts">
// Remote-keybox-sync card (design §19.5). Everything on it comes from
// `rkp-mini-sync status`, which is local-only: no keybox, private key, passphrase or
// token is ever read or rendered here. Push runs as a detached `sync.sh` task;
// the card shows its progress and one result, and asks the page to re-read the
// status afterwards. Other sync commands are unavailable while it runs.
import { computed, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixIcon, MiuixIconButton, MiuixProgressIndicator, MiuixText, showSnackbar } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import StatusPill, { type PillTone } from '../../../shared/components/StatusPill.vue'
import RiskSheet from '../../../shared/components/RiskSheet.vue'
import { daysLeft, formatAgo, formatDate, formatElapsed, truncateMiddle } from '../../../shared/lib/format'
import { CommandError } from '../../../shared/lib/command'
import {
  initSender,
  syncCodeLabel,
  syncErrorText,
  syncTaskArgs,
  type PushResult,
  type SecretStatus,
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
}>()
const emit = defineEmits<{ settings: []; share: []; changed: [] }>()

const r = props.runner
const riskOpen = ref(false)
const initBusy = ref<'gist' | 'webdav' | null>(null)
const initError = ref<string | null>(null)

const PROVIDER: Record<string, string> = { gist: 'GitHub Gist', webdav: 'WebDAV', url: '公开 HTTPS' }

const pill = computed<{ tone: PillTone; label: string } | null>(() => {
  const s = props.status
  if (!s) return null
  if (!s.enabled) return { tone: 'neutral', label: '已禁用' }
  if (!s.risk_ack_current) return { tone: 'warning', label: '需重新确认风险' }
  if (!s.loop_running) return { tone: 'warning', label: '循环未运行' }
  return { tone: 'ok', label: '已启用' }
})

function secretText(secret: SecretStatus | undefined): string {
  if (!secret?.present) return '缺失'
  return secret.ok ? '已设置' : '权限错误'
}

function secretBad(secret: SecretStatus | undefined): boolean {
  return !secret?.present || !secret.ok
}

const rows = computed(() => {
  const s = props.status
  if (!s) return []
  const out: { k: string; v: string; bad?: boolean }[] = []
  out.push({ k: '存储方式', v: PROVIDER[s.provider] ?? s.provider })
  out.push({ k: '位置', v: s.location ?? '未配置' })
  out.push({ k: '同步流', v: truncateMiddle(s.stream_id, 12) })

  const state = s.sync_state
  if (state) {
    out.push({
      k: '上次推送',
      v: `第 ${state.verified_seq} 版${state.verified_at ? ` · ${formatAgo(state.verified_at, props.now)}` : ''}`,
    })
    if (state.last_error) {
      out.push({
        k: '上次错误',
        v: `${syncCodeLabel(state.last_error.code)}（${state.last_error.code}）· ${formatAgo(state.last_error.at, props.now)}`,
        bad: true,
      })
    }
  } else {
    out.push({ k: '上次推送', v: '尚未推送' })
  }

  const kb = s.local_keybox
  if (kb) {
    const notAfter = kb.leaf_not_after
    const days = notAfter === undefined ? null : daysLeft(notAfter, props.now)
    out.push({
      k: '本地 keybox',
      v: notAfter === undefined ? kb.summary : `${kb.summary} · 剩余 ${days} 天（${formatDate(notAfter)}）`,
      bad: !kb.valid || (days !== null && days <= 0),
    })
  }

  out.push({ k: '自动签发', v: s.auto_issue_days ? `剩余 ≤ ${s.auto_issue_days} 天时重新签发` : '关闭' })
  out.push({ k: '自动循环', v: s.loop_running ? '运行中' : '未运行' })
  out.push({ k: 'age 口令', v: secretText(s.secrets.age), bad: secretBad(s.secrets.age) })

  if (s.provider === 'gist' || s.provider === 'webdav') {
    const gist = s.provider === 'gist'
    const credential = gist ? s.secrets.gist_token : s.secrets.webdav_password
    out.push({
      k: gist ? 'Gist token' : 'WebDAV 密码',
      v: secretText(credential),
      bad: secretBad(credential),
    })
  }
  return out
})

const canPush = computed(
  () =>
    props.status?.enabled === true &&
    props.status.risk_ack_current &&
    !r.busy.value &&
    r.phase.value !== 'stale',
)

const needsEnable = computed(() => !!props.status && (!props.status.enabled || !props.status.risk_ack_current))

const lastTask = computed(() => {
  if (!r.showLast.value) return null
  const result = r.result.value
  if (!result) return null
  const env = result.envelope
  if (!env) return { text: `任务异常退出（${result.code}），未产生结果`, bad: result.code !== 0 }
  if (env.ok) {
    if (result.op === 'rotate') return { text: '口令轮换任务完成', bad: false }
    const data = env.data as PushResult
    return {
      text: data.noop
        ? `第 ${data.seq} 版内容未变化，未重新上传`
        : `已推送第 ${data.seq} 版（${formatAgo(result.finishedAt, props.now)}）`,
      bad: false,
    }
  }
  return { text: syncErrorText(env.error), bad: true }
})

async function startInit(provider: 'gist' | 'webdav'): Promise<void> {
  if (initBusy.value) return
  initBusy.value = provider
  initError.value = null
  try {
    await initSender(provider)
    void showSnackbar({ message: '已初始化同步流，请设置 age 口令后启用' })
    emit('changed')
  } catch (e) {
    initError.value = e instanceof CommandError ? e.message : String(e)
  } finally {
    initBusy.value = null
  }
}

async function push(): Promise<void> {
  if (!canPush.value) return
  try {
    await r.start(syncTaskArgs('push'))
  } catch (e) {
    void showSnackbar({ message: e instanceof Error ? e.message : String(e) })
  }
}

function onEnabled(): void {
  emit('changed')
}
</script>

<template>
  <MiuixCard>
    <div class="sync">
      <div class="sync__head">
        <MiuixText type="title4" weight="medium">远程同步</MiuixText>
        <MiuixProgressIndicator v-if="loading" type="infinite" :size="18" />
        <StatusPill v-else-if="pill" :tone="pill.tone" :label="pill.label" />
      </div>

      <!-- Never initialized: create the [sync] table with a provider. -->
      <template v-if="notInitialized">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          把当前 keybox 用 age 口令加密后上传到你自己的存储，供其他设备拉取。先选择存储方式；之后设置 age 口令并接受风险说明才能启用。
        </MiuixText>
        <div class="actions">
          <MiuixButton type="primary" :disabled="initBusy !== null" @click="startInit('gist')">
            {{ initBusy === 'gist' ? '初始化中…' : '使用 GitHub Gist' }}
          </MiuixButton>
          <MiuixButton :disabled="initBusy !== null" @click="startInit('webdav')">
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
            <span class="row__v" :class="{ 'row__v--bad': row.bad }">{{ row.v }}</span>
          </div>
        </div>

        <div v-if="r.busy.value" class="task">
          <MiuixProgressIndicator type="infinite" :size="18" />
          <MiuixText type="body2">
            推送进行中 · 已用时 {{ formatElapsed(r.elapsedMs.value) }}{{ r.overdue.value ? ' · 结果未知' : '' }}
          </MiuixText>
        </div>
        <div v-else-if="lastTask" class="task">
          <MiuixText type="body2" :color="lastTask.bad ? 'var(--m-color-error)' : undefined">{{ lastTask.text }}</MiuixText>
          <MiuixIconButton aria-label="不再显示" @click="r.dismiss()">
            <MiuixIcon :icon="Close" :size="16" color="var(--m-color-on-surface-variant-actions)" />
          </MiuixIconButton>
        </div>
        <div v-else-if="r.phase.value === 'launch_failed'" class="task">
          <MiuixText type="body2" color="var(--m-color-error)">推送任务未能启动，请检查 rkp-mini-sync 是否可执行。</MiuixText>
        </div>
        <div v-else-if="r.phase.value === 'stale'" class="task">
          <MiuixText type="body2" color="var(--m-color-error)">存在残留的同步任务锁，上次任务可能被中断。</MiuixText>
          <MiuixButton :disabled="r.busy.value" @click="r.clearStaleLock()">清除锁</MiuixButton>
        </div>

        <MiuixText v-if="!status.enabled" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          同步已禁用：定时任务与自动推送不会运行，本地配置和远端文件都保留。禁用不是撤销。
        </MiuixText>
        <MiuixText v-else-if="!status.risk_ack_current" type="footnote1" color="var(--app-warning)">
          风险说明已更新，需要重新逐条确认才能继续推送。
        </MiuixText>

        <div class="actions">
          <MiuixButton v-if="needsEnable" type="primary" @click="riskOpen = true">启用</MiuixButton>
          <MiuixButton v-else type="primary" :disabled="!canPush" @click="push">
            {{ r.busy.value ? '推送中…' : '立即推送' }}
          </MiuixButton>
          <MiuixButton :disabled="r.busy.value" @click="emit('settings')">同步设置</MiuixButton>
          <MiuixButton :disabled="r.busy.value || !status.enabled" @click="emit('share')">分享</MiuixButton>
        </div>
      </template>

      <template v-else>
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">正在读取同步状态…</MiuixText>
      </template>
    </div>
  </MiuixCard>

  <RiskSheet v-model="riskOpen" @enabled="onEnabled" />
</template>

<style scoped>
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
  margin-bottom: 2px;
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
  width: 92px;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  flex: 1;
  min-width: 0;
  word-break: break-all;
  user-select: text;
  -webkit-user-select: text;
}
.row__v--bad {
  color: var(--m-color-error);
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
.actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
code {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
}
</style>
