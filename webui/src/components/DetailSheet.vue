<script setup lang="ts">
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixBottomSheet, MiuixIcon, MiuixIconButton, MiuixSmallTitle, MiuixText } from 'miuix-vue'
import { Close, Copy } from 'miuix-vue/icons'
import StatusPill from './StatusPill.vue'
import { KEYBOX_STATE_LABEL, keyboxState, type StatusData } from '../lib/rkp'
import { daysLeft, formatDateTime } from '../lib/format'
import { copyText } from '../lib/clipboard'

// Only fields already present in status.json; never keybox/key contents.
const open = defineModel<boolean>({ required: true })
const props = defineProps<{ status: StatusData; now: number }>()

const kb = computed(() => props.status.keybox)
const state = computed(() => keyboxState(kb.value, props.now))
const tone = computed(() =>
  state.value === 'valid' ? 'ok' : state.value === 'expiring' ? 'warning' : state.value === 'missing' ? 'neutral' : 'error',
)
const remaining = computed(() => {
  if (kb.value.not_after == null) return '—'
  const d = daysLeft(kb.value.not_after, props.now)
  return d < 0 ? `已过期 ${-d} 天` : `${d} 天`
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="证书详情">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div class="head">
        <StatusPill :tone="tone" :label="KEYBOX_STATE_LABEL[state]" />
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">
          检查于 {{ formatDateTime(status.generated_at) }}
        </MiuixText>
      </div>

      <div class="sheet-group">
        <MiuixBasicComponent
          title="叶子证书主题"
          :summary="kb.leaf_subject || '—'"
          :clickable="!!kb.leaf_subject"
          @click="kb.leaf_subject && copyText(kb.leaf_subject, '证书主题')"
        >
          <template v-if="kb.leaf_subject" #end>
            <MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
        <MiuixBasicComponent title="证书链长度" :summary="`${kb.certificates} 张（叶子 → 根）`" />
        <MiuixBasicComponent
          title="有效期至"
          :summary="kb.not_after != null ? `${formatDateTime(kb.not_after)}（${kb.not_after_utc}）` : '—'"
        />
        <MiuixBasicComponent title="剩余" :summary="remaining" />
        <MiuixBasicComponent
          title="私钥与叶子证书匹配"
          :summary="kb.key_match ? '是' : '否'"
          :summary-color="kb.key_match ? undefined : 'var(--m-color-error)'"
        />
        <MiuixBasicComponent v-if="kb.error" title="错误" :summary="kb.error" summary-color="var(--m-color-error)" />
      </div>

      <MiuixSmallTitle text="路径与设备" />
      <div class="sheet-group">
        <MiuixBasicComponent title="keybox" :summary="kb.path" clickable @click="copyText(kb.path, '路径')">
          <template #end><MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" /></template>
        </MiuixBasicComponent>
        <MiuixBasicComponent
          title="系统指纹"
          :summary="status.device.fingerprint || '—'"
          :clickable="!!status.device.fingerprint"
          @click="status.device.fingerprint && copyText(status.device.fingerprint, '指纹')"
        >
          <template v-if="status.device.fingerprint" #end>
            <MiuixIcon :icon="Copy" :size="20" color="var(--m-color-on-surface-variant-actions)" />
          </template>
        </MiuixBasicComponent>
      </div>

      <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        WebUI 不读取、不显示 keybox.xml 与私钥内容。需要时请以 root 直接复制该文件。
      </MiuixText>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.head {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.sheet-body :deep(.m-small-title) {
  padding: 4px 0 0;
}
.sheet-body :deep(.m-basic-component__center .m-text:last-child) {
  word-break: break-all;
}
</style>
