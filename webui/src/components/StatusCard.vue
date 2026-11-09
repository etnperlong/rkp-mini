<script setup lang="ts">
import { computed } from 'vue'
import { MiuixButton, MiuixCard, MiuixIcon, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import StatusPill, { type PillTone } from './StatusPill.vue'
import FoldBlock from './FoldBlock.vue'
import {
  CommandError,
  KEYBOX_STATE_LABEL,
  keyboxState,
  type KeyboxState,
  type StatusData,
} from '../lib/rkp'
import { daysLeft, formatAgo, formatDateTime } from '../lib/format'

const props = defineProps<{
  status: StatusData | null
  loading: boolean
  error: CommandError | null
  /** Shown data came from status.json because an issuance is running. */
  cached: boolean
  now: number
}>()

const emit = defineEmits<{ detail: []; retry: [] }>()

const TONE: Record<KeyboxState, PillTone> = {
  valid: 'ok',
  expiring: 'warning',
  expired: 'error',
  invalid: 'error',
  missing: 'neutral',
}

const ERROR_KIND: Record<CommandError['kind'], string> = {
  exec: '无法执行命令',
  exit: '命令返回错误',
  parse: '输出无法解析',
}

const state = computed(() => (props.status ? keyboxState(props.status.keybox, props.now) : null))
const kb = computed(() => props.status?.keybox ?? null)
const days = computed(() => (kb.value?.not_after != null ? daysLeft(kb.value.not_after, props.now) : null))
</script>

<template>
  <MiuixCard :show-indication="!!kb?.exists" @click="kb?.exists && emit('detail')">
    <div class="status">
      <div class="status__head">
        <MiuixText type="title4" weight="medium">证书状态</MiuixText>
        <MiuixProgressIndicator v-if="loading" type="infinite" :size="18" />
        <StatusPill v-else-if="state" :tone="TONE[state]" :label="KEYBOX_STATE_LABEL[state]" />
      </div>

      <!-- Read failure with no data to fall back on -->
      <template v-if="error && !status">
        <MiuixText type="body1" color="var(--m-color-error)">读取状态失败：{{ ERROR_KIND[error.kind] }}</MiuixText>
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">{{ error.message }}</MiuixText>
      </template>

      <template v-else-if="status && kb && state">
        <div v-if="state === 'valid' || state === 'expiring'" class="status__hero">
          <span class="status__days" :class="{ 'status__days--warn': state === 'expiring' }">{{ days }}</span>
          <MiuixText type="body1">天后过期</MiuixText>
        </div>
        <MiuixText v-else-if="state === 'expired'" type="title3" color="var(--m-color-error)">证书已过期</MiuixText>
        <MiuixText v-else-if="state === 'invalid'" type="title3" color="var(--m-color-error)">keybox 无效</MiuixText>
        <MiuixText v-else type="title3">尚未签发证书</MiuixText>

        <MiuixText v-if="kb.not_after != null" type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ state === 'expired' ? '过期于' : '有效期至' }} {{ formatDateTime(kb.not_after) }}
        </MiuixText>
        <MiuixText v-if="state === 'invalid' && kb.error" type="body2" color="var(--m-color-error)">
          {{ kb.error }}
        </MiuixText>

        <div v-if="kb.exists" class="status__stats">
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">证书链</MiuixText>
            <MiuixText type="body1" weight="medium">{{ kb.certificates }} 张</MiuixText>
          </div>
          <div class="stat">
            <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">密钥匹配</MiuixText>
            <MiuixText type="body1" weight="medium" :color="kb.key_match ? undefined : 'var(--m-color-error)'">
              {{ kb.key_match ? '是' : '否' }}
            </MiuixText>
          </div>
          <MiuixIcon class="stat__more" :icon="ChevronForward" :size="18" color="var(--m-color-on-surface-variant-actions)" />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          {{ cached ? '签发进行中，显示缓存状态 · ' : '' }}检查于 {{ formatAgo(status.generated_at, now) }}
        </MiuixText>
        <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">
          刷新失败（{{ ERROR_KIND[error.kind] }}）：{{ error.message }}
        </MiuixText>
      </template>

      <MiuixText v-else-if="loading" type="body2" color="var(--m-color-on-surface-variant-summary)">正在读取…</MiuixText>
    </div>

    <template v-if="error">
      <FoldBlock v-if="error.detail" title="错误详情" :text="error.detail" @click.stop />
      <div v-if="!status" class="status__retry" @click.stop>
        <MiuixButton :disabled="loading" @click="emit('retry')">重试</MiuixButton>
      </div>
    </template>
  </MiuixCard>
</template>

<style scoped>
.status {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.status__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.status__hero {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.status__days {
  font-size: 44px;
  font-weight: 600;
  line-height: 1.1;
  color: var(--m-color-primary);
  font-variant-numeric: tabular-nums;
}
.status__days--warn {
  color: var(--app-warning);
}
.status__stats {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 8px 0 4px;
  padding: 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.stat {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.stat__more {
  flex: none;
}
.status__retry {
  padding: 0 16px 16px;
  display: flex;
}
.status__retry > * {
  flex: 1;
}
</style>
