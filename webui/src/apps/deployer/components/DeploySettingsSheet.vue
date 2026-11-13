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
  identityPolicyLabel,
  targetLabel,
  forgetSync,
  readSyncConfig,
  setConfig,
  type IdentityPolicy,
  type ReceiverConfigKey,
  type SecretKind,
  type SyncConfig,
  type SyncStatus,
} from '../../../shared/lib/sync'
import { t, type MessageKey } from '../../../shared/lib/i18n'
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
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : errorMessage(e)
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
  addingTeesim.value && !isTeesimFileName(teesimName.value) ? t('deployer.settings.teesimNameRule') : null,
)

const teesimTarget = computed(() => configured.value.find((t) => t.id === 'teesim') ?? null)
const renameOpen = ref(false)
const renameName = ref('')
const renameBusy = ref(false)
const renameError = computed(() =>
  isTeesimFileName(renameName.value) ? null : t('deployer.settings.fileNameInvalid'),
)

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
    void showSnackbar({ message: t('deployer.settings.renamed') })
  } catch (e) {
    actionError.value = message(e)
  } finally {
    renameBusy.value = false
  }
}

const webdavNeeded = computed(() => cfg.value?.provider === 'webdav' && !!cfg.value.webdav_user)
const locationSummary = computed(() => {
  const s = props.status
  if (!s?.stream_id) return t('deployer.settings.notImported')
  return `${providerLabel(s.provider)}${s.location ? ` · ${truncateMiddle(s.location, 16)}` : ''}`
})

async function save(): Promise<void> {
  const current = cfg.value
  if (!current || locked.value) return
  actionError.value = null

  const minDays = /^\d+$/.test(draft.min_validity_days.trim()) ? Number(draft.min_validity_days) : NaN
  const leadDays = /^\d+$/.test(draft.rotation_lead_days.trim()) ? Number(draft.rotation_lead_days) : NaN
  if (!Number.isSafeInteger(minDays)) {
    actionError.value = t('deployer.settings.badMinValidity')
    return
  }
  if (!Number.isSafeInteger(leadDays)) {
    actionError.value = t('deployer.settings.badRotationLead')
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
    void showSnackbar({ message: t('deployer.settings.noChanges') })
    return
  }

  saving.value = true
  try {
    for (const [key, value] of updates) await setConfig(key, value)
    void showSnackbar({ message: t('deployer.settings.saved') })
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
    void showSnackbar({ message: t('deployer.settings.addedTarget', { name: targetLabel(id) }) })
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
    void showSnackbar({ message: t('deployer.settings.removedTarget') })
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

const DIALOG: Record<DangerAction, { title: MessageKey; ok: MessageKey }> = {
  remove: {
    title: 'deployer.settings.dialogRemoveTitle',
    ok: 'deployer.settings.dialogRemoveOk',
  },
  forget: {
    title: 'deployer.settings.dialogForgetTitle',
    ok: 'deployer.settings.dialogForgetOk',
  },
}

const dialog = computed(() => DIALOG[danger.value ?? 'forget'])
const dialogSummary = computed(() => {
  if (danger.value !== 'remove') return t('deployer.settings.dialogForgetSummary')
  if (!deployedPaths.value.length) return t('deployer.settings.removeNone')
  return t('deployer.settings.removeList', { paths: deployedPaths.value.join(t('deployer.settings.listSeparator')) })
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
      const ok = call.envelope.data.targets.filter((x) => x.ok).length
      void showSnackbar({
        message: ok ? t('deployer.settings.removeDone', { n: ok }, ok) : t('deployer.settings.removeNothing'),
      })
      danger.value = null
      emit('changed')
      await load()
    } else {
      await forgetSync(forgetStaging.value)
      danger.value = null
      open.value = false
      void showSnackbar({ message: t('deployer.settings.forgot') })
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
  <MiuixBottomSheet v-model="open" :title="t('deployer.app.settings')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('deployer.settings.intro') }}
      </MiuixText>

      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>
      <MiuixText v-else-if="loadError" type="body2" color="var(--m-color-error)">{{ t('deployer.settings.loadFailed', { error: loadError }) }}</MiuixText>

      <template v-else-if="cfg">
        <MiuixSmallTitle :text="t('deployer.common.autoDeploy')" />
        <div class="sheet-group">
          <MiuixSwitchPreference
            v-model="draft.auto_deploy"
            :title="t('deployer.settings.autoDeployNew')"
            :summary="t('deployer.settings.autoDeploySummary')"
            :disabled="locked"
          />
          <MiuixInput v-model="draft.min_validity_days" :label="t('deployer.settings.minValidity')" single-line :disabled="locked" />
          <MiuixRadioButtonPreference
            v-for="p in POLICIES"
            :key="p"
            :title="identityPolicyLabel(p)"
            :model-value="draft.policy === p"
            :disabled="locked"
            @select="draft.policy = p"
          />
          <MiuixInput
            v-if="draft.policy === 'scheduled'"
            v-model="draft.rotation_lead_days"
            :label="t('deployer.settings.rotationLead')"
            single-line
            :disabled="locked"
          />
        </div>

        <MiuixSmallTitle :text="t('deployer.common.deployTargets')" />
        <div class="sheet-group">
          <div v-if="!configured.length" class="empty">{{ t('deployer.settings.noTargets') }}</div>
          <div v-for="tg in configured" :key="tg.id" class="target">
            <div class="target__head">
              <span class="target__name">{{ targetLabel(tg.id) }}</span>
              <span class="target__actions">
                <MiuixButton
                  v-if="tg.id === 'teesim'"
                  :disabled="locked || busyId === tg.id"
                  @pointerdown.stop
                  @click.stop="openRename"
                >
                  {{ t('deployer.settings.renameFile') }}
                </MiuixButton>
                <MiuixButton :disabled="locked || busyId === tg.id" @pointerdown.stop @click.stop="drop(tg.id)">
                  {{ t('deployer.settings.remove') }}
                </MiuixButton>
              </span>
            </div>
            <span class="target__path selectable">{{ tg.path ?? '—' }}</span>
            <span v-if="tg.id === 'teesim'" class="target__note">{{ t('deployer.settings.fileName', { name: teesimFileName(tg.path) }) }}</span>
          </div>
          <template v-if="detectError || addable.length">
            <div v-if="detectError" class="empty">{{ t('deployer.settings.detectFailed', { error: detectError }) }}</div>
            <div v-for="b in addable" :key="b.id" class="target">
              <div class="target__head">
                <span class="target__name">{{ targetLabel(b.id) }}</span>
                <MiuixButton
                  :disabled="locked || busyId === b.id || (b.id === 'teesim' && !isTeesimFileName(teesimName))"
                  @pointerdown.stop
                  @click.stop="add(b.id)"
                >
                  {{ t('deployer.settings.add') }}
                </MiuixButton>
              </div>
              <span class="target__path selectable">{{ b.path }}</span>
              <span class="target__note">
                {{
                  b.module.version
                    ? t('deployer.settings.moduleDetectedVersion', { name: b.module.name ?? b.module.id, version: b.module.version })
                    : t('deployer.settings.moduleDetected', { name: b.module.name ?? b.module.id })
                }}
              </span>
              <template v-if="b.id === 'teesim'">
                <MiuixInput v-model="teesimName" :label="t('deployer.settings.fileNameLabel')" single-line :disabled="locked" />
                <span class="target__note">
                  {{ t('deployer.settings.teesimNote') }}
                </span>
                <span v-if="teesimNameError" class="target__error">{{ teesimNameError }}</span>
              </template>
            </div>
          </template>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.settings.targetsNote') }}
        </MiuixText>

        <MiuixSmallTitle :text="t('deployer.settings.cloud')" />
        <div class="sheet-group">
          <MiuixBasicComponent :title="t('deployer.settings.syncLink')" :summary="locationSummary" clickable :disabled="locked" @click="emit('import')">
            <template #end>
              <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
            </template>
          </MiuixBasicComponent>
          <MiuixBasicComponent
            v-if="webdavNeeded"
            :title="t('deployer.common.webdavPassword')"
            :summary="t('deployer.settings.webdavSummary')"
            clickable
            :disabled="locked"
            @click="emit('secret', 'webdav-password')"
          />
          <MiuixInput v-model="draft.proxy" :label="t('deployer.settings.proxy')" single-line :disabled="locked" />
        </div>

        <MiuixSmallTitle :text="t('deployer.settings.encryption')" />
        <div class="sheet-group">
          <MiuixBasicComponent
            :title="t('deployer.common.password')"
            :summary="t('deployer.settings.passwordSummary')"
            clickable
            :disabled="locked"
            @click="emit('secret', 'age')"
          />
        </div>

        <MiuixSmallTitle :text="t('deployer.settings.other')" />
        <div class="sheet-group">
          <MiuixInput v-model="draft.label" :label="t('deployer.settings.label')" single-line :disabled="locked" />
        </div>

        <MiuixButton type="primary" :disabled="locked" @click="save">
          {{ saving ? t('shared.common.saving') : t('deployer.settings.save') }}
        </MiuixButton>

        <MiuixText v-if="actionError" type="footnote1" color="var(--m-color-error)">{{ actionError }}</MiuixText>

        <MiuixSmallTitle :text="t('deployer.settings.danger')" color="var(--m-color-error)" />
        <div class="sheet-group danger-group">
          <MiuixBasicComponent
            :title="t('deployer.settings.removeDeployed')"
            :summary="t('deployer.settings.removeDeployedSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'remove'"
          />
          <MiuixBasicComponent
            :title="t('deployer.settings.forget')"
            :summary="t('deployer.settings.forgetSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'forget'"
          />
        </div>
      </template>
    </div>

    <MiuixDialog v-model="dialogOpen" :title="t(dialog.title)" :summary="dialogSummary">
      <div class="dialog">
        <MiuixCheckboxPreference
          v-if="danger === 'forget'"
          v-model="forgetStaging"
          :title="t('deployer.settings.forgetStaging')"
          :summary="t('deployer.settings.forgetStagingSummary')"
          :disabled="pending !== null"
        />
        <div class="dialog-actions">
          <MiuixButton @click="dialogOpen = false">{{ t('shared.common.cancel') }}</MiuixButton>
          <MiuixButton class="danger-btn" type="primary" :disabled="pending !== null || busy" @click="runDanger">
            {{ pending ? t('deployer.settings.processing') : t(dialog.ok) }}
          </MiuixButton>
        </div>
      </div>
    </MiuixDialog>

    <MiuixDialog
      v-model="renameOpen"
      :title="t('deployer.settings.renameTitle')"
      :summary="t('deployer.settings.renameSummary')"
    >
      <div class="dialog">
        <MiuixInput v-model="renameName" :label="t('deployer.settings.fileNameLabel')" single-line :disabled="renameBusy" />
        <MiuixText v-if="renameError" type="footnote1" color="var(--m-color-error)">{{ renameError }}</MiuixText>
        <div class="dialog-actions">
          <MiuixButton @click="renameOpen = false">{{ t('shared.common.cancel') }}</MiuixButton>
          <MiuixButton type="primary" :disabled="renameBusy || renameError !== null" @click="confirmRename">
            {{ renameBusy ? t('deployer.settings.processing') : t('deployer.settings.renameConfirm') }}
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
