<script setup lang="ts">
// Shared log sheet: the module's background loop log (logs/<loop>.log, last
// 200 lines) and the page's in-memory debug lines. Apps add their own sections
// through the default slot (the issuer page lists its issue logs there).
// Nothing here reads a keybox or a secret.
import { ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import FoldBlock from './FoldBlock.vue'
import { SYNC_LOG, SYNC_LOG_LABEL, readSyncLog } from '../lib/sync'
import { CommandError } from '../lib/command'
import { debugLog } from '../lib/debug'

const open = defineModel<boolean>({ required: true })
withDefaults(
  defineProps<{
    /** Footnote under 后台日志; the default is the receiver's wording. */
    loopHint?: string
  }>(),
  { loopHint: '后台循环的记录（最后 200 行），只含 provider、seq、hash 与错误码，不含密码或 token。' },
)

const loopText = ref('')
const loopLoading = ref(false)

function message(e: unknown): string {
  return e instanceof CommandError ? `${e.message}${e.detail ? `：${e.detail}` : ''}` : String(e)
}

async function load(): Promise<void> {
  loopLoading.value = true
  try {
    loopText.value = await readSyncLog()
  } catch (e) {
    loopText.value = message(e)
  } finally {
    loopLoading.value = false
  }
}

watch(open, (v) => {
  if (v) void load()
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="日志">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <slot />

      <MiuixSmallTitle text="后台日志" />
      <div class="sheet-group">
        <FoldBlock
          :title="SYNC_LOG_LABEL"
          :summary="loopLoading ? '正在读取…' : SYNC_LOG"
          :text="loopLoading ? '读取中…' : loopText"
        />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        {{ loopHint }}
      </MiuixText>

      <MiuixSmallTitle text="调试日志" />
      <div class="sheet-group">
        <FoldBlock title="页面调试日志" :summary="`${debugLog.length} 条`" :text="debugLog.join('\n')" />
      </div>
      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        页面运行时的诊断记录，只保存在内存中，关闭页面即清空。
      </MiuixText>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
</style>
