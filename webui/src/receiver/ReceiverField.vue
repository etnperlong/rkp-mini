<script setup lang="ts">
// Plain text field for non-secret values (paths, labels, numbers, pasted
// links). Secrets must keep using components/sync/SecretInput.vue.
const value = defineModel<string>({ required: true })

withDefaults(
  defineProps<{
    label: string
    placeholder?: string
    hint?: string
    /** Show a numeric keyboard; the value stays a string until validated. */
    numeric?: boolean
    multiline?: boolean
    rows?: number
    disabled?: boolean
    mono?: boolean
  }>(),
  { rows: 5 },
)
</script>

<template>
  <label class="field">
    <span class="field__label">{{ label }}</span>
    <textarea
      v-if="multiline"
      v-model="value"
      class="field__control field__control--area"
      :class="{ 'field__control--mono': mono }"
      :rows="rows"
      :placeholder="placeholder"
      :disabled="disabled"
      autocapitalize="off"
      autocorrect="off"
      spellcheck="false"
    ></textarea>
    <input
      v-else
      v-model="value"
      class="field__control"
      :class="{ 'field__control--mono': mono }"
      type="text"
      :inputmode="numeric ? 'numeric' : undefined"
      :placeholder="placeholder"
      :disabled="disabled"
      autocapitalize="off"
      autocorrect="off"
      spellcheck="false"
    />
    <span v-if="hint" class="field__hint">{{ hint }}</span>
  </label>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 16px;
}
.field__label {
  font-size: 13px;
  color: var(--m-color-on-surface-variant-summary);
}
.field__control {
  width: 100%;
  box-sizing: border-box;
  padding: 12px 14px;
  border: none;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
  color: var(--m-color-on-surface);
  font-size: 16px;
  font-family: inherit;
  outline: none;
  resize: vertical;
}
.field__control--area {
  min-height: 96px;
  line-height: 1.5;
  word-break: break-all;
}
.field__control--mono {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 13px;
}
.field__control:focus {
  box-shadow: 0 0 0 2px var(--m-color-primary);
}
.field__control:disabled {
  opacity: 0.6;
}
.field__hint {
  font-size: var(--m-text-footnote1-size);
  color: var(--m-color-on-surface-variant-actions);
}
</style>
