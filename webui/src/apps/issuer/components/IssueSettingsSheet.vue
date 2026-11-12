<script setup lang="ts">
// Issue settings. Auto-issue (issuer-owned `auto_issue_days`) is written by
// `rkp-mini-issuer config set`; the loop itself is `auto-loop.sh`, started here
// whenever auto-issue is on and its pid file says it is not running. The last
// run is read straight from the loop's `tmp/auto-issue.last.*` files (C6).
// The danger zone deletes the local keybox and its artifacts (`remove --yes`).
import { computed, ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixDialog,
  MiuixIcon,
  MiuixIconButton,
  MiuixInput,
  MiuixSmallTitle,
  MiuixSwitchPreference,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import { CommandError } from '../../../shared/lib/command'
import { formatAgo, formatDateTime } from '../../../shared/lib/format'
import {
  AUTO_ISSUE_DEFAULT_DAYS,
  AUTO_ISSUE_WARN_DAYS,
  readAutoIssueLast,
  removeKeybox,
  setAutoIssueDays,
  stageLabel,
  startAutoLoop,
  type AutoIssueLast,
  type StatusData,
} from '../lib/rkp'

const open = defineModel<boolean>({ required: true })
/** `busy`: an issuance is running, so nothing may be deleted. */
const props = defineProps<{ status: StatusData | null; busy: boolean }>()
const emit = defineEmits<{ changed: [] }>()

const confirmRemove = ref(false)
const removing = ref(false)

async function remove(): Promise<void> {
  if (removing.value) return
  removing.value = true
  error.value = null
  try {
    const r = await removeKeybox()
    confirmRemove.value = false
    open.value = false
    emit('changed')
    void showSnackbar({ message: r.removed.length ? '本地证书已删除' : '本地没有可删除的证书' })
  } catch (e) {
    confirmRemove.value = false
    error.value = message(e)
  } finally {
    removing.value = false
  }
}

const on = ref(false)
const days = ref(String(AUTO_ISSUE_DEFAULT_DAYS))
const saving = ref(false)
const error = ref<string | null>(null)
const last = ref<AutoIssueLast | null>(null)
const lastLoading = ref(false)
const lastError = ref<string | null>(null)

const loopText = computed(() => {
  const s = props.status
  if (!s) return '读取中…'
  return s.auto_issue?.loop_running ? '运行中' : '未运行'
})

const lastConsequence = computed(() => {
  const l = last.value
  if (!l) return null
  const env = l.envelope
  if (!env) return { text: `异常退出（${l.code}）`, bad: true }
  if (env.ok) {
    if ('skipped' in env.data) return { text: `证书仍有 ${env.data.days_left} 天，未签发`, bad: false }
    return { text: `已签发新证书（Device ID ${env.data.device_id}）`, bad: false }
  }
  const e = env.error
  const ta = e.ta_status === undefined ? '' : `（TA ${e.ta_status}）`
  return { text: `签发失败：${stageLabel(e.stage)}：${e.code}${ta}`, bad: true }
})

const lastAt = computed(() => {
  const at = last.value?.at
  return at ? `${formatAgo(at)} · ${formatDateTime(at)}` : '—'
})

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

/** Draft mirrors the last read status; 0 days keeps the default ready for switching on. */
function syncDraft(): void {
  const d = props.status?.auto_issue?.days ?? 0
  on.value = d > 0
  days.value = String(d > 0 ? d : AUTO_ISSUE_DEFAULT_DAYS)
}

const tooLong = computed(() => on.value && Number(days.value.trim()) > AUTO_ISSUE_WARN_DAYS)

async function loadLast(): Promise<void> {
  lastLoading.value = true
  lastError.value = null
  try {
    last.value = await readAutoIssueLast()
  } catch (e) {
    last.value = null
    lastError.value = message(e)
  } finally {
    lastLoading.value = false
  }
}

watch(open, (v) => {
  if (!v) return
  syncDraft()
  error.value = null
  void loadLast()
})

// Opened while the status was still loading: fill the draft once it arrives
// (saving is disabled until then, so an unknown value is never overwritten).
watch(
  () => props.status,
  (s, prev) => {
    if (open.value && s && !prev) syncDraft()
  },
)

async function save(): Promise<void> {
  if (saving.value) return
  error.value = null
  const n = on.value ? Number(days.value.trim()) : 0
  if (on.value && (!Number.isInteger(n) || n < 1 || n > 365)) {
    error.value = '天数必须是 1–365 的整数'
    return
  }
  saving.value = true
  try {
    await setAutoIssueDays(n)
    if (n > 0 && !props.status?.auto_issue?.loop_running) await startAutoLoop()
    emit('changed')
    void showSnackbar({ message: '自动签发设置已保存' })
  } catch (e) {
    error.value = message(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="签发设置">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixSmallTitle text="自动签发" />
      <div class="sheet-group">
        <MiuixSwitchPreference
          v-model="on"
          title="自动签发"
          :summary="on ? `证书剩余 ≤ ${days} 天时重新签发` : '关闭'"
        />
        <MiuixInput v-if="on" v-model="days" label="剩余天数 ≤ N 时重新签发" single-line />
      </div>
      <MiuixText v-if="tooLong" type="footnote1" color="var(--app-warning)">
        超过 {{ AUTO_ISSUE_WARN_DAYS }} 天：本机签发的证书有效期只有约 12–14 天，每次检查都会重新签发，可能导致频繁签发。建议设为 3。
      </MiuixText>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        后台进程每约 12 小时检查一次证书；签发成功后，如远程同步已启用会随即推送。休眠可能推迟检查。
      </MiuixText>

      <div class="sheet-group">
        <MiuixBasicComponent title="后台进程" :summary="loopText" />
      </div>

      <MiuixSmallTitle text="上次自动检查" />
      <div class="sheet-group">
        <MiuixBasicComponent v-if="lastLoading" title="正在读取…" />
        <MiuixBasicComponent v-else-if="lastError" title="读取失败" :summary="lastError" summary-color="var(--m-color-error)" />
        <template v-else-if="last">
          <MiuixBasicComponent title="时间" :summary="lastAt" />
          <MiuixBasicComponent
            v-if="lastConsequence"
            title="结果"
            :summary="lastConsequence.text"
            :summary-color="lastConsequence.bad ? 'var(--m-color-error)' : undefined"
          />
          <FoldBlock v-if="last.stderr.trim()" title="错误输出（stderr）" :text="last.stderr.trim()" />
        </template>
        <MiuixBasicComponent v-else title="尚无记录" />
      </div>

      <MiuixButton type="primary" :disabled="saving || !status" @click="save">{{ saving ? '保存中…' : '保存' }}</MiuixButton>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixSmallTitle text="危险区" color="var(--m-color-error)" />
      <div class="sheet-group danger-group">
        <MiuixBasicComponent
          title="删除本地证书"
          :summary="
            status?.keybox.exists
              ? '删除 keybox.xml、备份、私钥与证书链；不影响云端文件'
              : '本地没有已签发的证书'
          "
          title-color="var(--m-color-error)"
          clickable
          :disabled="busy || removing || !status?.keybox.exists"
          @click="confirmRemove = true"
        />
      </div>
    </div>

    <MiuixDialog
      v-model="confirmRemove"
      title="删除本地证书？"
      summary="将删除输出目录中的 keybox.xml、keybox.xml.prev、私钥、证书链与 meta.json，无法恢复。读取该 keybox 的模块将失去证书；已同步到云端的文件不受影响。若开启了自动签发，下一轮检查会重新签发。"
    >
      <div class="dialog-actions">
        <MiuixButton @click="confirmRemove = false">取消</MiuixButton>
        <MiuixButton class="danger-btn" type="primary" :disabled="removing || busy" @click="remove">
          {{ removing ? '删除中…' : '确认删除' }}
        </MiuixButton>
      </div>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.danger-group {
  background: color-mix(in srgb, var(--m-color-error) 8%, var(--app-sheet-group));
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 12px;
}
.danger-btn.m-button--primary:not(.m-button--disabled) {
  background: var(--m-color-error);
  color: #fff;
}
</style>
