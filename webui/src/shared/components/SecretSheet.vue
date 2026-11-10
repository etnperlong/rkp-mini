<script setup lang="ts">
// Enter one secret and store it via the 0600 inbox (lib/sync.ts setSecret).
// Inputs are cleared on every open, after submit and on close; the page never
// reads a stored secret back, only its presence/permissions from `status`.
import { computed, ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixButton, MiuixIcon, MiuixIconButton, MiuixText, showSnackbar } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import SecretInput from './SecretInput.vue'
import { CommandError } from '../lib/command'
import { setSecret, type SecretKind } from '../lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ kind: SecretKind; title: string; hint: string }>()
const emit = defineEmits<{ saved: [weak: boolean] }>()

const first = ref('')
const second = ref('')
const busy = ref(false)
const error = ref<string | null>(null)

/** The age passphrase is typed twice; tokens/app passwords are usually pasted once. */
const twice = computed(() => props.kind === 'age')

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
  if (twice.value && first.value !== second.value) return '两次输入不一致'
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
    void showSnackbar({ message: r.weak ? '已保存；口令较短，建议 6 个以上随机单词或 20+ 随机字符' : '已保存（0600）' })
  } catch (e) {
    error.value = e instanceof CommandError ? `${e.message}` : String(e)
  } finally {
    clear()
    busy.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="title">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">{{ hint }}</MiuixText>
      <div class="sheet-group">
        <SecretInput v-model="first" :label="twice ? '输入' : '值'" :disabled="busy" />
        <SecretInput v-if="twice" v-model="second" label="再次输入" :disabled="busy" />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        经 KernelSU root shell 写入 0600 临时文件后由 rkp-mini-sync 保存并立即删除；不会出现在命令行参数、URL 或日志中。
      </MiuixText>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>
      <MiuixButton type="primary" :disabled="busy || problem !== null" @click="save">
        {{ busy ? '保存中…' : '保存' }}
      </MiuixButton>
    </div>
  </MiuixBottomSheet>
</template>
