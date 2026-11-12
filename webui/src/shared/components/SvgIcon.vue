<script setup lang="ts">
// One-path 24×24 icon from shared/lib/icons.ts. Size and position come from the caller's class.
defineProps<{ path: string; color?: string }>()
</script>

<template>
  <svg viewBox="0 0 24 24" aria-hidden="true" :fill="color ?? 'currentColor'"><path :d="path" /></svg>
</template>
