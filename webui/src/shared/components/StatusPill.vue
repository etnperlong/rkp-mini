<script setup lang="ts">
// Miuix has no Badge in miuix-vue 0.1.1; this is the app-owned capsule.
export type PillTone = 'ok' | 'warning' | 'error' | 'neutral'

defineProps<{ tone: PillTone; label: string }>()
</script>

<template>
  <span class="pill" :class="`pill--${tone}`">{{ label }}</span>
</template>

<style scoped>
.pill {
  display: inline-flex;
  align-items: center;
  flex: none;
  padding: 3px 10px;
  border-radius: var(--m-radius-full);
  font-size: var(--m-text-footnote1-size);
  font-weight: 500;
  line-height: 1.4;
  white-space: nowrap;
}
.pill--ok {
  background: var(--m-color-tertiary-container);
  color: var(--m-color-on-tertiary-container);
}
/* First background is the fallback for WebViews without color-mix(). */
.pill--warning {
  background: var(--m-color-secondary-variant);
  background: color-mix(in srgb, var(--app-warning) 16%, transparent);
  color: var(--app-warning);
}
.pill--error {
  background: var(--m-color-error-container);
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
  color: var(--m-color-error);
}
.pill--neutral {
  background: var(--m-color-secondary-variant);
  color: var(--m-color-on-surface-variant-summary);
}
</style>
