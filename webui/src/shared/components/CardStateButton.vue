<script setup lang="ts">
import { MiuixIcon, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { Refresh } from 'miuix-vue/icons'

// Top-right corner of a state card: the state label doubles as its refresh
// control, and shows a spinner (with the busy text) while work runs.
// pointerdown is stopped because MiuixCard captures the pointer for its press
// feedback, which would otherwise retarget this click to the card.
defineProps<{ label: string; busy: boolean; disabled: boolean }>()
const emit = defineEmits<{ refresh: [] }>()
</script>

<template>
  <button
    type="button"
    class="state-btn"
    :disabled="disabled || busy"
    :aria-label="busy ? label : `${label}，点击刷新`"
    @pointerdown.stop
    @click.stop="emit('refresh')"
  >
    <MiuixText type="body2" weight="medium">{{ label }}</MiuixText>
    <MiuixProgressIndicator v-if="busy" type="infinite" :size="16" />
    <MiuixIcon v-else :icon="Refresh" :size="18" />
  </button>
</template>

<style scoped>
.state-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: -6px -8px -6px 0;
  padding: 6px 8px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;
  -webkit-tap-highlight-color: transparent;
}
.state-btn:active:not(:disabled) {
  background: var(--m-color-secondary-variant);
}
.state-btn:disabled {
  cursor: default;
}
</style>
