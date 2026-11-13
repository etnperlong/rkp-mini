<script setup lang="ts">
// Rotate the stream's age passphrase (design §19.5) in one dialog: it states the
// consequences, takes the new passphrase twice and then shows the task. The new
// passphrase never becomes a command argument: prepareRotation() writes it to the
// 0600 inbox and the detached `rotate` task reads it. If the task cannot start, the
// staged file is removed again. Typed values are cleared on open, on submit and on close.
import { computed, ref, watch } from 'vue'
import { MiuixButton, MiuixDialog, MiuixProgressIndicator, MiuixText, showSnackbar } from 'miuix-vue'
import SecretInput from '../../../shared/components/SecretInput.vue'
import { CommandError } from '../../../shared/lib/command'
import {
  discardRotation,
  prepareRotation,
  syncErrorText,
  syncTaskArgs,
  type SyncRunner,
} from '../../../shared/lib/sync'
import { t } from '../../../shared/lib/i18n'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ runner: SyncRunner }>()

const r = props.runner
const first = ref('')
const second = ref('')
const busy = ref(false)
const error = ref<string | null>(null)
/** The rotate task was launched: show its progress and the rotation semantics. */
const launched = ref(false)

function message(e: unknown): string {
  return e instanceof CommandError
    ? e.detail
      ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
      : e.message
    : String(e)
}

function clear(): void {
  first.value = ''
  second.value = ''
}

watch(open, () => {
  clear()
  error.value = null
  launched.value = false
})

// The runner can also report launch_failed after its start grace period; the
// staged passphrase must not outlive a task that never ran.
watch(
  () => r.phase.value,
  (phase) => {
    if (phase === 'launch_failed' && launched.value) {
      void discardRotation()
      launched.value = false
      error.value = t('issuer.senderRotateDialog.rotateNotStarted')
    }
  },
)

const problem = computed(() => {
  if (!first.value) return t('issuer.senderRotateDialog.required')
  if (/[\r\n]/.test(first.value)) return t('issuer.senderRotateDialog.newline')
  if (first.value !== second.value) return t('issuer.senderRotateDialog.mismatch')
  if (r.busy.value) return t('issuer.senderRotateDialog.busyOther')
  return null
})

const short = computed(() => {
  const bytes = new TextEncoder().encode(first.value).length
  return bytes > 0 && bytes < 20
})

const progress = computed(() => {
  if (!launched.value) return null
  if (r.busy.value) return t('issuer.senderRotateDialog.progressRunning')
  const result = r.result.value
  if (!result || !r.showLast.value) return t('issuer.senderRotateDialog.started')
  const env = result.envelope
  if (!env) return t('issuer.senderRotateDialog.exitUnknown', { code: result.code })
  return env.ok ? t('issuer.senderRotateDialog.done') : syncErrorText(env.error)
})

const failed = computed(() => {
  if (!launched.value) return false
  const result = r.result.value
  return !!result && !r.busy.value && !!result.envelope && result.envelope.ok === false
})

async function begin(): Promise<void> {
  if (problem.value || busy.value) return
  busy.value = true
  error.value = null
  try {
    await prepareRotation(first.value)
    await r.start(syncTaskArgs('rotate'))
    if (r.phase.value === 'launch_failed') {
      await discardRotation()
      error.value = t('issuer.senderRotateDialog.rotateNotStarted')
      return
    }
    launched.value = true
    void showSnackbar({ message: t('issuer.senderRotateDialog.launched') })
  } catch (e) {
    await discardRotation()
    error.value = message(e)
  } finally {
    clear()
    busy.value = false
  }
}

function retry(): void {
  r.dismiss()
  launched.value = false
  error.value = null
}
</script>

<template>
  <MiuixDialog
    v-model="open"
    :title="t('issuer.senderRotateDialog.title')"
    :summary="t('issuer.senderRotateDialog.summary')"
    :close-on-click-modal="!busy && !r.busy.value"
  >
    <form class="dialog" @submit.prevent="begin">
      <template v-if="!launched">
        <SecretInput v-model="first" :placeholder="t('issuer.senderRotateDialog.newPassword')" :disabled="busy" />
        <SecretInput v-model="second" :placeholder="t('shared.secret.retype')" :disabled="busy" />
        <MiuixText v-if="short" type="footnote1" color="var(--app-warning)">
          {{ t('issuer.senderRotateDialog.short') }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('issuer.senderRotateDialog.note') }}
        </MiuixText>
      </template>

      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <div v-if="progress" class="progress">
        <MiuixProgressIndicator v-if="r.busy.value" type="infinite" :size="18" />
        <MiuixText type="body2" :color="failed ? 'var(--m-color-error)' : undefined">{{ progress }}</MiuixText>
      </div>

      <div v-if="launched" class="notes">
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · {{ t('issuer.senderRotateDialog.noteReceiver') }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · {{ t('issuer.senderRotateDialog.noteHistory') }}
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · {{ t('issuer.senderRotateDialog.noteRevoke') }}
        </MiuixText>
      </div>

      <div class="dialog-actions">
        <template v-if="!launched">
          <MiuixButton :disabled="busy" @click="open = false">{{ t('shared.common.cancel') }}</MiuixButton>
          <MiuixButton class="danger-btn" type="primary" :disabled="busy || problem !== null" @click="begin">
            {{ busy ? t('issuer.senderRotateDialog.submitting') : t('issuer.senderRotateDialog.reset') }}
          </MiuixButton>
        </template>
        <template v-else>
          <MiuixButton v-if="failed" :disabled="r.busy.value" @click="retry">{{ t('issuer.senderRotateDialog.retry') }}</MiuixButton>
          <MiuixButton type="primary" :disabled="r.busy.value" @click="open = false">{{ t('issuer.senderRotateDialog.finish') }}</MiuixButton>
        </template>
      </div>
    </form>
  </MiuixDialog>
</template>

<style scoped>
.dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}
.dialog :deep(.secret) {
  padding: 0;
}
.dialog-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 4px;
}
.danger-btn.m-button--primary:not(.m-button--disabled) {
  background: var(--m-color-error);
  color: #fff;
}
.progress {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.notes {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--app-sheet-group);
}
</style>
