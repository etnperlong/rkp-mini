<script setup lang="ts">
import { computed } from 'vue'
import { MiuixDropdownPreference } from 'miuix-vue'
import { LOCALE_NAMES, LOCALES, localePref, saveLocale, systemLocale, t, type LocalePref } from '../lib/i18n'

// Index 0 follows the system; the rest map to LOCALES in order.
const PREFS: LocalePref[] = ['system', ...LOCALES]

const items = computed(() =>
  PREFS.map((p) =>
    p === 'system' ? { text: t('shared.language.system'), summary: LOCALE_NAMES[systemLocale()] } : LOCALE_NAMES[p],
  ),
)
const index = computed({
  get: () => PREFS.indexOf(localePref.value),
  set: (i: number) => void saveLocale(PREFS[i]),
})
</script>

<template>
  <MiuixDropdownPreference v-model="index" :title="t('shared.language.title')" :items="items" />
</template>
