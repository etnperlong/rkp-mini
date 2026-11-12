<script setup lang="ts">
// 部署设置 (§13.3, §19.4): policy fields through the `config set` allow-list,
// the configured deploy targets (add only modules detected as installed), the
// cloud location entry points, the password entry point (presence is never
// shown), and the two red danger actions behind confirmation dialogs.
// The page never reads config.toml, a secret file or a keybox itself.
import { computed, reactive, ref, watch } from 'vue'
import {
  MiuixBasicComponent,
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixDialog,
  MiuixIcon,
  MiuixIconButton,
  MiuixInput,
  MiuixProgressIndicator,
  MiuixRadioButtonPreference,
  MiuixSmallTitle,
  MiuixSwitchPreference,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { ChevronForward, Close } from 'miuix-vue/icons'
import { CommandError } from '../../../shared/lib/command'
import {
  IDENTITY_POLICY_LABEL,
  TARGET_LABEL,
  forgetSync,
  readSyncConfig,
  setConfig,
  type IdentityPolicy,
  type ReceiverConfigKey,
  type SecretKind,
  type SyncConfig,
  type SyncStatus,
} from '../../../shared/lib/sync'
import {
  addTarget,
  isTeesimFileName,
  readBuiltinTargets,
  removeDeployed,
  removeTarget,
  teesimFileName,
  type BuiltinTarget,
  type BuiltinTargetInfo,
} from '../lib/deploy'
import { deployErrorText, errorMessage, providerLabel } from '../lib/display'
import { truncateMiddle } from '../../../shared/lib/format'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{
  status: SyncStatus | null
  /** A sync/deploy task is running: no command may start. */
  busy: boolean
}>()
const emit = defineEmits<{ secret: [kind: SecretKind]; import: []; changed: [] }>()

const POLICIES: IdentityPolicy[] = ['scheduled', 'always', 'never']

const cfg = ref<SyncConfig | null>(null)
/** Module detection; `null` (failed) allows no target selection at all. */
const builtins = ref<BuiltinTargetInfo[] | null>(null)
const detectError = ref<string | null>(null)
const loading = ref(false)
const loadError = ref<string | null>(null)
const saving = ref(false)
const busyId = ref<string | null>(null)
const actionError = ref<string | null>(null)

const draft = reactive({
  auto_deploy: true,
  min_validity_days: '2',
  policy: 'scheduled' as IdentityPolicy,
  rotation_lead_days: '4',
  label: '',
  proxy: '',
})

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : errorMessage(e)
}

function fill(config: SyncConfig): void {
  draft.auto_deploy = config.auto_deploy ?? true
  draft.min_validity_days = String(config.min_validity_days)
  draft.policy = config.identity_change_policy ?? 'scheduled'
  draft.rotation_lead_days = String(config.rotation_lead_days ?? 4)
  draft.label = config.label
  draft.proxy = config.proxy
}

async function load(): Promise<void> {
  loading.value = true
  loadError.value = null
  actionError.value = null
  const [config, detected] = await Promise.allSettled([readSyncConfig(), readBuiltinTargets()])
  if (config.status === 'fulfilled') {
    cfg.value = config.value
    fill(config.value)
  } else {
    cfg.value = null
    loadError.value = message(config.reason)
  }
  if (detected.status === 'fulfilled') {
    builtins.value = detected.value
    detectError.value = null
  } else {
    builtins.value = null
    detectError.value = message(detected.reason)
  }
  loading.value = false
}

watch(open, (v) => {
  if (v) void load()
})

const configured = computed(() => cfg.value?.targets ?? [])

/** Builtin targets not configured yet and whose module is installed. */
const addable = computed(() => {
  const ids = new Set(configured.value.map((t) => t.id))
  return (builtins.value ?? []).filter((b) => b.module.installed && !ids.has(b.id))
})

/** TEESimulator reads a configurable `*.xml` file directly inside /data/adb/teesim. */
const teesimName = ref('keybox.xml')
const addingTeesim = computed(() => addable.value.some((b) => b.id === 'teesim'))
const teesimNameError = computed(() =>
  addingTeesim.value && !isTeesimFileName(teesimName.value)
    ? '文件名需为 /data/adb/teesim 下的 .xml 文件名（1–64 个字母、数字、. _ -，且不以 . 开头）'
    : null,
)

const teesimTarget = computed(() => configured.value.find((t) => t.id === 'teesim') ?? null)
const renameOpen = ref(false)
const renameName = ref('')
const renameBusy = ref(false)
const renameError = computed(() => (isTeesimFileName(renameName.value) ? null : '文件名无效'))

function openRename(): void {
  const current = teesimFileName(teesimTarget.value?.path)
  renameName.value = current === '—' ? 'keybox.xml' : current
  renameBusy.value = false
  renameOpen.value = true
}

/** Changing the name is a remove + add: the old file stays until it is removed. */
async function confirmRename(): Promise<void> {
  if (renameBusy.value || locked.value || !isTeesimFileName(renameName.value)) return
  renameBusy.value = true
  actionError.value = null
  try {
    await removeTarget('teesim')
    await addTarget('teesim', renameName.value)
    renameOpen.value = false
    await load()
    emit('changed')
    void showSnackbar({ message: '已更新 TEESimulator 文件名；下一次部署写入该文件' })
  } catch (e) {
    actionError.value = message(e)
  } finally {
    renameBusy.value = false
  }
}

const webdavNeeded = computed(() => cfg.value?.provider === 'webdav' && !!cfg.value.webdav_user)
const locationSummary = computed(() => {
  const s = props.status
  if (!s?.stream_id) return '尚未导入'
  return `${providerLabel(s.provider)}${s.location ? ` · ${truncateMiddle(s.location, 16)}` : ''}`
})

async function save(): Promise<void> {
  const current = cfg.value
  if (!current || locked.value) return
  actionError.value = null

  const minDays = /^\d+$/.test(draft.min_validity_days.trim()) ? Number(draft.min_validity_days) : NaN
  const leadDays = /^\d+$/.test(draft.rotation_lead_days.trim()) ? Number(draft.rotation_lead_days) : NaN
  if (!Number.isSafeInteger(minDays)) {
    actionError.value = '最短剩余有效期必须是非负整数'
    return
  }
  if (!Number.isSafeInteger(leadDays)) {
    actionError.value = '轮换提前天数必须是非负整数'
    return
  }

  const updates: [ReceiverConfigKey, string | number | boolean][] = []
  if (draft.auto_deploy !== (current.auto_deploy ?? true)) updates.push(['auto_deploy', draft.auto_deploy])
  if (minDays !== current.min_validity_days) updates.push(['min_validity_days', minDays])
  if (draft.policy !== (current.identity_change_policy ?? 'scheduled')) {
    updates.push(['identity_change_policy', draft.policy])
  }
  if (leadDays !== (current.rotation_lead_days ?? 4)) updates.push(['rotation_lead_days', leadDays])
  if (draft.label !== current.label) updates.push(['label', draft.label])
  if (draft.proxy !== current.proxy) updates.push(['proxy', draft.proxy])

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

async function add(id: BuiltinTarget): Promise<void> {
  // The button is disabled for these; never rely on that alone.
  if (!builtins.value?.some((b) => b.id === id && b.module.installed)) return
  if (id === 'teesim' && !isTeesimFileName(teesimName.value)) return
  busyId.value = id
  actionError.value = null
  try {
    await addTarget(id, id === 'teesim' ? teesimName.value : undefined)
    await load()
    emit('changed')
    void showSnackbar({ message: `已添加部署目标 ${TARGET_LABEL[id] ?? id}` })
  } catch (e) {
    actionError.value = message(e)
  } finally {
    busyId.value = null
  }
}

async function drop(id: string): Promise<void> {
  busyId.value = id
  actionError.value = null
  try {
    await removeTarget(id)
    await load()
    emit('changed')
    void showSnackbar({ message: '已从配置移除；已部署的文件未被删除' })
  } catch (e) {
    actionError.value = message(e)
  } finally {
    busyId.value = null
  }
}

type DangerAction = 'remove' | 'forget'

const danger = ref<DangerAction | null>(null)
const pending = ref<DangerAction | null>(null)
const forgetStaging = ref(false)

const locked = computed(() => props.busy || saving.value || busyId.value !== null || danger.value !== null)

const dialogOpen = computed({
  get: () => danger.value !== null,
  set: (v: boolean) => {
    if (!v) danger.value = null
  },
})

/** Deployed paths listed in the remove dialog (configured targets otherwise). */
const deployedPaths = computed(() => {
  const deployed = (props.status?.deploy_state?.targets ?? [])
    .filter((t) => !!t.deployed_sha256)
    .map((t) => t.path)
  if (deployed.length) return deployed
  return configured.value.map((t) => t.path ?? '—')
})

const DIALOG: Record<DangerAction, { title: string; summary: string; ok: string }> = {
  remove: {
    title: '移除已部署的 keybox？',
    summary: '',
    ok: '移除 keybox',
  },
  forget: {
    title: '清除本地数据？',
    summary:
      '会删除本地同步状态、密码与本机导入的链接，页面回到未导入。不会删除已部署的 keybox，也不会删除云端文件。',
    ok: '确认清除',
  },
}

const dialog = computed(() => DIALOG[danger.value ?? 'forget'])
const dialogSummary = computed(() => {
  if (danger.value !== 'remove') return dialog.value.summary
  if (!deployedPaths.value.length) return '当前没有已部署的 keybox。'
  return `将删除以下目标的 keybox：${deployedPaths.value.join('、')}。对应模块会失去 keybox，直到下一次部署。`
})

async function runDanger(): Promise<void> {
  const action = danger.value
  if (!action || pending.value || props.busy) return
  actionError.value = null
  pending.value = action
  try {
    if (action === 'remove') {
      const call = await removeDeployed([])
      if (!call.envelope.ok) {
        throw new CommandError('exit', deployErrorText(call.envelope.error), JSON.stringify(call.envelope.error))
      }
      const ok = call.envelope.data.targets.filter((t) => t.ok).length
      void showSnackbar({ message: ok ? `已移除 ${ok} 个目标的 keybox` : '没有可移除的 keybox' })
      danger.value = null
      emit('changed')
      await load()
    } else {
      await forgetSync(forgetStaging.value)
      danger.value = null
      open.value = false
      void showSnackbar({ message: '本地数据已清除（云端文件与已部署 keybox 未删除）' })
      emit('changed')
    }
  } catch (e) {
    actionError.value = message(e)
  } finally {
    pending.value = null
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="部署设置">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        这里只修改 [sync] 表中的非敏感字段；密码经 root shell 写入 0600 文件后由 rkp-mini-sync 接管，页面不会读回。
      </MiuixText>

      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>
      <MiuixText v-else-if="loadError" type="body2" color="var(--m-color-error)">读取配置失败：{{ loadError }}</MiuixText>

      <template v-else-if="cfg">
        <MiuixSmallTitle text="自动部署" />
        <div class="sheet-group">
          <MiuixSwitchPreference
            v-model="draft.auto_deploy"
            title="自动部署新版本"
            summary="后台循环拉取后自动写入目标；首次部署与策略外的身份变化仍会暂停待确认"
            :disabled="locked"
          />
          <MiuixInput v-model="draft.min_validity_days" label="最短剩余有效期（天）" single-line :disabled="locked" />
          <MiuixRadioButtonPreference
            v-for="p in POLICIES"
            :key="p"
            :title="IDENTITY_POLICY_LABEL[p]"
            :model-value="draft.policy === p"
            :disabled="locked"
            @select="draft.policy = p"
          />
          <MiuixInput
            v-if="draft.policy === 'scheduled'"
            v-model="draft.rotation_lead_days"
            label="轮换提前天数（天）"
            single-line
            :disabled="locked"
          />
        </div>

        <MiuixSmallTitle text="部署目标" />
        <div class="sheet-group">
          <div v-if="!configured.length" class="empty">尚未配置部署目标；部署不会写入任何位置。</div>
          <div v-for="t in configured" :key="t.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ TARGET_LABEL[t.id] ?? t.id }}</span>
              <span class="target__actions">
                <MiuixButton
                  v-if="t.id === 'teesim'"
                  :disabled="locked || busyId === t.id"
                  @pointerdown.stop
                  @click.stop="openRename"
                >
                  更改文件名
                </MiuixButton>
                <MiuixButton :disabled="locked || busyId === t.id" @pointerdown.stop @click.stop="drop(t.id)">
                  移除
                </MiuixButton>
              </span>
            </div>
            <span class="target__path selectable">{{ t.path ?? '—' }}</span>
            <span v-if="t.id === 'teesim'" class="target__note">文件名 {{ teesimFileName(t.path) }}</span>
          </div>
          <template v-if="detectError || addable.length">
            <div v-if="detectError" class="empty">无法检测已安装的模块，暂不能添加目标：{{ detectError }}</div>
            <div v-for="b in addable" :key="b.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ TARGET_LABEL[b.id] ?? b.id }}</span>
                <MiuixButton
                  :disabled="locked || busyId === b.id || (b.id === 'teesim' && !isTeesimFileName(teesimName))"
                  @pointerdown.stop
                  @click.stop="add(b.id)"
                >
                  添加
                </MiuixButton>
              </div>
              <span class="target__path selectable">{{ b.path }}</span>
              <span class="target__note">
                检测到模块 {{ b.module.name ?? b.module.id }}{{ b.module.version ? ` ${b.module.version}` : '' }}
              </span>
              <template v-if="b.id === 'teesim'">
                <MiuixInput v-model="teesimName" label="文件名" single-line :disabled="locked" />
                <span class="target__note">
                  需与 TEESimulator 配置（config.json）中使用的 keybox 文件名一致；只接受 /data/adb/teesim 下的 .xml
                  文件。
                </span>
                <span v-if="teesimNameError" class="target__error">{{ teesimNameError }}</span>
              </template>
            </div>
          </template>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          目标路径、owner、权限与 reload 全部来自本机配置；只有检测到已安装模块的内置目标才能添加。
        </MiuixText>

        <MiuixSmallTitle text="云端存储" />
        <div class="sheet-group">
          <MiuixBasicComponent title="同步链接" :summary="locationSummary" clickable :disabled="locked" @click="emit('import')">
            <template #end>
              <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
            </template>
          </MiuixBasicComponent>
          <MiuixBasicComponent
            v-if="webdavNeeded"
            title="WebDAV 密码"
            summary="建议使用可单独撤销的应用密码"
            clickable
            :disabled="locked"
            @click="emit('secret', 'webdav-password')"
          />
          <MiuixInput v-model="draft.proxy" label="代理（可留空）" single-line :disabled="locked" />
        </div>

        <MiuixSmallTitle text="加密" />
        <div class="sheet-group">
          <MiuixBasicComponent
            title="密码"
            summary="发送端设置的密码，用来解密云端储存的 Keybox"
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
            title="移除已部署的 keybox"
            summary="删除所有目标中由同步写入的 keybox 文件"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'remove'"
          />
          <MiuixBasicComponent
            title="清除本地数据"
            summary="删除本地状态、密码与导入的链接；云端文件与已部署 keybox 不删除"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'forget'"
          />
        </div>
      </template>
    </div>

    <MiuixDialog v-model="dialogOpen" :title="dialog.title" :summary="dialogSummary">
      <div class="dialog">
        <MiuixCheckboxPreference
          v-if="danger === 'forget'"
          v-model="forgetStaging"
          title="同时删除暂存的 keybox"
          summary="staging/ 中的明文 keybox 会被删除。"
          :disabled="pending !== null"
        />
        <div class="dialog-actions">
          <MiuixButton @click="dialogOpen = false">取消</MiuixButton>
          <MiuixButton class="danger-btn" type="primary" :disabled="pending !== null || busy" @click="runDanger">
            {{ pending ? '处理中…' : dialog.ok }}
          </MiuixButton>
        </div>
      </div>
    </MiuixDialog>

    <MiuixDialog
      v-model="renameOpen"
      title="更改 TEESimulator 文件名"
      summary="旧文件会留在 /data/adb/teesim，直到手动删除；新文件名会在下一次部署时写入。需与 config.json 中使用的 keybox 文件名一致。"
    >
      <div class="dialog">
        <MiuixInput v-model="renameName" label="文件名" single-line :disabled="renameBusy" />
        <MiuixText v-if="renameError" type="footnote1" color="var(--m-color-error)">{{ renameError }}</MiuixText>
        <div class="dialog-actions">
          <MiuixButton @click="renameOpen = false">取消</MiuixButton>
          <MiuixButton type="primary" :disabled="renameBusy || renameError !== null" @click="confirmRename">
            {{ renameBusy ? '处理中…' : '确认更改' }}
          </MiuixButton>
        </div>
      </div>
    </MiuixDialog>
  </MiuixBottomSheet>
</template>

<style scoped>
.center {
  display: flex;
  justify-content: center;
  padding: 24px;
}
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.empty {
  padding: 12px 16px;
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-body2-size);
}
.target {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 16px;
  border-top: 1px solid color-mix(in srgb, var(--m-color-on-surface) 6%, transparent);
}
.target:first-child {
  border-top: none;
}
.target__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.target__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.target__error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
}
.target__name {
  font-weight: 500;
}
.target__path {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  color: var(--m-color-on-surface-variant-summary);
}
.target__note {
  color: var(--m-color-on-surface-variant-actions);
  font-size: var(--m-text-footnote1-size);
}
.dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.danger-group {
  background: color-mix(in srgb, var(--m-color-error) 8%, var(--app-sheet-group));
}
.danger-btn.m-button--primary:not(.m-button--disabled) {
  background: var(--m-color-error);
  color: #fff;
}
</style>
