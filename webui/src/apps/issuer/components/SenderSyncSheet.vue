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
import { t, type MessageKey } from '../../../shared/lib/i18n'

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

const DIALOG: Record<'delete' | 'forget' | 'reset', { title: MessageKey; summary: MessageKey; ok: MessageKey }> = {
  delete: {
    title: 'issuer.senderSyncSheet.deleteTitle',
    summary: 'issuer.senderSyncSheet.deleteSummary',
    ok: 'issuer.senderSyncSheet.deleteOk',
  },
  forget: {
    title: 'issuer.senderSyncSheet.forgetTitle',
    summary: 'issuer.senderSyncSheet.forgetSummary',
    ok: 'issuer.senderSyncSheet.forgetOk',
  },
  reset: {
    title: 'issuer.senderSyncSheet.resetTitle',
    summary: 'issuer.senderSyncSheet.resetSummary',
    ok: 'issuer.senderSyncSheet.resetOk',
  },
}

const dialog = computed(() => (danger.value ? DIALOG[danger.value] : DIALOG.forget))

/** WebDAV deletes only the one file; say so instead of the Gist wording. */
const dialogSummary = computed(() =>
  danger.value === 'delete' && draft.provider !== 'gist'
    ? t('issuer.senderSyncSheet.deleteSummaryWebdav')
    : t(dialog.value.summary),
)

function message(e: unknown): string {
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : String(e)
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
    actionError.value = t('issuer.senderSyncSheet.daysInvalid')
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
    void showSnackbar({ message: t('issuer.senderSyncSheet.noChanges') })
    return
  }

  const turningOn = draft.auto_push && !(c.auto_push ?? true)
  saving.value = true
  try {
    for (const [key, value] of updates) await setConfig(key, value)
    // The loop is what actually pushes; starting it is idempotent (pid lock).
    if (turningOn) await startLoop()
    void showSnackbar({ message: t('issuer.senderSyncSheet.saved') })
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
    void showSnackbar({
      message: action === 'reset' ? t('issuer.senderSyncSheet.resetDone') : t('issuer.senderSyncSheet.forgotten'),
    })
    emit('changed')
  } catch (e) {
    actionError.value = message(e)
  } finally {
    pending.value = null
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.senderSyncSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('issuer.senderSyncSheet.intro') }}
      </MiuixText>

      <MiuixText v-if="loading" type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('issuer.senderSyncSheet.loadingConfig') }}
      </MiuixText>
      <MiuixText v-else-if="loadError" type="body2" color="var(--m-color-error)">
        {{ t('issuer.senderSyncSheet.loadFailed', { error: loadError }) }}
      </MiuixText>

      <template v-else-if="cfg">
        <MiuixSmallTitle :text="t('issuer.senderSyncSheet.autoPush')" />
        <div class="sheet-group">
          <MiuixSwitchPreference
            v-model="draft.auto_push"
            :title="t('issuer.senderSyncSheet.autoPush')"
            :summary="
              syncEnabled
                ? t('issuer.senderSyncSheet.autoPushOn')
                : t('issuer.senderSyncSheet.autoPushNeedsSync')
            "
            :disabled="locked || !syncEnabled"
          />
          <MiuixInput
            v-model="draft.min_validity_days"
            :label="t('issuer.senderSyncSheet.minDays')"
            single-line
            :disabled="locked"
          />
        </div>

        <MiuixSmallTitle :text="t('issuer.senderSyncSheet.cloud')" />
        <div class="sheet-group">
          <MiuixDropdownPreference
            v-model="providerIndex"
            :title="t('issuer.senderSyncSheet.provider')"
            :summary="
              draft.provider === 'gist'
                ? t('issuer.senderSyncSheet.providerGist')
                : t('issuer.senderSyncSheet.providerWebdav')
            "
            :items="PROVIDERS"
            :disabled="locked"
          />
          <template v-if="draft.provider === 'gist'">
            <MiuixInput
              v-model="draft.gist_id"
              :label="t('issuer.senderSyncSheet.gistId')"
              single-line
              :disabled="locked"
            />
            <MiuixInput
              v-model="draft.gist_filename"
              :label="t('issuer.senderSyncSheet.filename')"
              single-line
              :disabled="locked"
            />
            <MiuixBasicComponent
              title="Gist token"
              :summary="t('issuer.senderSyncSheet.gistTokenSummary')"
              clickable
              :disabled="locked"
              @click="emit('secret', 'gist-token')"
            />
          </template>
          <template v-else>
            <MiuixInput
              v-model="draft.webdav_endpoint"
              :label="t('issuer.senderSyncSheet.endpoint')"
              single-line
              :disabled="locked"
            />
            <MiuixInput
              v-model="draft.webdav_path"
              :label="t('issuer.senderSyncSheet.webdavPath')"
              single-line
              :disabled="locked"
            />
            <MiuixInput
              v-model="draft.webdav_user"
              :label="t('issuer.senderSyncSheet.webdavUser')"
              single-line
              :disabled="locked"
            />
            <MiuixBasicComponent
              :title="t('issuer.senderSyncSheet.webdavPassword')"
              :summary="t('issuer.senderSyncSheet.webdavPasswordSummary')"
              clickable
              :disabled="locked"
              @click="emit('secret', 'webdav-password')"
            />
          </template>
          <MiuixInput v-model="draft.proxy" :label="t('issuer.senderSyncSheet.proxy')" single-line :disabled="locked" />
        </div>

        <MiuixSmallTitle :text="t('issuer.senderSyncSheet.encryption')" />
        <div class="sheet-group">
          <MiuixBasicComponent
            :title="t('issuer.senderSyncSheet.password')"
            :summary="t('issuer.senderSyncSheet.passwordSummary')"
            clickable
            :disabled="locked"
            @click="emit('secret', 'age')"
          />
        </div>

        <MiuixSmallTitle :text="t('issuer.senderSyncSheet.other')" />
        <div class="sheet-group">
          <MiuixInput v-model="draft.label" :label="t('issuer.senderSyncSheet.label')" single-line :disabled="locked" />
        </div>

        <MiuixButton type="primary" :disabled="locked" @click="save">
          {{ saving ? t('shared.common.saving') : t('issuer.senderSyncSheet.save') }}
        </MiuixButton>

        <MiuixText v-if="actionError" type="footnote1" color="var(--m-color-error)">{{ actionError }}</MiuixText>

        <MiuixSmallTitle :text="t('issuer.senderSyncSheet.danger')" color="var(--m-color-error)" />
        <div class="sheet-group danger-group">
          <MiuixBasicComponent
            :title="t('issuer.senderSyncSheet.deleteItem')"
            :summary="t('issuer.senderSyncSheet.deleteItemSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'delete'"
          />
          <MiuixBasicComponent
            :title="t('issuer.senderSyncSheet.rotateItem')"
            :summary="t('issuer.senderSyncSheet.rotateItemSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="emit('rotate')"
          />
          <MiuixBasicComponent
            :title="t('issuer.senderSyncSheet.forgetItem')"
            :summary="t('issuer.senderSyncSheet.forgetItemSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'forget'"
          />
          <MiuixBasicComponent
            :title="t('issuer.senderSyncSheet.resetItem')"
            :summary="t('issuer.senderSyncSheet.resetItemSummary')"
            title-color="var(--m-color-error)"
            clickable
            :disabled="locked"
            @click="danger = 'reset'"
          />
        </div>
      </template>
    </div>

    <MiuixDialog v-model="dialogOpen" :title="t(dialog.title)" :summary="dialogSummary">
      <div class="dialog-actions">
        <MiuixButton @click="dialogOpen = false">{{ t('shared.common.cancel') }}</MiuixButton>
        <MiuixButton class="danger-btn" type="primary" :disabled="pending !== null || busy" @click="runDanger">
          {{ pending ? t('issuer.senderSyncSheet.processing') : t(dialog.ok) }}
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
