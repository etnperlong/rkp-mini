<script setup lang="ts">
// Rotate the stream's age passphrase (design §19.5). The new passphrase never
// becomes a command argument: prepareRotation() writes it to the 0600 inbox and
// the detached `rotate` task reads it. If the task cannot start, the staged
// file is removed again. Typed values are cleared on open, on submit and on close.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import SecretInput from './SecretInput.vue'
import { CommandError } from '../../lib/rkp'
import {
  discardRotation,
  prepareRotation,
  syncErrorText,
  syncTaskArgs,
  type SyncRunner,
} from '../../lib/sync'

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
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
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
      error.value = '轮换任务未能启动，新口令未保存'
    }
  },
)

const problem = computed(() => {
  if (!first.value) return '请输入新口令'
  if (/[\r\n]/.test(first.value)) return '不能包含换行'
  if (first.value !== second.value) return '两次输入不一致'
  if (r.busy.value) return '另一个同步任务正在运行'
  return null
})

const short = computed(() => {
  const bytes = new TextEncoder().encode(first.value).length
  return bytes > 0 && bytes < 20
})

const progress = computed(() => {
  if (!launched.value) return null
  if (r.busy.value) return '轮换任务进行中…'
  const result = r.result.value
  if (!result || !r.showLast.value) return '任务已启动，结果见同步卡片'
  const env = result.envelope
  if (!env) return `任务异常退出（${result.code}）`
  return env.ok ? '口令轮换完成' : syncErrorText(env.error)
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
      error.value = '轮换任务未能启动，新口令未保存'
      return
    }
    launched.value = true
    void showSnackbar({ message: '口令轮换任务已启动' })
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
  <MiuixBottomSheet v-model="open" title="轮换 age 口令">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        用新口令重新加密当前 keybox 并上传。旧口令立即失效，接收端必须改成新口令才能继续拉取。
      </MiuixText>

      <div class="sheet-group">
        <SecretInput v-model="first" label="新口令" :disabled="busy || launched" />
        <SecretInput v-model="second" label="再次输入" :disabled="busy || launched" />
      </div>
      <MiuixText v-if="short" type="footnote1" color="var(--app-warning)">
        口令较短：建议 6 个以上随机单词或 20+ 随机字符。
      </MiuixText>

      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <div v-if="progress" class="progress">
        <MiuixProgressIndicator v-if="r.busy.value" type="infinite" :size="18" />
        <MiuixText type="body2" :color="failed ? 'var(--m-color-error)' : undefined">{{ progress }}</MiuixText>
      </div>

      <div v-if="launched" class="notes">
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · 接收端需要设置新口令后才能继续拉取；旧口令不再可用。
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · Gist 的历史 revision 仍然可以用旧口令解密：轮换口令不会删除历史密文。
        </MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          · 真正的撤销 = 重新签发 keybox。已泄露的旧 keybox 仍然有效，换口令只是切断对它的分发。
        </MiuixText>
      </div>

      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        新口令经 root shell 写入 0600 临时文件，由 rkp-mini-sync 读取并立即删除；不会出现在命令行参数、URL 或日志中。
      </MiuixText>

      <MiuixButton v-if="!launched" type="primary" :disabled="busy || problem !== null" @click="begin">
        {{ busy ? '提交中…' : '开始轮换' }}
      </MiuixButton>
      <template v-else>
        <MiuixButton v-if="failed" :disabled="r.busy.value" @click="retry">重新输入</MiuixButton>
        <MiuixButton type="primary" :disabled="r.busy.value" @click="open = false">完成</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
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
