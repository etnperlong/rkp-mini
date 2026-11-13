<script setup lang="ts">
// Enter one secret in a dialog and store it via the 0600 inbox (lib/sync.ts setSecret).
// Inputs are cleared on every open, after submit and on close; the page never
// reads a stored secret back, only its presence/permissions from `status`.
import { computed, ref, watch } from 'vue'
import { MiuixButton, MiuixDialog, MiuixText, showSnackbar } from 'miuix-vue'
import SecretInput from './SecretInput.vue'
import { CommandError } from '../lib/command'
import { SYNC_ROLE, setSecret, type SecretKind } from '../lib/sync'
import { t, type MessageKey } from '../lib/i18n'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ kind: SecretKind; title: string; hint: string }>()
const emit = defineEmits<{ saved: [weak: boolean] }>()

const first = ref('')
const second = ref('')
const busy = ref(false)
const error = ref<string | null>(null)

/** Only the sender chooses the passphrase, so only it types it twice and gets the strength hint;
 * the receiver re-enters a known one and verifies it by decrypting (the page runs check-remote). */
const choosing = computed(() => props.kind === 'age' && SYNC_ROLE === 'sender')

const PLACEHOLDER: Record<SecretKind, MessageKey> = {
  age: 'shared.secret.placeholder.age',
  'gist-token': 'shared.secret.placeholder.gistToken',
  'webdav-password': 'shared.secret.placeholder.webdavPassword',
}

function clear(): void {
  first.value = ''
  second.value = ''
}

watch(open, () => {
  clear()
  error.value = null
})

const problem = computed(() => {
  if (!first.value) return t('shared.secret.required')
  if (/[\r\n]/.test(first.value)) return t('shared.secret.newline')
  if (choosing.value && first.value !== second.value) return t('shared.secret.mismatch')
  return null
})

async function save(): Promise<void> {
  if (problem.value) return
  busy.value = true
  error.value = null
  try {
    const r = await setSecret(props.kind, first.value)
    clear()
    open.value = false
    emit('saved', r.weak)
    const weak = choosing.value && r.weak
    void showSnackbar({ message: weak ? t('shared.secret.savedWeak') : t('shared.secret.saved') })
  } catch (e) {
    error.value = e instanceof CommandError ? `${e.message}` : String(e)
  } finally {
    clear()
    busy.value = false
  }
}
</script>

<template>
  <MiuixDialog v-model="open" :title="title" :summary="hint" :close-on-click-modal="!busy">
    <form class="dialog" @submit.prevent="save">
      <SecretInput v-model="first" :placeholder="t(PLACEHOLDER[kind])" :disabled="busy" />
      <SecretInput v-if="choosing" v-model="second" :placeholder="t('shared.secret.retype')" :disabled="busy" />
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ t('shared.secret.note') }}
      </MiuixText>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>
      <div class="dialog-actions">
        <MiuixButton :disabled="busy" @click="open = false">{{ t('shared.common.cancel') }}</MiuixButton>
        <MiuixButton type="primary" :disabled="busy || problem !== null" @click="save">
          {{ busy ? t('shared.common.saving') : t('shared.common.save') }}
        </MiuixButton>
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
</style>
