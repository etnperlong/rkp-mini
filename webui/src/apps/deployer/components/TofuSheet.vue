<script setup lang="ts">
// Trust on first use (§7.4, §19.3). The facts shown here are bound: accepting
// passes exactly `plaintext_sha256` back to `fetch --accept-first`. A mismatch
// means the remote changed after review, so nothing is approved implicitly.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixIcon,
  MiuixIconButton,
  MiuixText,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { t } from '../../../shared/lib/i18n'
import { leafLabel, providerLabel, shortHash, stamp } from '../lib/display'
import { nowUnix } from '../../../shared/lib/format'
import type { TofuFacts } from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const props = defineProps<{ facts: TofuFacts | null; mismatch: boolean }>()
const emit = defineEmits<{ accept: [plaintextSha256: string]; retry: [] }>()

const checked = ref(false)
const now = ref(nowUnix())

watch(
  () => [open.value, props.facts?.plaintext_sha256, props.mismatch] as const,
  () => {
    checked.value = false
    now.value = nowUnix()
  },
)

const canAccept = computed(() => !!props.facts && checked.value)

function accept(): void {
  if (!props.facts || !checked.value) return
  emit('accept', props.facts.plaintext_sha256)
}
</script>

<template>
  <MiuixBottomSheet v-model="open" :title="t('deployer.tofu.title')">
    <template #start-action>
      <MiuixIconButton :aria-label="t('shared.common.close')" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div v-if="mismatch" class="banner banner--error">
        <MiuixText type="body2" color="var(--m-color-error)">
          {{ t('deployer.tofu.mismatch') }}
          <template v-if="facts">{{ t('deployer.tofu.mismatchFacts') }}</template>
          <template v-else>{{ t('deployer.tofu.mismatchRetry') }}</template>
        </MiuixText>
        <MiuixButton @click="emit('retry')">{{ t('deployer.tofu.retry') }}</MiuixButton>
      </div>

      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        {{ t('deployer.tofu.intro') }}
      </MiuixText>

      <template v-if="facts">
        <div class="rows">
          <div class="row">
            <span class="row__k">{{ t('deployer.common.storage') }}</span>
            <span class="row__v">{{ providerLabel(facts.provider) }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">{{ t('deployer.tofu.location') }}</span>
            <span class="row__v selectable">{{ facts.location }}</span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.common.stream') }}</span>
            <span class="row__v selectable">{{ facts.stream_id }}</span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.tofu.seqLabel') }}</span>
            <span class="row__v">{{ facts.seq }}</span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.common.issuedAt') }}</span>
            <span class="row__v">{{ stamp(facts.issued_at) }}</span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.common.deviceId') }}</span>
            <span class="row__v selectable">{{ facts.device_id }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">{{ t('deployer.tofu.leafValidity') }}</span>
            <span class="row__v">
              {{ stamp(facts.leaf_not_before) }} → {{ leafLabel(facts.leaf_not_after, now) }}
            </span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.tofu.keyboxHash') }}</span>
            <span class="row__v mono">{{ shortHash(facts.keybox_sha256) }}</span>
          </div>
          <div class="row">
            <span class="row__k">{{ t('deployer.common.contentHash') }}</span>
            <span class="row__v mono">{{ shortHash(facts.plaintext_sha256) }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">{{ t('deployer.tofu.targets') }}</span>
            <span v-if="!facts.targets.length" class="row__v">{{ t('deployer.tofu.noTargets') }}</span>
            <template v-else>
              <span v-for="target in facts.targets" :key="target" class="row__v selectable">{{ target }}</span>
            </template>
          </div>
        </div>

        <div class="sheet-group">
          <MiuixCheckboxPreference
            v-model="checked"
            :title="t('deployer.tofu.ackTitle')"
            :summary="t('deployer.tofu.ackSummary')"
          />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ t('deployer.tofu.footnote') }}
        </MiuixText>

        <MiuixButton type="primary" :disabled="!canAccept" @click="accept">{{ t('deployer.tofu.accept') }}</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.rows {
  border-radius: var(--m-radius-md);
  background: var(--app-sheet-group);
  overflow: hidden;
}
.row {
  display: flex;
  gap: 16px;
  justify-content: space-between;
  padding: 10px 16px;
  font-size: var(--m-text-body2-size);
  border-top: 1px solid color-mix(in srgb, var(--m-color-on-surface) 6%, transparent);
}
.row:first-child {
  border-top: none;
}
.row--stack {
  flex-direction: column;
  gap: 4px;
}
.row__k {
  flex: none;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  text-align: right;
  word-break: break-all;
}
.row--stack .row__v {
  text-align: start;
}
.mono {
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
}
.banner {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.banner--error {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
</style>
