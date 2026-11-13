<script setup lang="ts">
// Inline renderer for the `share --json` QR matrix (`qr.rows`, '1' = dark).
// One <rect> per dark module inside a 4-module quiet zone, black on white so
// any scanner can read it from the transparent WebView. No remote images
// (CSP `img-src 'self' data:`); the payload is the import URI only.
import { computed } from 'vue'
import { t } from '../../../shared/lib/i18n'

const props = defineProps<{
  /** `size` rows of `size` chars. */
  rows: string[]
  /** Screen-reader description; the module matrix itself is not text. */
  label?: string
}>()

const QUIET = 4

const valid = computed(
  () =>
    props.rows.length > 0 &&
    props.rows.every((row) => row.length === props.rows.length && /^[01]+$/.test(row)),
)

const dim = computed(() => props.rows.length + QUIET * 2)

const modules = computed(() => {
  if (!valid.value) return []
  const out: { x: number; y: number }[] = []
  props.rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x += 1) {
      if (row[x] === '1') out.push({ x: x + QUIET, y: y + QUIET })
    }
  })
  return out
})
</script>

<template>
  <div class="qr" :style="{ aspectRatio: `${dim} / ${dim}` }">
    <svg
      v-if="valid"
      class="qr__svg"
      :viewBox="`0 0 ${dim} ${dim}`"
      shape-rendering="crispEdges"
      role="img"
      :aria-label="label ?? t('issuer.qrCode.label')"
    >
      <rect :width="dim" :height="dim" fill="#ffffff" />
      <rect v-for="(cell, i) in modules" :key="i" :x="cell.x" :y="cell.y" width="1" height="1" fill="#000000" />
    </svg>
    <span v-else class="qr__invalid">{{ t('issuer.qrCode.invalid') }}</span>
  </div>
</template>

<style scoped>
.qr {
  width: 100%;
  max-width: 232px;
  margin: 0 auto;
  border-radius: 12px;
  overflow: hidden;
  background: #ffffff;
}
.qr__svg {
  display: block;
  width: 100%;
  height: 100%;
}
.qr__invalid {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
  color: var(--m-color-error);
  font-size: 14px;
}
</style>
