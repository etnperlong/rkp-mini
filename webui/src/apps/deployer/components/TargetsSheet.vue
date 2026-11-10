<script setup lang="ts">
// Deployment targets (§11.4, §13.3). Config targets come from the local
// config only; a builtin target can be added only while the module that
// reads its keybox is detected as installed. Removing a deployed keybox is
// destructive and needs the dialog plus typing the exact target id.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixDialog,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixSmallTitle,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Add, Close } from 'miuix-vue/icons'
import ReceiverField from './ReceiverField.vue'
import StatusPill from '../../../shared/components/StatusPill.vue'
import { deployCodeLabel, deployErrorText, errorMessage } from '../lib/display'
import {
  addTarget,
  readBuiltinTargets,
  removeDeployed,
  removeTarget,
  type BuiltinTarget,
  type BuiltinTargetInfo,
  type TargetModule,
} from '../lib/deploy'
import {
  TARGET_LABEL,
  readSyncConfig,
  type SyncConfig,
  type TargetReport,
  type TargetState,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ deployed: TargetState[] }>()
const emit = defineEmits<{ changed: [] }>()

const config = ref<SyncConfig | null>(null)
/** Module detection; `null` (unknown or failed) allows no selection at all. */
const builtins = ref<BuiltinTargetInfo[] | null>(null)
const detectError = ref<string | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const busyId = ref<string | null>(null)

const removeOpen = ref(false)
const removeId = ref('')
const removePath = ref('')
const typed = ref('')
const removing = ref(false)
const removeError = ref<string | null>(null)
const removeReports = ref<TargetReport[] | null>(null)

const NOTES: string[] = [
  'OhMyKeymint（模块 oh_my_keymint）：写入 /data/misc/keystore/omk/keybox.xml，owner 1017:1017；模块监听该目录，文件替换后立即重载，无需重启。若开启 retire_stale_keybox_bound_keys（默认 true），更换 keybox 会删除绑定旧 keybox 的客户端密钥。',
  'TEESimulator（模块 teesim）：写入 /data/adb/teesim/ 下的 .xml 文件，文件名必须被 config.json 中的某个 profile 引用，且只接受 EC keybox；替换后立即重载，并为已有密钥重新生成证明。',
  'TrickyStore / TEESimulator-RS（两者的模块 id 都是 tricky_store）：写入 /data/adb/tricky_store/keybox.xml；替换后立即重载，但替换前已生成的密钥可能沿用旧证书链，直到该模块的守护进程重启。模块覆盖安装时该文件可能被替换。',
]

watch(open, (v) => {
  if (!v) return
  removeReports.value = null
  removeError.value = null
  busyId.value = null
  void load()
})

async function load(): Promise<void> {
  loading.value = true
  error.value = null
  detectError.value = null
  const [conf, detected] = await Promise.allSettled([readSyncConfig(), readBuiltinTargets()])
  if (conf.status === 'fulfilled') {
    config.value = conf.value
  } else {
    config.value = null
    error.value = errorMessage(conf.reason)
  }
  if (detected.status === 'fulfilled') {
    builtins.value = detected.value
  } else {
    builtins.value = null
    detectError.value = errorMessage(detected.reason)
  }
  loading.value = false
}

const deployedById = computed(() => new Map(props.deployed.map((t) => [t.id, t])))

function ownerOf(id: string, owner: string | null | undefined): string {
  return owner ?? (id === 'omk' ? '1017:1017' : '0:0')
}

const rows = computed(() => {
  const configured = config.value?.targets ?? []
  const ids = new Set<string>([...configured.map((t) => t.id), ...props.deployed.map((t) => t.id)])
  return [...ids].map((id) => {
    const conf = configured.find((t) => t.id === id)
    const dep = deployedById.value.get(id)
    return {
      id,
      label: TARGET_LABEL[id] ?? id,
      path: conf?.path ?? dep?.path ?? '—',
      owner: ownerOf(id, conf?.owner ?? dep?.owner),
      mode: conf?.mode ?? dep?.mode ?? '0600',
      reload: conf?.reload ?? dep?.reload ?? 'auto',
      configured: configured.some((t) => t.id === id),
      deployed: !!dep?.deployed_sha256,
    }
  })
})

/** Builtin targets not yet in the config, with the state of their module. */
const addable = computed(() => {
  const ids = new Set((config.value?.targets ?? []).map((t) => t.id))
  return (builtins.value ?? []).filter((b) => !ids.has(b.id))
})

function moduleTone(m: TargetModule): 'ok' | 'warning' | 'neutral' {
  if (!m.installed) return 'neutral'
  return m.enabled ? 'ok' : 'warning'
}

function moduleLabel(m: TargetModule): string {
  if (!m.installed) return '未安装'
  return m.enabled ? '已安装' : '已停用'
}

function moduleLine(m: TargetModule): string {
  if (!m.installed) return `未检测到模块 ${m.id}（/data/adb/modules/${m.id}）；安装并重启后才能选择。`
  const found = `检测到模块 ${m.name ?? m.id}${m.version ? ` ${m.version}` : ''}（${m.id}）`
  return m.enabled ? found : `${found}；模块已停用或待卸载，启用前不会重载 keybox。`
}

async function add(id: BuiltinTarget): Promise<void> {
  // The button is disabled for these; never rely on that alone.
  if (!builtins.value?.some((b) => b.id === id && b.module.installed)) return
  busyId.value = id
  error.value = null
  try {
    await addTarget(id)
    await load()
    emit('changed')
    void showSnackbar({ message: `已添加部署目标 ${TARGET_LABEL[id] ?? id}` })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busyId.value = null
  }
}

async function removeFromConfig(id: string): Promise<void> {
  busyId.value = id
  error.value = null
  try {
    await removeTarget(id)
    await load()
    emit('changed')
    void showSnackbar({ message: '已从配置移除；已部署的文件未被删除' })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busyId.value = null
  }
}

function startRemove(id: string, path: string): void {
  removeId.value = id
  removePath.value = path
  typed.value = ''
  removeError.value = null
  removeReports.value = null
  removeOpen.value = true
}

async function confirmRemove(): Promise<void> {
  if (typed.value.trim() !== removeId.value || removing.value) return
  const id = removeId.value
  removing.value = true
  removeError.value = null
  try {
    const call = await removeDeployed([id])
    if (call.envelope.ok) {
      removeReports.value = call.envelope.data.targets
      removeOpen.value = false
    } else {
      const data = call.envelope.data
      if (typeof data === 'object' && data !== null && Array.isArray((data as { targets?: unknown }).targets)) {
        removeReports.value = (data as { targets: TargetReport[] }).targets
      }
      removeError.value = deployErrorText(call.envelope.error)
      removeOpen.value = false
    }
    emit('changed')
  } catch (e) {
    removeError.value = errorMessage(e)
    removeOpen.value = false
  } finally {
    removing.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="部署目标">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        目标路径、owner、权限与 reload 全部来自本机配置；远端链接与密文不能指定任何路径或命令。
      </MiuixText>

      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <template v-if="config">
        <MiuixSmallTitle text="已配置" />
        <div v-if="!rows.length" class="empty">尚未配置任何目标；部署不会写入任何位置。</div>
        <div v-else class="targets">
          <div v-for="t in rows" :key="t.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ t.label }}</span>
              <span class="target__meta">{{ t.owner }} · {{ t.mode }} · reload {{ t.reload }}</span>
            </div>
            <span class="target__path selectable">{{ t.path }}</span>
            <span class="target__state">
              {{ t.deployed ? '已部署 keybox' : '尚未部署 keybox' }}<template v-if="!t.configured">（已从配置移除，部署状态仍在）</template>
            </span>
            <div class="target__actions">
              <MiuixButton v-if="t.configured" :disabled="busyId === t.id" @click="removeFromConfig(t.id)">
                从配置移除
              </MiuixButton>
              <MiuixButton v-if="t.deployed" type="primary" :disabled="busyId === t.id" @click="startRemove(t.id, t.path)">
                移除已部署的 keybox…
              </MiuixButton>
            </div>
          </div>
        </div>

        <template v-if="removeReports">
          <MiuixSmallTitle text="移除结果" />
          <div class="targets">
            <div v-for="r in removeReports" :key="r.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ TARGET_LABEL[r.id] ?? r.id }}</span>
                <span class="target__state">{{ r.ok ? '已删除' : '未删除' }}</span>
              </div>
              <span class="target__path selectable">{{ r.path }}</span>
              <span v-if="r.error" class="target__error">
                {{ deployCodeLabel(r.error.code) }}（{{ r.error.code }}）：{{ r.error.message }}
              </span>
            </div>
          </div>
        </template>
        <MiuixText v-if="removeError" type="footnote1" color="var(--m-color-error)">{{ removeError }}</MiuixText>

        <template v-if="detectError || addable.length">
          <MiuixSmallTitle text="添加内置目标" />
          <MiuixText v-if="detectError" type="footnote1" color="var(--m-color-error)">
            无法检测已安装的模块，暂不能添加目标：{{ detectError }}
          </MiuixText>
          <div v-else class="targets">
            <div v-for="b in addable" :key="b.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ TARGET_LABEL[b.id] ?? b.id }}</span>
                <StatusPill :tone="moduleTone(b.module)" :label="moduleLabel(b.module)" />
              </div>
              <span class="target__path selectable">{{ b.path }}</span>
              <span class="target__state">{{ moduleLine(b.module) }}</span>
              <div class="target__actions">
                <MiuixButton :disabled="!b.module.installed || busyId === b.id" @click="add(b.id)">
                  <MiuixIcon :icon="Add" :size="18" />
                  <span>添加</span>
                </MiuixButton>
              </div>
            </div>
          </div>
        </template>

        <MiuixSmallTitle text="注意事项" />
        <ul class="notes">
          <li v-for="note in NOTES" :key="note">{{ note }}</li>
        </ul>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          “从配置移除”只改配置，不动已写入的文件；“移除已部署的 keybox”会删除目标文件，且需要输入目标 id 二次确认。两者都不会删除远端同步文件。
        </MiuixText>
      </template>
    </div>

    <MiuixDialog v-model="removeOpen" title="移除已部署的 keybox" :close-on-click-modal="false">
      <template #default="{ close }">
        <div class="dialog">
          <MiuixText type="body2" color="var(--m-color-error)">
            将删除目标 {{ removeId }} 中由同步部署的 keybox 文件：{{ removePath }}。对应模块可能回到无 keybox 状态。
          </MiuixText>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
            远端同步文件、age 口令与暂存 keybox 不受影响。删除后如需恢复，必须重新“部署”（并再次确认）。
          </MiuixText>
          <ReceiverField
            v-model="typed"
            :label="`输入目标 id（${removeId}）以确认`"
            placeholder="目标 id"
            :mono="true"
            :disabled="removing"
          />
          <div class="dialog__actions">
            <MiuixButton @click="close">取消</MiuixButton>
            <MiuixButton type="primary" :disabled="removing || typed.trim() !== removeId" @click="confirmRemove">
              {{ removing ? '删除中…' : '删除 keybox' }}
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
.empty {
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-body2-size);
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
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
}
.target__head {
  display: flex;
  align-items: baseline;
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
.target__state {
  color: var(--m-color-on-surface-variant-actions);
  font-size: var(--m-text-footnote1-size);
}
.target__error {
  color: var(--m-color-error);
  font-size: var(--m-text-footnote1-size);
  word-break: break-word;
}
.target__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 6px;
}
.notes {
  margin: 0;
  padding-inline-start: 20px;
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
  line-height: 1.6;
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
