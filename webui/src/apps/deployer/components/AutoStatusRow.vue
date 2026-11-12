<script setup lang="ts">
// Two small cards: are unattended fetch and deploy actually active? Fetch needs
// sync enabled with the current risk accepted, a stored password (prompt mode
// cannot run unattended), one verified remote version, and the loop; deploy
// also needs `auto_deploy`. Tapping a box opens the 部署 tab.
import { computed } from 'vue'
import { MiuixCard, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_WARNING } from '../../../shared/lib/icons'
import type { SyncConfig, SyncStatus } from '../../../shared/lib/sync'

const props = defineProps<{
  status: SyncStatus | null
  /** `config show`: only `auto_deploy` is needed here. */
  config: SyncConfig | null
  loading: boolean
  /** Read failure message, or null when the last read succeeded. */
  error: string | null
}>()
const emit = defineEmits<{ open: [tab: number] }>()

interface Box {
  title: string
  on: boolean
  hint: string
  pending: boolean
  tab: number
}

/** Why automatic fetch is off, or null when it runs. */
const autoFetchOff = computed<string | null>(() => {
  const s = props.status
  if (!s) return props.error ? '读取同步状态失败' : '读取中…'
  if (!s.stream_id) return '未导入同步链接'
  if (!s.enabled) return '同步未启用'
  if (!s.risk_ack_current) return '需重新确认风险'
  if (s.password_mode !== 'secret-file') return '密码模式为 prompt'
  if (!s.sync_state?.verified_at) return '尚未验证远端版本'
  if (!s.loop_running) return '后台进程未运行'
  return null
})

/** Why automatic deploy is off, or null when it runs. */
const autoDeployOff = computed<string | null>(() => {
  if (autoFetchOff.value) return autoFetchOff.value
  if (!props.config) return '读取配置中…'
  if (props.config.auto_deploy === false) return '已在设置中关闭'
  return null
})

const boxes = computed<Box[]>(() => [
  {
    title: '自动拉取',
    on: autoFetchOff.value === null,
    hint: autoFetchOff.value ?? '约每 8 小时检查一次',
    pending: props.loading && !props.status,
    tab: 1,
  },
  {
    title: '自动部署',
    on: autoDeployOff.value === null,
    hint: autoDeployOff.value ?? '拉取后自动写入目标',
    pending: props.loading && !props.status,
    tab: 1,
  },
])
</script>

<template>
  <div class="row">
    <MiuixCard
      v-for="b in boxes"
      :key="b.title"
      class="box"
      press-feedback="sink"
      show-indication
      @click="emit('open', b.tab)"
    >
      <div class="box__body">
        <div class="box__head">
          <MiuixText type="body1" weight="medium">{{ b.title }}</MiuixText>
          <MiuixProgressIndicator v-if="b.pending" type="infinite" :size="20" />
          <SvgIcon
            v-else
            class="box__icon"
            :path="b.on ? ICON_CHECK_CIRCLE : ICON_WARNING"
            :color="b.on ? 'var(--app-ok)' : 'var(--app-warning)'"
          />
        </div>
        <MiuixText type="body2" weight="medium">{{ b.pending ? '读取中' : b.on ? '已启用' : '未启用' }}</MiuixText>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-summary)">{{ b.hint }}</MiuixText>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.row {
  display: flex;
  gap: 12px;
}
.box {
  flex: 1;
  min-width: 0;
}
.box__body {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.box__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 4px;
}
.box__icon {
  width: 22px;
  height: 22px;
  flex: none;
}
</style>
