<script setup lang="ts">
import { ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixButton, MiuixCheckboxPreference, MiuixIcon, MiuixIconButton, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ keyboxPath: string; hasKeybox: boolean }>()
const emit = defineEmits<{ confirm: [] }>()

const acknowledged = ref(false)

// Require a fresh acknowledgement every time the sheet opens.
watch(open, (v) => {
  if (v) acknowledged.value = false
})

function confirm(): void {
  open.value = false
  emit('confirm')
}

const points = [
  '每次签发都会生成全新的 P-256 密钥，由设备 KeyMint TA（Native V2，0x3122）签名 CSR，再向 Google RKP 服务申请证书。',
  '需要联网，通常几秒到一分钟。签发在后台进行，可以离开此页面。',
  '失败不会影响现有证书；签发成功也不保证通过 Play Integrity。',
]
</script>

<template>
  <MiuixBottomSheet v-model="open" title="签发新证书">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <ul class="points">
        <li v-for="p in points" :key="p">
          <MiuixText type="body2">{{ p }}</MiuixText>
        </li>
        <li>
          <MiuixText type="body2">
            成功后保存到 <code>{{ props.keyboxPath }}</code>{{ props.hasKeybox ? '，旧文件备份为 keybox.xml.prev' : '' }}。
          </MiuixText>
        </li>
      </ul>

      <div class="sheet-group">
        <MiuixCheckboxPreference v-model="acknowledged" title="我已了解，确认签发" />
      </div>

      <MiuixButton type="primary" :disabled="!acknowledged" @click="confirm">开始签发</MiuixButton>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.points {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  color: var(--m-color-on-surface-variant-summary);
}
code {
  font-family: ui-monospace, 'Roboto Mono', monospace;
  font-size: 12px;
  word-break: break-all;
}
</style>
