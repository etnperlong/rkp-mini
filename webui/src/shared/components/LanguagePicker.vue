<script setup lang="ts">
import { computed, ref } from 'vue'
import { MiuixDropdownPreference, MiuixText } from 'miuix-vue'
import { LOCALE_NAMES, LOCALES, localePref, systemLocale, t, type LocalePref } from '../lib/i18n'
import { changeModuleLocale } from '../lib/moduleLocale'
import { CommandError } from '../lib/command'

// Index 0 follows the system; the rest map to LOCALES in order.
const PREFS: LocalePref[] = ['system', ...LOCALES]

const error = ref<string | null>(null)
let seq = 0

async function select(pref: LocalePref): Promise<void> {
  const mine = ++seq
  error.value = null
  try {
    await changeModuleLocale(pref)
  } catch (e) {
    // A newer selection already reported its own outcome.
    if (mine !== seq) return
    error.value =
      e instanceof CommandError
        ? e.detail
          ? t('shared.common.withDetail', { message: e.message, detail: e.detail })
          : e.message
        : String(e)
  }
}

const items = computed(() =>
  PREFS.map((p) =>
    p === 'system' ? { text: t('shared.language.system'), summary: LOCALE_NAMES[systemLocale()] } : LOCALE_NAMES[p],
  ),
)
const index = computed({
  get: () => PREFS.indexOf(localePref.value),
  set: (i: number) => void select(PREFS[i]),
})
</script>

<template>
  <MiuixDropdownPreference v-model="index" :title="t('shared.language.title')" :items="items" />
  <MiuixText v-if="error" class="language-error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>
</template>

<style scoped>
.language-error {
  padding: 4px 16px 8px;
}
</style>
