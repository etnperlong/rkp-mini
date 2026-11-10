<script setup lang="ts">
// Receiver settings (§13.3, §19.4): policy fields via the `config set`
// allow-list, secret presence and secret entry (shared SecretSheet), disable
// (keeps deployed keyboxes, not revocation), forget-local-data and the sync log.
import { computed, ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixDialog,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixRadioButtonPreference,
  MiuixSmallTitle,
  MiuixSwitchPreference,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { ChevronForward, Close } from 'miuix-vue/icons'
import FoldBlock from '../components/FoldBlock.vue'
import ReceiverField from './ReceiverField.vue'
import { errorMessage, secretText } from './display'
import {
  IDENTITY_POLICY_LABEL,
  SYNC_BIN,
  SYNC_CONFIG,
  SYNC_DATA,
  SYNC_LOG,
  SYNC_MODULE_DIR,
  disableSync,
  forgetSync,
  readSyncConfig,
  readSyncLog,
  setConfig,
  type IdentityPolicy,
  type ReceiverConfigKey,
  type SecretKind,
  type SyncConfig,
  type SyncStatus,
} from '../lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ status: SyncStatus | null }>()
const emit = defineEmits<{
  refresh: []
  risk: []
  import: []
  secret: [kind: SecretKind]
}>()

const POLICIES: IdentityPolicy[] = ['scheduled', 'always', 'never']

interface Form {
  policy: IdentityPolicy
  lead: string
  minValidity: string
  autoDeploy: boolean
  label: string
  proxy: string
}

const config = ref<SyncConfig | null>(null)
const loading = ref(false)
const saving = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)
const form = ref<Form>({
  policy: 'scheduled',
  lead: '30',
  minValidity: '7',
  autoDeploy: true,
  label: '',
  proxy: '',
})

const logText = ref('')
const logLoading = ref(false)
const logError = ref<string | null>(null)
const forgetStaging = ref(false)
const forgetOpen = ref(false)
const forgetBusy = ref(false)

const enabled = computed(() => !!props.status?.enabled)
const riskOutdated = computed(() => !!props.status && !props.status.risk_ack_current)
const webdavNeeded = computed(() => config.value?.provider === 'webdav' && !!config.value.webdav_user)

watch(open, (v) => {
  if (v) void load()
})

function fill(cfg: SyncConfig): void {
  form.value = {
    policy: cfg.identity_change_policy ?? 'scheduled',
    lead: String(cfg.rotation_lead_days ?? 30),
    minValidity: String(cfg.min_validity_days),
    autoDeploy: cfg.auto_deploy ?? true,
    label: cfg.label,
    proxy: cfg.proxy,
  }
}

async function loadLog(): Promise<void> {
  logLoading.value = true
  logError.value = null
  try {
    logText.value = await readSyncLog(200)
  } catch (e) {
    logText.value = ''
    logError.value = errorMessage(e)
  } finally {
    logLoading.value = false
  }
}

async function load(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    const cfg = await readSyncConfig()
    config.value = cfg
    fill(cfg)
  } catch (e) {
    config.value = null
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
  await loadLog()
}

/** Non-negative integer from a text draft; invalid input is rejected before any write. */
function count(what: string, raw: string): number {
  const value = raw.trim()
  if (!/^\d+$/.test(value)) throw new Error(`${what}需要非负整数`)
  const n = Number(value)
  if (!Number.isSafeInteger(n)) throw new Error(`${what}超出范围`)
  return n
}

async function save(): Promise<void> {
  const cfg = config.value
  if (!cfg) return
  saving.value = true
  error.value = null
  try {
    const ops: [ReceiverConfigKey, string | number | boolean][] = []
    if (form.value.policy !== (cfg.identity_change_policy ?? 'scheduled')) {
      ops.push(['identity_change_policy', form.value.policy])
    }
    const lead = count('轮换提前天数', form.value.lead)
    if (lead !== cfg.rotation_lead_days) ops.push(['rotation_lead_days', lead])
    const min = count('最短有效期', form.value.minValidity)
    if (min !== cfg.min_validity_days) ops.push(['min_validity_days', min])
    if (form.value.autoDeploy !== (cfg.auto_deploy ?? true)) ops.push(['auto_deploy', form.value.autoDeploy])
    if (form.value.label !== cfg.label) ops.push(['label', form.value.label])
    if (form.value.proxy !== cfg.proxy) ops.push(['proxy', form.value.proxy])

    if (!ops.length) {
      void showSnackbar({ message: '没有需要保存的改动' })
      return
    }
    for (const [key, value] of ops) await setConfig(key, value)
    await load()
    emit('refresh')
    void showSnackbar({ message: '设置已保存' })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    saving.value = false
  }
}

async function disable(): Promise<void> {
  busy.value = true
  error.value = null
  try {
    await disableSync()
    await load()
    emit('refresh')
    void showSnackbar({ message: '已禁用同步；已部署的 keybox 保留，且未撤销任何证书' })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busy.value = false
  }
}

async function confirmForget(): Promise<void> {
  forgetBusy.value = true
  error.value = null
  try {
    const result = await forgetSync(forgetStaging.value)
    forgetOpen.value = false
    await load()
    emit('refresh')
    void showSnackbar({ message: `已删除 ${result.removed.length} 项本地数据；远端同步文件未被删除` })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    forgetBusy.value = false
  }
}

const paths = computed(() => [
  { label: '模块目录', value: SYNC_MODULE_DIR },
  { label: 'rkp-mini-sync 二进制', value: SYNC_BIN },
  { label: '配置', value: SYNC_CONFIG },
  { label: '数据目录', value: SYNC_DATA },
  { label: '同步日志', value: SYNC_LOG },
])
</script>

<template>
  <MiuixBottomSheet v-model="open" title="接收端设置">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixSmallTitle text="启用状态" />
      <div class="sheet-group">
        <MiuixBasicComponent
          v-if="enabled"
          title="同步已启用"
          summary="后台循环会按模块 service.sh 的调度运行；策略由下方决定。"
        />
        <MiuixBasicComponent v-else title="同步已禁用" summary="不会自动拉取或部署；已部署 keybox 保留。" />
        <MiuixBasicComponent
          v-if="!enabled"
          title="启用同步…"
          summary="逐条确认风险说明后启用；会同时启动后台循环。"
          clickable
          @click="emit('risk')"
        >
          <template #end>
            <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          v-if="riskOutdated"
          title="重新确认风险说明"
          summary="风险说明版本已更新，必须重新确认后后台循环才会继续。"
          clickable
          @click="emit('risk')"
        >
          <template #end>
            <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
      </div>
      <MiuixButton v-if="enabled" :disabled="busy" @click="disable">
        {{ busy ? '禁用中…' : '禁用同步（保留已部署 keybox）' }}
      </MiuixButton>
      <MiuixText v-if="enabled" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        禁用会停止定时任务与后台循环（循环在下一次唤醒时退出），保留已部署的 keybox。禁用不是撤销：旧 keybox 仍然有效，真正撤销需要更换 age 口令并重新签发。
      </MiuixText>

      <MiuixSmallTitle text="同步策略" />
      <div class="sheet-group">
        <MiuixRadioButtonPreference
          v-for="p in POLICIES"
          :key="p"
          :title="IDENTITY_POLICY_LABEL[p]"
          :model-value="form.policy === p"
          @select="form.policy = p"
        />
      </div>
      <div class="sheet-group sheet-group--form">
        <ReceiverField
          v-model="form.lead"
          label="轮换提前天数（rotation_lead_days）"
          hint="仅 scheduled：旧 keybox 距离到期不超过该天数时才允许自动换身份。"
          numeric
        />
        <ReceiverField
          v-model="form.minValidity"
          label="最短剩余有效期（min_validity_days）"
          hint="新 keybox 剩余有效期低于该值时拒绝拉取。"
          numeric
        />
        <MiuixSwitchPreference
          v-model="form.autoDeploy"
          title="自动部署新版本"
          summary="auto_deploy：后台循环拉取后自动写入目标；首次部署与策略外的身份变化仍会暂停待确认。"
        />
        <ReceiverField v-model="form.label" label="标签（label）" placeholder="例如 phone-a" />
        <ReceiverField
          v-model="form.proxy"
          label="代理（proxy）"
          placeholder="例如 http://127.0.0.1:7890"
          hint="留空表示直连；只接受 https 远端地址。"
        />
      </div>
      <MiuixButton type="primary" :disabled="saving || loading" @click="save">
        {{ saving ? '保存中…' : '保存策略与显示设置' }}
      </MiuixButton>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        只通过 config set 的白名单写入；未知值会在写入前拒绝并回滚。
      </MiuixText>

      <MiuixSmallTitle text="密钥" />
      <div class="sheet-group">
        <MiuixBasicComponent
          title="age 口令"
          :summary="secretText(status?.secrets.age)"
          clickable
          @click="emit('secret', 'age')"
        >
          <template #end>
            <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          v-if="webdavNeeded"
          title="WebDAV 应用密码"
          :summary="secretText(status?.secrets.webdav_password)"
          clickable
          @click="emit('secret', 'webdav-password')"
        >
          <template #end>
            <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          v-if="config"
          title="口令模式"
          :summary="config.password_mode === 'secret-file' ? 'secret-file：0600 文件，支持自动运行' : 'prompt：仅手动输入，不支持自动运行'"
          :summary-color="config.password_mode === 'secret-file' ? undefined : 'var(--app-warning)'"
        />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        只显示“已设置/缺失/权限异常”，从不读取或回显密钥内容；age 口令必须来自与链接不同且安全可信的渠道。
      </MiuixText>

      <MiuixSmallTitle text="位置" />
      <div class="sheet-group">
        <MiuixBasicComponent
          title="导入 / 更换同步链接"
          :summary="status?.stream_id || '尚未导入'"
          clickable
          @click="emit('import')"
        >
          <template #end>
            <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
      </div>

      <MiuixSmallTitle text="本地数据" />
      <div class="sheet-group">
        <MiuixCheckboxPreference
          v-model="forgetStaging"
          title="同时删除暂存的 keybox"
          summary="staging/ 中的明文 keybox；已部署到 OMK/TEESim 的文件不受影响。"
        />
      </div>
      <MiuixButton :disabled="busy || loading" @click="forgetOpen = true">忘记同步数据…</MiuixButton>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        删除 age 口令、WebDAV 密码、sync/deploy 状态与位置信息并禁用同步；远端同步文件不会被删除。不清除已部署的 keybox。
      </MiuixText>

      <MiuixSmallTitle text="同步日志" />
      <div class="sheet-group">
        <FoldBlock v-if="logText" title="logs/sync.log" :summary="SYNC_LOG" :text="logText" />
        <MiuixBasicComponent v-else :title="logError ? '读取日志失败' : logLoading ? '正在读取…' : '暂无日志'" :summary="logError ?? SYNC_LOG" :summary-color="logError ? 'var(--m-color-error)' : undefined" />
      </div>
      <MiuixButton :disabled="logLoading" @click="loadLog">{{ logLoading ? '读取中…' : '刷新日志' }}</MiuixButton>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        日志只包含 provider、seq、短哈希与错误码；不含 keybox、私钥或任何密钥。
      </MiuixText>

      <MiuixSmallTitle text="模块与路径" />
      <div class="paths">
        <div v-for="p in paths" :key="p.label" class="path">
          <span class="path__k">{{ p.label }}</span>
          <span class="path__v selectable">{{ p.value }}</span>
        </div>
      </div>
    </div>

    <MiuixDialog v-model="forgetOpen" title="忘记本地同步数据" :close-on-click-modal="false">
      <template #default="{ close }">
        <div class="dialog">
          <MiuixText type="body2" color="var(--m-color-error)">
            将删除 age 口令、WebDAV 密码、sync/deploy 状态，并清空同步流与位置信息；同时禁用同步。
          </MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            远端同步文件不会被删除。已部署到 OMK/TEESim 的 keybox 也不会被删除；如需移除，请在“部署目标”中操作。
          </MiuixText>
          <MiuixCheckboxPreference
            v-model="forgetStaging"
            title="同时删除暂存的 keybox"
            summary="staging/ 中的明文 keybox 会被删除。"
          />
          <div class="dialog__actions">
            <MiuixButton @click="close">取消</MiuixButton>
            <MiuixButton type="primary" :disabled="forgetBusy" @click="confirmForget">
              {{ forgetBusy ? '删除中…' : '确认删除' }}
            </MiuixButton>
          </div>
        </div>
      </template>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.center {
  display: flex;
  justify-content: center;
  padding: 24px;
}
.sheet-group--form {
  display: flex;
  flex-direction: column;
}
.paths {
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  overflow: hidden;
}
.path {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 9px 16px;
  border-top: 1px solid color-mix(in srgb, var(--m-color-on-surface) 6%, transparent);
}
.path:first-child {
  border-top: none;
}
.path__k {
  font-size: var(--m-text-footnote1-size);
  color: var(--m-color-on-surface-variant-summary);
}
.path__v {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  color: var(--m-color-on-surface);
}
.dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 8px;
}
.dialog__actions {
  display: flex;
  gap: 10px;
}
.dialog__actions > * {
  flex: 1;
}
</style>
