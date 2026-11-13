<script setup lang="ts">
import { ref, watch } from 'vue'
import { MiuixBottomSheet, MiuixButton, MiuixCheckboxPreference, MiuixIcon, MiuixIconButton, MiuixText } from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { t, type MessageKey } from '../../../shared/lib/i18n'

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

const points: MessageKey[] = ['issuer.issueSheet.point1', 'issuer.issueSheet.point2', 'issuer.issueSheet.point3']
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('issuer.issueSheet.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <ul class="points">
        <li v-for="p in points" :key="p">
          <MiuixText type="body2">{{ t(p) }}</MiuixText>
        </li>
        <li>
          <MiuixText type="body2">
            {{ t('issuer.issueSheet.savedPrefix') }}<code>{{ props.keyboxPath }}</code>{{ t(props.hasKeybox ? 'issuer.issueSheet.savedBackup' : 'issuer.issueSheet.savedEnd') }}
          </MiuixText>
        </li>
      </ul>

      <div class="sheet-group">
        <MiuixCheckboxPreference v-model="acknowledged" :title="t('issuer.issueSheet.acknowledge')" />
      </div>

      <MiuixButton type="primary" :disabled="!acknowledged" @click="confirm">{{ t('issuer.issueSheet.start') }}</MiuixButton>
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
