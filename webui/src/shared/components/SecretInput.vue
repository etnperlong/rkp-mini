<script setup lang="ts">
// Masked single-line input for secrets. MiuixInput renders type="text", which
// would display the value; this keeps the browser from showing, autofilling
// or spell-checking it. The value lives only in the parent's ref.
const value = defineModel<string>({ required: true })
defineProps<{ label: string; disabled?: boolean }>()
</script>

<template>
  <label class="secret">
    <span class="secret__label">{{ label }}</span>
    <input
      v-model="value"
      class="secret__field"
      type="password"
      autocomplete="off"
      autocapitalize="off"
      autocorrect="off"
      spellcheck="false"
      :disabled="disabled"
    />
  </label>
</template>

<style scoped>
.secret {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 16px;
}
.secret__label {
  font-size: 13px;
  color: var(--m-color-on-surface-variant-summary);
}
.secret__field {
  width: 100%;
  box-sizing: border-box;
  padding: 12px 14px;
  border: none;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  color: var(--m-color-on-surface);
  font-size: 16px;
  outline: none;
}
.secret__field:focus {
  box-shadow: 0 0 0 2px var(--m-color-primary);
}
</style>
