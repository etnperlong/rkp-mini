<script setup lang="ts">
import { ref } from 'vue'
import { MiuixBasicComponent, MiuixIcon } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'

// Collapsed-by-default monospace block for long text (logs, hex, stderr).
defineProps<{ title: string; text: string; summary?: string }>()

const open = ref(false)
</script>

<template>
  <div class="fold">
    <MiuixBasicComponent :title="title" :summary="summary" clickable @click="open = !open">
      <template #end>
        <span class="fold__chevron" :class="{ 'fold__chevron--open': open }">
          <MiuixIcon :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
        </span>
      </template>
    </MiuixBasicComponent>
    <pre v-if="open" class="fold__body">{{ text || '（空）' }}</pre>
  </div>
</template>

<style scoped>
.fold__chevron {
  display: inline-flex;
  transform: rotate(90deg);
  transition: transform 0.2s ease;
}
.fold__chevron--open {
  transform: rotate(-90deg);
}
.fold__body {
  margin: 0 16px 16px;
  padding: 12px;
  max-height: 50vh;
  overflow: auto;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  color: var(--m-color-on-background);
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  user-select: text;
}
</style>
