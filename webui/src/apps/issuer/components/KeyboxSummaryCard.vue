<script setup lang="ts">
// Read-only one-line keybox state for the issue tab; details stay on the status tab.
import { computed } from 'vue'
import { MiuixBasicComponent, MiuixCard, MiuixProgressIndicator } from 'miuix-vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { KEYBOX_STATE_LABEL, KEYBOX_STATE_VISUAL, keyboxState, type StatusData } from '../lib/rkp'
import { daysLeft, formatDate } from '../../../shared/lib/format'

const props = defineProps<{ status: StatusData | null; loading: boolean; now: number }>()
const emit = defineEmits<{ detail: [] }>()

const state = computed(() => (props.status ? keyboxState(props.status.keybox, props.now) : null))

const summary = computed(() => {
  const kb = props.status?.keybox
  if (!kb || !state.value) return props.loading ? '正在读取…' : '未读取到状态，请在“状态”页重试'
  switch (state.value) {
    case 'valid':
    case 'expiring':
      return kb.not_after != null
        ? `${KEYBOX_STATE_LABEL[state.value]} · 剩余 ${daysLeft(kb.not_after, props.now)} 天（至 ${formatDate(kb.not_after)}）`
        : KEYBOX_STATE_LABEL[state.value]
    case 'expired':
      return kb.not_after != null ? `已于 ${formatDate(kb.not_after)} 过期` : '已过期'
    case 'invalid':
      return `无效${kb.error ? `：${kb.error}` : ''}`
    case 'missing':
      return '尚未签发证书'
  }
})
</script>

<template>
  <MiuixCard>
    <MiuixBasicComponent
      title="当前证书"
      :summary="summary"
      :clickable="!!status?.keybox.exists"
      @click="status?.keybox.exists && emit('detail')"
    >
      <template #start>
        <MiuixProgressIndicator v-if="loading && !state" type="infinite" :size="24" />
        <SvgIcon v-else-if="state" class="mark" :path="KEYBOX_STATE_VISUAL[state].icon" :color="KEYBOX_STATE_VISUAL[state].color" />
      </template>
    </MiuixBasicComponent>
  </MiuixCard>
</template>

<style scoped>
.mark {
  width: 28px;
  height: 28px;
  margin-right: 12px;
  flex: none;
}
</style>
