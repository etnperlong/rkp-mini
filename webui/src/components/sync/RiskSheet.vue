<script setup lang="ts">
// First-enable risk confirmation (design §13.2, §19.3). The statement comes
// from `rkp-sync risk` so the text and its version have one source; every item
// is a separate, initially unchecked checkbox, and acceptance names the exact
// version shown. Enabling also starts the module's unattended loop.
import { computed, ref, watch } from 'vue'
import {
  MiuixBottomSheet,
  MiuixButton,
  MiuixCheckboxPreference,
  MiuixIcon,
  MiuixIconButton,
  MiuixProgressIndicator,
  MiuixText,
  showSnackbar,
} from 'miuix-vue'
import { Close } from 'miuix-vue/icons'
import { CommandError } from '../../lib/rkp'
import { enableSync, readRisk, startLoop, type RiskStatement } from '../../lib/sync'

const open = defineModel<boolean>({ required: true })
const emit = defineEmits<{ enabled: [] }>()

const statement = ref<RiskStatement | null>(null)
const checked = ref<boolean[]>([])
const loading = ref(false)
const busy = ref(false)
const error = ref<string | null>(null)

function message(e: unknown): string {
  return e instanceof CommandError ? e.message : String(e)
}

watch(open, async (v) => {
  if (!v) return
  statement.value = null
  checked.value = []
  error.value = null
  loading.value = true
  try {
    statement.value = await readRisk()
    checked.value = statement.value.items.map(() => false)
  } catch (e) {
    error.value = message(e)
  } finally {
    loading.value = false
  }
})

const allChecked = computed(() => checked.value.length > 0 && checked.value.every(Boolean))

async function accept(): Promise<void> {
  if (!statement.value || !allChecked.value) return
  busy.value = true
  error.value = null
  try {
    await enableSync(statement.value.version)
    await startLoop()
    open.value = false
    emit('enabled')
    void showSnackbar({ message: '已启用远程同步' })
  } catch (e) {
    error.value = message(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <MiuixBottomSheet v-model="open" title="启用远程同步">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        远程同步会把 keybox（含私钥）加密后放到你自己的存储并部署到其他设备。请逐条阅读并确认：
      </MiuixText>

      <div v-if="loading" class="center"><MiuixProgressIndicator /></div>

      <div v-else-if="statement" class="sheet-group">
        <MiuixCheckboxPreference
          v-for="(item, i) in statement.items"
          :key="item.id"
          v-model="checked[i]"
          :title="item.zh"
          :summary="item.en"
        />
      </div>

      <MiuixText v-if="statement?.passphrase_path" type="footnote1" color="var(--m-color-on-surface-variant-actions)">
        age 密码将保存在 {{ statement.passphrase_path }}（0600）以支持自动运行。风险说明版本 v{{ statement.version }}。
      </MiuixText>
      <MiuixText v-if="error" type="footnote1" color="var(--m-color-error)">{{ error }}</MiuixText>

      <MiuixButton type="primary" :disabled="!allChecked || busy" @click="accept">
        {{ busy ? '启用中…' : '我已逐条了解，启用' }}
      </MiuixButton>
    </div>
  </MiuixBottomSheet>
</template>

<style scoped>
.center {
  display: flex;
  justify-content: center;
  padding: 24px;
}
</style>
