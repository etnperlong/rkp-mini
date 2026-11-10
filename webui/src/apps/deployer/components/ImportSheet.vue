<script setup lang="ts">
// Receiver import (§19.5): paste the rkpsync:// link or its JSON, preview with
// `import --check` (writes nothing), then explicitly confirm with `--yes`.
// The link never contains the passphrase; no camera/scanner path exists.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixIcon,
  MiuixIconButton,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import ReceiverField from './ReceiverField.vue'
import { errorMessage, providerLabel } from '../lib/display'
import {
  importApply,
  importCheck,
  type ImportPreview,
  type ImportResult,
  type LocationInfo,
} from '../../../shared/lib/sync'

const open = defineModel<boolean>({ required: true })
const emit = defineEmits<{
  imported: [result: ImportResult]
  risk: []
  secret: [kind: 'age' | 'webdav-password']
}>()

const input = ref('')
const checkedInput = ref('')
const preview = ref<ImportPreview | null>(null)
const result = ref<ImportResult | null>(null)
const checking = ref(false)
const applying = ref(false)
const error = ref<string | null>(null)

watch(open, (v) => {
  if (!v) return
  input.value = ''
  checkedInput.value = ''
  preview.value = null
  result.value = null
  error.value = null
})

// Editing the pasted text invalidates a preview of the previous text; the
// confirmed import must always be what was reviewed.
watch(input, (value) => {
  if (preview.value && normalize(value) !== checkedInput.value) preview.value = null
})

/** Pretty-printed JSON is accepted: compact it before it enters a shell argv. */
function normalize(value: string): string {
  const trimmed = value.trim()
  if (!trimmed.startsWith('{')) return trimmed
  try {
    return JSON.stringify(JSON.parse(trimmed))
  } catch {
    return trimmed
  }
}

async function check(): Promise<void> {
  const value = normalize(input.value)
  if (!value) return
  checking.value = true
  error.value = null
  preview.value = null
  result.value = null
  try {
    preview.value = await importCheck(value)
    checkedInput.value = value
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    checking.value = false
  }
}

async function apply(): Promise<void> {
  if (!preview.value || preview.value.blocked) return
  applying.value = true
  error.value = null
  try {
    result.value = await importApply(checkedInput.value)
    emit('imported', result.value)
    void showSnackbar({ message: '已保存同步位置；远端数据尚未下载' })
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    applying.value = false
  }
}

function locationRows(loc: LocationInfo): { label: string; value: string }[] {
  switch (loc.provider) {
    case 'gist':
      return [
        { label: '最新版直链', value: loc.url ?? '未提供（需要 gist_id 解析）' },
        { label: 'Gist ID', value: loc.gist_id ?? '—' },
        { label: '文件名', value: loc.filename },
      ]
    case 'webdav':
      return [
        { label: 'Endpoint', value: loc.endpoint },
        { label: '路径', value: loc.path },
        { label: '用户名', value: loc.user ?? '匿名' },
      ]
    case 'url':
      return [{ label: 'URL', value: loc.url }]
  }
}

const rows = computed(() => (preview.value ? locationRows(preview.value.location) : []))
const webdavUser = computed(() => {
  const loc = preview.value?.location
  return loc?.provider === 'webdav' ? loc.user : null
})
</script>

<template>
  <MiuixBottomSheet v-model="open" title="导入同步链接">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        粘贴发送端分享的 rkpsync:// 链接或其 JSON。链接只包含存储位置，不包含 age 口令；口令另行设置。
      </MiuixText>

      <div class="sheet-group">
        <ReceiverField
          v-model="input"
          label="同步链接或 JSON"
          placeholder="rkpsync://v1/import?p=…"
          :multiline="true"
          :rows="4"
          :mono="true"
          :disabled="checking || applying"
        />
      </div>

      <MiuixButton type="primary" :disabled="checking || applying || !input.trim()" @click="check">
        {{ checking ? '检查中…' : '检查' }}
      </MiuixButton>

      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <template v-if="preview">
        <div class="preview">
          <div class="preview__head">
            <MiuixText type="title4" weight="medium">解码结果（尚未写入）</MiuixText>
            <MiuixText type="button" color="var(--m-color-primary)">{{ providerLabel(preview.location.provider) }}</MiuixText>
          </div>

          <div class="rows">
            <div class="row">
              <span class="row__k">同步流</span>
              <span class="row__v selectable">{{ preview.stream_id }}</span>
            </div>
            <div v-if="preview.label" class="row">
              <span class="row__k">标签</span>
              <span class="row__v selectable">{{ preview.label }}</span>
            </div>
            <div v-for="r in rows" :key="r.label" class="row row--stack">
              <span class="row__k">{{ r.label }}</span>
              <span class="row__v selectable">{{ r.value }}</span>
            </div>
          </div>

          <div v-if="preview.blocked" class="banner banner--error">
            <MiuixText type="body2" color="var(--m-color-error)">
              本地已跟随另一个同步流并验证到 seq {{ preview.verified_seq ?? 0 }}，不能直接导入。请先在设置中“忘记同步数据”后重试。
            </MiuixText>
          </div>
          <div v-else-if="preview.stream_change" class="banner banner--warn">
            <MiuixText type="body2" color="var(--app-warning)">
              将更换同步流：{{ preview.current_stream_id ?? '未配置' }} → {{ preview.stream_id }}。本地尚无已验证版本，可以继续。
            </MiuixText>
          </div>
          <div v-else-if="preview.current_stream_id" class="banner banner--info">
            <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
              与当前同步流相同，可用于更换存储位置；已验证版本会保留。
            </MiuixText>
          </div>

          <MiuixButton
            type="primary"
            :disabled="applying || preview.blocked"
            @click="apply"
          >
            {{ applying ? '导入中…' : '确认导入' }}
          </MiuixButton>
          <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
            导入只保存位置信息；还需要风险确认、age 口令，并在“拉取”里完成首次 TOFU 核对后才会有 keybox。
          </MiuixText>
        </div>
      </template>

      <template v-if="result">
        <div class="divider"></div>
        <MiuixText type="title4" weight="medium">下一步</MiuixText>
        <ul class="notes">
          <li v-for="note in result.notes" :key="note">{{ note }}</li>
        </ul>
        <div class="sheet-group">
          <MiuixButton v-if="!result.enabled" type="primary" @click="emit('risk')">接受风险并启用同步</MiuixButton>
          <MiuixButton :type="result.enabled ? 'primary' : 'default'" @click="emit('secret', 'age')">
            设置 / 更换 age 口令
          </MiuixButton>
          <MiuixButton v-if="webdavUser" @click="emit('secret', 'webdav-password')">设置 WebDAV 应用密码</MiuixButton>
        </div>
        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          age 口令必须通过另一个安全渠道从发送端获得；不要把它放进链接、二维码或聊天记录。
        </MiuixText>
        <MiuixButton @click="open = false">完成</MiuixButton>
      </template>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.preview__head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 12px;
}
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
  font-family: ui-monospace, 'JetBrains Mono', 'Roboto Mono', monospace;
  font-size: 12px;
}
.banner {
  padding: 12px 14px;
  border-radius: var(--m-radius-md);
}
.banner--error {
  background: color-mix(in srgb, var(--m-color-error) 12%, transparent);
}
.banner--warn {
  background: color-mix(in srgb, var(--app-warning) 14%, transparent);
}
.banner--info {
  background: var(--m-color-secondary-variant);
}
.divider {
  height: 1px;
  background: var(--m-color-secondary-variant);
  margin: 4px 0;
}
.notes {
  margin: 0;
  padding-inline-start: 20px;
  color: var(--m-color-on-surface-variant-summary);
  font-size: var(--m-text-footnote1-size);
  line-height: 1.6;
  word-break: break-word;
}
.sheet-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
}
</style>
