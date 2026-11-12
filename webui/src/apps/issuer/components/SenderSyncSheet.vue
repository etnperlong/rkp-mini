<script setup lang="ts">
// Sync settings for the sender (design §13.3, §19.4). Only `config set`'s
// allow-list, the enable/disable/forget commands and the secret inbox are used:
// the page never reads config.toml, a secret file or the keybox itself.
import { computed, reactive, ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixDialog,
  MiuixDropdownPreference,
  MiuixIcon,
  MiuixIconButton,
  MiuixInput,
  MiuixSmallTitle,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { CommandError } from '../../../shared/lib/command'
import {
  SYNC_LOG,
  SYNC_LOG_LABEL,
  disableSync,
  forgetSync,
  readSyncConfig,
  readSyncLog,
  setConfig,
  type SecretKind,
  type SecretStatus,
  type SenderConfigKey,
  type SyncConfig,
  type SyncStatus,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{
  status: SyncStatus | null
  /** A sync task (push/rotate) is running: no other sync command may start. */
  busy: boolean
}>()
const emit = defineEmits<{ secret: [kind: SecretKind]; rotate: []; changed: [] }>()

const cfg = ref<SyncConfig | null>(null)
const loading = ref(false)
const loadError = ref<string | null>(null)
const saving = ref(false)
const pending = ref<'disable' | 'forget' | null>(null)
const actionError = ref<string | null>(null)
const confirmForget = ref(false)
const logOpen = ref(false)
const logLoading = ref(false)
const logText = ref('')

const draft = reactive({
  provider: 'gist' as 'gist' | 'webdav',
  label: '',
  min_validity_days: '3',
  proxy: '',
  gist_id: '',
  gist_filename: 'rkp-sync.age',
  webdav_endpoint: '',
  webdav_path: '/rkp/rkp-sync.age',
  webdav_user: '',
})

const PROVIDERS = ['GitHub Gist', 'WebDAV']

const providerIndex = computed({
  get: () => (draft.provider === 'webdav' ? 1 : 0),
  set: (index: number) => {
    draft.provider = index === 1 ? 'webdav' : 'gist'
  },
})

const locked = computed(() => props.busy || saving.value || pending.value !== null)

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

function secretText(secret: SecretStatus | undefined): string {
  if (!secret?.present) return '缺失'
  return secret.ok ? `已设置（${secret.mode ?? '0600'}）` : `权限错误（${secret.mode ?? '?'}）`
}

async function load(): Promise<void> {
  loading.value = true
  loadError.value = null
  actionError.value = null
  logOpen.value = false
  logText.value = ''
  try {
    const c = await readSyncConfig()
    cfg.value = c
    draft.provider = c.provider === 'webdav' ? 'webdav' : 'gist'
    draft.label = c.label
    draft.min_validity_days = String(c.min_validity_days)
    draft.proxy = c.proxy
    draft.gist_id = c.gist_id
    draft.gist_filename = c.gist_filename || 'rkp-sync.age'
    draft.webdav_endpoint = c.webdav_endpoint
    draft.webdav_path = c.webdav_path || '/rkp/rkp-sync.age'
    draft.webdav_user = c.webdav_user
  } catch (e) {
    loadError.value = message(e)
  } finally {
    loading.value = false
  }
}

watch(open, (v) => {
  if (v) void load()
})

/** Non-negative whole days, or null when the field is not a valid value. */
function parseDays(value: string): number | null {
  const text = value.trim()
  if (!text) return null
  const n = Number(text)
  return Number.isInteger(n) && n >= 0 ? n : null
}

async function save(): Promise<void> {
  const c = cfg.value
  if (!c || locked.value) return
  actionError.value = null

  const minDays = parseDays(draft.min_validity_days)
  if (minDays === null) {
    actionError.value = '最小剩余有效期必须是非负整数'
    return
  }

  const updates: [SenderConfigKey, string | number][] = []
  if (draft.provider !== c.provider) updates.push(['provider', draft.provider])
  if (draft.label !== c.label) updates.push(['label', draft.label])
  if (minDays !== c.min_validity_days) updates.push(['min_validity_days', minDays])
  if (draft.proxy !== c.proxy) updates.push(['proxy', draft.proxy])
  if (draft.provider === 'gist') {
    if (draft.gist_id !== c.gist_id) updates.push(['gist_id', draft.gist_id])
    if (draft.gist_filename !== c.gist_filename) updates.push(['gist_filename', draft.gist_filename])
  } else {
    if (draft.webdav_endpoint !== c.webdav_endpoint) updates.push(['webdav_endpoint', draft.webdav_endpoint])
    if (draft.webdav_path !== c.webdav_path) updates.push(['webdav_path', draft.webdav_path])
    if (draft.webdav_user !== c.webdav_user) updates.push(['webdav_user', draft.webdav_user])
  }

  if (!updates.length) {
    void showSnackbar({ message: '没有需要保存的更改' })
    return
  }

  saving.value = true
  try {
    for (const [key, value] of updates) await setConfig(key, value)
    void showSnackbar({ message: '设置已保存' })
    emit('changed')
    await load()
  } catch (e) {
    actionError.value = message(e)
  } finally {
    saving.value = false
  }
}

async function disable(): Promise<void> {
  if (locked.value || !props.status?.enabled) return
  pending.value = 'disable'
  actionError.value = null
  try {
    await disableSync()
    void showSnackbar({ message: '已禁用远程同步；本地与远端数据均保留' })
    emit('changed')
    await load()
  } catch (e) {
    actionError.value = message(e)
  } finally {
    pending.value = null
  }
}

async function forget(): Promise<void> {
  if (pending.value !== null) return
  pending.value = 'forget'
  actionError.value = null
  try {
    await forgetSync(false)
    confirmForget.value = false
    open.value = false
    void showSnackbar({ message: '本地同步数据已清除（远端文件未删除）' })
    emit('changed')
  } catch (e) {
    actionError.value = message(e)
  } finally {
    pending.value = null
  }
}

async function toggleLog(): Promise<void> {
  if (logOpen.value) {
    logOpen.value = false
    return
  }
  logOpen.value = true
  logText.value = ''
  logLoading.value = true
  try {
    logText.value = await readSyncLog()
  } catch (e) {
    logText.value = message(e)
  } finally {
    logLoading.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="远程同步设置">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        这里只修改 [sync] 表中的非敏感字段；密钥经 root shell 写入 0600 文件后由 rkp-mini-sync 接管，页面不会读回。
      </MiuixText>

      <MiuixText v-if="loading" type="body2" color="var(--m-color-on-surface-variant-summary)">正在读取配置…</MiuixText>
      <MiuixText v-else-if="loadError" type="body2" color="var(--m-color-error)">读取配置失败：{{ loadError }}</MiuixText>

      <template v-else-if="cfg">
        <MiuixSmallTitle text="存储" />
        <div class="sheet-group">
          <MiuixDropdownPreference
            v-model="providerIndex"
            title="存储方式"
            :summary="draft.provider === 'gist' ? 'GitHub Gist（密文，需 token）' : 'WebDAV（自定义路径，需应用密码）'"
            :items="PROVIDERS"
            :disabled="locked"
          />
          <template v-if="draft.provider === 'gist'">
            <MiuixInput
              v-model="draft.gist_id"
              label="Gist ID（可留空，首次推送创建）"
              single-line
              :disabled="locked"
            />
            <MiuixInput v-model="draft.gist_filename" label="文件名" single-line :disabled="locked" />
          </template>
          <template v-else>
            <MiuixInput v-model="draft.webdav_endpoint" label="Endpoint（https://…/dav/）" single-line :disabled="locked" />
            <MiuixInput v-model="draft.webdav_path" label="路径（/rkp/rkp-sync.age）" single-line :disabled="locked" />
            <MiuixInput v-model="draft.webdav_user" label="用户名（可留空）" single-line :disabled="locked" />
          </template>
        </div>

        <MiuixSmallTitle text="凭据" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="age 口令"
            :summary="secretText(status?.secrets.age)"
            clickable
            :disabled="locked"
            @click="emit('secret', 'age')"
          />
          <MiuixBasicComponent
            v-if="draft.provider === 'gist'"
            title="Gist token"
            :summary="secretText(status?.secrets.gist_token)"
            clickable
            :disabled="locked"
            @click="emit('secret', 'gist-token')"
          />
          <MiuixBasicComponent
            v-else
            title="WebDAV 密码"
            :summary="secretText(status?.secrets.webdav_password)"
            clickable
            :disabled="locked"
            @click="emit('secret', 'webdav-password')"
          />
          <MiuixBasicComponent
            title="轮换 age 口令"
            summary="用新口令重新加密当前 keybox；接收端需要同时更新口令"
            clickable
            :disabled="locked"
            @click="emit('rotate')"
          />
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          Gist token 只勾选 gist 权限；WebDAV 建议使用可单独撤销的应用密码。这里只显示“已设置/缺失/权限错误”。
        </MiuixText>

        <MiuixSmallTitle text="策略" />
        <div class="sheet-group">
          <MiuixInput v-model="draft.label" label="标签（仅用于分享展示）" single-line :disabled="locked" />
          <MiuixInput v-model="draft.min_validity_days" label="最小剩余有效期（天）" single-line :disabled="locked" />
          <MiuixInput v-model="draft.proxy" label="代理（可留空）" single-line :disabled="locked" />
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          自动签发在“签发”页单独设置，不依赖远程同步。
        </MiuixText>

        <MiuixButton type="primary" :disabled="locked" @click="save">
          {{ saving ? '保存中…' : '保存设置' }}
        </MiuixButton>

        <MiuixSmallTitle text="启用状态" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="禁用同步"
            :summary="
              status?.enabled
                ? '停止定时任务与自动推送；保留本地配置、密钥、远端文件与已部署 keybox。禁用不是撤销。'
                : '同步当前已禁用'
            "
            :clickable="!!status?.enabled"
            :disabled="locked || !status?.enabled"
            @click="disable"
          />
          <MiuixBasicComponent
            title="清除本地数据"
            summary="删除本地同步状态和已保存的密钥文件；远端文件与已部署 keybox 不删除"
            clickable
            :disabled="locked"
            @click="confirmForget = true"
          />
        </div>

        <MiuixSmallTitle text="同步日志" />
        <div class="sheet-group">
          <MiuixBasicComponent :title="SYNC_LOG_LABEL" :summary="SYNC_LOG" clickable @click="toggleLog" />
          <pre v-if="logOpen" class="log">{{ logLoading ? '读取中…' : logText || '（空）' }}</pre>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          日志只含 provider、seq、hash 与错误码，不含口令或 token。
        </MiuixText>

        <MiuixText v-if="actionError" type="footnote1" color="var(--m-color-error)">{{ actionError }}</MiuixText>
      </template>
    </div>

    <MiuixDialog
      v-model="confirmForget"
      title="清除本地同步数据？"
      summary="将删除本地同步状态与已保存的 age 口令 / token / WebDAV 密码。远端文件不会被删除，已部署的 keybox 也不受影响；之后需要重新初始化。"
    >
      <div class="dialog-actions">
        <MiuixButton @click="confirmForget = false">取消</MiuixButton>
        <MiuixButton type="primary" :disabled="pending !== null" @click="forget">
          {{ pending === 'forget' ? '清除中…' : '确认清除' }}
        </MiuixButton>
      </div>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.log {
  margin: 0 16px 16px;
  padding: 12px;
  max-height: 40vh;
  overflow: auto;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  user-select: text;
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 12px;
}
</style>
