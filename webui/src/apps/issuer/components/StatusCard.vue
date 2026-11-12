<script setup lang="ts">
import { computed } from 'vue'
import { MiuixButton, MiuixCard, MiuixIcon, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { ChevronForward } from 'miuix-vue/icons'
import FoldBlock from '../../../shared/components/FoldBlock.vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { CommandError } from '../../../shared/lib/command'
import { KEYBOX_STATE_LABEL, KEYBOX_STATE_VISUAL, keyboxState, type StatusData } from '../lib/rkp'
import { daysLeft, formatAgo, formatDateTime } from '../../../shared/lib/format'

const props = defineProps<{
  status: StatusData | null
  loading: boolean
  error: CommandError | null
  /** Shown data came from status.json because an issuance is running. */
  cached: boolean
  now: number
}>()

const emit = defineEmits<{ detail: []; retry: [] }>()

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
  <!-- KernelSU Miuix home style: state-tinted card with an oversized icon clipped at the bottom-right corner. -->
  <MiuixCard
    :class="state && `status-card--${state}`"
    :press-feedback="kb?.exists ? 'tilt' : 'none'"
    :show-indication="!!kb?.exists"
    @click="kb?.exists && emit('detail')"
  >
    <SvgIcon v-if="state" class="status__mark" :path="KEYBOX_STATE_VISUAL[state].icon" :color="KEYBOX_STATE_VISUAL[state].color" />
    <div class="status">
      <div class="status__head">
        <MiuixText type="title4" weight="medium">证书状态</MiuixText>
        <MiuixProgressIndicator v-if="loading" type="infinite" :size="18" />
        <MiuixText v-else-if="state" type="body2" weight="medium">{{ KEYBOX_STATE_LABEL[state] }}</MiuixText>
      </div>

      <!-- Read failure with no data to fall back on -->
      <template v-if="error && !status">
        <MiuixText type="body1" color="var(--m-color-error)">读取状态失败：{{ ERROR_KIND[error.kind] }}</MiuixText>
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">{{ error.message }}</MiuixText>
      </template>

      <template v-else-if="status && kb && state">
        <div v-if="state === 'valid' || state === 'expiring'" class="status__hero">
          <span class="status__days">{{ days }}</span>
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
/* MiuixCard paints its background from --m-card-color. Without color-mix() the
   warning/error cards keep the default surface; the corner icon still shows the state. */
.status-card--valid {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .status-card--expiring {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .status-card--expired,
  .status-card--invalid {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.status__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.status,
.status ~ * {
  position: relative;
}
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
  font-variant-numeric: tabular-nums;
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
/* Translucent so the tinted card shows through. */
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .status__stats {
    background: color-mix(in srgb, var(--m-color-surface-container) 55%, transparent);
  }
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
