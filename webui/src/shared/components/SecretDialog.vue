<script setup lang="ts">
// Enter one secret in a dialog and store it via the 0600 inbox (lib/sync.ts setSecret).
// Inputs are cleared on every open, after submit and on close; the page never
// reads a stored secret back, only its presence/permissions from `status`.
import { computed, ref, watch } from 'vue'
import { MiuixButton, MiuixDialog, MiuixText, showSnackbar } from 'miuix-vue'
import SecretInput from './SecretInput.vue'
import { CommandError } from '../lib/command'
import { SYNC_ROLE, setSecret, type SecretKind } from '../lib/sync'

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

const PLACEHOLDER: Record<SecretKind, string> = {
  age: '密码',
  'gist-token': 'Gist token',
  'webdav-password': 'WebDAV 应用密码',
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
  if (!first.value) return '请输入'
  if (/[\r\n]/.test(first.value)) return '不能包含换行'
  if (choosing.value && first.value !== second.value) return '两次输入不一致'
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
    void showSnackbar({ message: weak ? '已保存；口令较短，建议 6 个以上随机单词或 20+ 随机字符' : '已保存（0600）' })
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
      <SecretInput v-model="first" :placeholder="PLACEHOLDER[kind]" :disabled="busy" />
      <SecretInput v-if="choosing" v-model="second" placeholder="再次输入" :disabled="busy" />
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        经 KernelSU root shell 写入 0600 临时文件后由 rkp-mini-sync 保存并立即删除；不会出现在命令行参数、URL 或日志中。
      </MiuixText>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>
      <div class="dialog-actions">
        <MiuixButton :disabled="busy" @click="open = false">取消</MiuixButton>
        <MiuixButton type="primary" :disabled="busy || problem !== null" @click="save">
          {{ busy ? '保存中…' : '保存' }}
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
