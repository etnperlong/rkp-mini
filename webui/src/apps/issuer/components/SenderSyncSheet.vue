<script setup lang="ts">
// Sync settings for the sender (design §13.3, §19.4/§19.5). Only `config set`'s
// allow-list, forget/remote-delete and the secret inbox are used: the page never
// reads config.toml, a secret file or the keybox itself, and never shows whether
// a secret is stored.
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
  MiuixSwitchPreference,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { CommandError } from '../../../shared/lib/command'
import {
  forgetSync,
  readSyncConfig,
  setConfig,
  startLoop,
  type SecretKind,
  type SenderConfigKey,
  type SyncConfig,
  type SyncStatus,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{
  status: SyncStatus | null
  /** A sync task (push/check/delete/rotate) is running: no command may start. */
  busy: boolean
}>()
const emit = defineEmits<{ secret: [kind: SecretKind]; rotate: []; delete: []; changed: [] }>()

const cfg = ref<SyncConfig | null>(null)
const loading = ref(false)
const loadError = ref<string | null>(null)
const saving = ref(false)
const pending = ref<'forget' | 'reset' | null>(null)
const actionError = ref<string | null>(null)
/** Danger action awaiting its confirmation dialog. */
const danger = ref<'delete' | 'forget' | 'reset' | null>(null)

const draft = reactive({
  provider: 'gist' as 'gist' | 'webdav',
  auto_push: true,
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
/** `auto_push` only matters once the stream is enabled; before that it is read-only. */
const syncEnabled = computed(() => props.status?.enabled === true)

const dialogOpen = computed({
  get: () => danger.value !== null,
  set: (v: boolean) => {
    if (!v) danger.value = null
  },
})

const DIALOG: Record<'delete' | 'forget' | 'reset', { title: string; summary: string; ok: string }> = {
  delete: {
    title: '删除云端文件？',
    summary:
      '删除不等于撤销，已拉取过的接收端仍保留 keybox。Gist 会整个删除，下次推送会新建 Gist，接收端需重新导入链接。',
    ok: '删除云端文件',
  },
  forget: {
    title: '清除本地同步数据？',
    summary: '将删除本地同步状态与已保存的密码 / token / WebDAV 密码。云端文件不会被删除，已部署的 keybox 也不受影响。',
    ok: '确认清除',
  },
  reset: {
    title: '完全重置同步设置？',
    summary:
      '在清除本地数据之外，还会删除整个 [sync] 配置表，页面回到未初始化。云端文件与已部署的 keybox 不删除；之后需要重新选择存储方式。',
    ok: '确认重置',
  },
}

const dialog = computed(() => (danger.value ? DIALOG[danger.value] : DIALOG.forget))

/** WebDAV deletes only the one file; say so instead of the Gist wording. */
const dialogSummary = computed(() => {
  if (danger.value === 'delete' && draft.provider !== 'gist') {
    return '删除不等于撤销，已拉取过的接收端仍保留 keybox。只删除该 WebDAV 文件，下次推送会重新创建。'
  }
  return dialog.value.summary
})

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

async function load(): Promise<void> {
  loading.value = true
  loadError.value = null
  actionError.value = null
  try {
    const c = await readSyncConfig()
    cfg.value = c
    draft.provider = c.provider === 'webdav' ? 'webdav' : 'gist'
    draft.auto_push = c.auto_push ?? true
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

  const updates: [SenderConfigKey, string | number | boolean][] = []
  if (draft.provider !== c.provider) updates.push(['provider', draft.provider])
  if (draft.auto_push !== (c.auto_push ?? true)) updates.push(['auto_push', draft.auto_push])
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

  const turningOn = draft.auto_push && !(c.auto_push ?? true)
  saving.value = true
  try {
    for (const [key, value] of updates) await setConfig(key, value)
    // The loop is what actually pushes; starting it is idempotent (pid lock).
    if (turningOn) await startLoop()
    void showSnackbar({ message: '设置已保存' })
    emit('changed')
    await load()
  } catch (e) {
    actionError.value = message(e)
  } finally {
    saving.value = false
  }
}

async function runDanger(): Promise<void> {
  const action = danger.value
  if (!action || locked.value) return
  actionError.value = null

  if (action === 'delete') {
    danger.value = null
    open.value = false
    emit('delete')
    return
  }

  pending.value = action
  try {
    await forgetSync(false, action === 'reset')
    danger.value = null
    open.value = false
    void showSnackbar({ message: action === 'reset' ? '已完全重置同步设置' : '本地同步数据已清除（云端文件未删除）' })
    emit('changed')
  } catch (e) {
    actionError.value = message(e)
  } finally {
    pending.value = null
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
        <MiuixSmallTitle text="自动推送" />
        <div class="sheet-group">
          <MiuixSwitchPreference
            v-model="draft.auto_push"
            title="自动推送"
            :summary="
              syncEnabled
                ? '每轮自动签发后，检查通过就推送新 keybox；手动推送不受影响'
                : '同步启用后才能开启自动推送'
            "
            :disabled="locked || !syncEnabled"
          />
          <MiuixInput
            v-model="draft.min_validity_days"
            label="最小剩余有效期（天）"
            single-line
            :disabled="locked"
          />
        </div>

        <MiuixSmallTitle text="云端存储" />
        <div class="sheet-group">
          <MiuixDropdownPreference
            v-model="providerIndex"
            title="存储方式"
            :summary="draft.provider === 'gist' ? 'GitHub Gist（密文，需 token）' : 'WebDAV（自定义路径，需应用密码）'"
            :items="PROVIDERS"
            :disabled="locked"
          />
          <template v-if="draft.provider === 'gist'">
            <MiuixInput v-model="draft.gist_id" label="Gist ID（可留空，首次推送创建）" single-line :disabled="locked" />
            <MiuixInput v-model="draft.gist_filename" label="文件名" single-line :disabled="locked" />
            <MiuixBasicComponent
              title="Gist token"
              summary="用于创建和更新 Gist，只勾选 gist 权限"
              clickable
              :disabled="locked"
              @click="emit('secret', 'gist-token')"
            />
          </template>
          <template v-else>
            <MiuixInput v-model="draft.webdav_endpoint" label="Endpoint（https://…/dav/）" single-line :disabled="locked" />
            <MiuixInput v-model="draft.webdav_path" label="路径（/rkp/rkp-sync.age）" single-line :disabled="locked" />
            <MiuixInput v-model="draft.webdav_user" label="用户名（可留空）" single-line :disabled="locked" />
            <MiuixBasicComponent
              title="WebDAV 密码"
              summary="建议使用可单独撤销的应用密码"
              clickable
              :disabled="locked"
              @click="emit('secret', 'webdav-password')"
            />
          </template>
          <MiuixInput v-model="draft.proxy" label="代理（可留空）" single-line :disabled="locked" />
        </div>

        <MiuixSmallTitle text="加密" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="密码"
            summary="在接收端需要输入密码用来解密云端储存的 Keybox"
            clickable
            :disabled="locked"
            @click="emit('secret', 'age')"
          />
        </div>

        <MiuixSmallTitle text="其他" />
        <div class="sheet-group">
          <MiuixInput v-model="draft.label" label="标签（仅用于分享展示）" single-line :disabled="locked" />
        </div>

        <MiuixButton type="primary" :disabled="locked" @click="save">
          {{ saving ? '保存中…' : '保存设置' }}
        </MiuixButton>

        <MiuixText v-if="actionError" type="footnote1" color="var(--m-color-error)">{{ actionError }}</MiuixText>

        <MiuixSmallTitle text="危险区" color="var(--m-color-error)" />
        <div class="sheet-group danger-group">
          <MiuixBasicComponent
            title="删除云端文件"
            summary="删除前会先用本机密码解密核对同步流"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'delete'"
          />
          <MiuixBasicComponent
            title="重置加密密码"
            summary="用新密码重新加密并上传，接收端需要同时更新"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="emit('rotate')"
          />
          <MiuixBasicComponent
            title="清除本地数据"
            summary="删除本地同步状态与密钥文件；云端文件与已部署 keybox 不删除"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'forget'"
          />
          <MiuixBasicComponent
            title="完全重置同步设置"
            summary="清除本地数据并删除 [sync] 配置，回到未初始化"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'reset'"
          />
        </div>
      </template>
    </div>

    <MiuixDialog v-model="dialogOpen" :title="dialog.title" :summary="dialogSummary">
      <div class="dialog-actions">
        <MiuixButton @click="dialogOpen = false">取消</MiuixButton>
        <MiuixButton class="danger-btn" type="primary" :disabled="pending !== null || busy" @click="runDanger">
          {{ pending ? '处理中…' : dialog.ok }}
        </MiuixButton>
      </div>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 12px;
}
.danger-group {
  background: color-mix(in srgb, var(--m-color-error) 8%, var(--app-sheet-group));
}
.danger-btn.m-button--primary:not(.m-button--disabled) {
  background: var(--m-color-error);
  color: #fff;
}
</style>
