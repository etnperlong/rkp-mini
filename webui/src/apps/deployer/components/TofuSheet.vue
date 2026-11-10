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
  <MiuixBottomSheet v-model="open" title="首次拉取核对">
    <template #start-action>
      <MiuixIconButton aria-label="关闭" @click="open = false">
        <MiuixIcon :icon="Close" />
      </MiuixIconButton>
    </template>

    <div class="sheet-body">
      <div v-if="mismatch" class="banner banner--error">
        <MiuixText type="body2" color="var(--m-color-error)">
          远端内容与你刚才核对的不同（confirmation_mismatch），因此没有任何内容被接受。
          <template v-if="facts">下面是远端当前内容，请重新与发送端核对。</template>
          <template v-else>请重新拉取以获取最新事实。</template>
        </MiuixText>
        <MiuixButton @click="emit('retry')">重新拉取以再次核对</MiuixButton>
      </div>

      <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
        这是本机第一次验证该同步流。信任前请与发送端核对下列事实；确认后这份 keybox 会被暂存，部署仍需另行批准。
      </MiuixText>

      <template v-if="facts">
        <div class="rows">
          <div class="row">
            <span class="row__k">存储方式</span>
            <span class="row__v">{{ providerLabel(facts.provider) }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">位置</span>
            <span class="row__v selectable">{{ facts.location }}</span>
          </div>
          <div class="row">
            <span class="row__k">同步流</span>
            <span class="row__v selectable">{{ facts.stream_id }}</span>
          </div>
          <div class="row">
            <span class="row__k">版本 seq</span>
            <span class="row__v">{{ facts.seq }}</span>
          </div>
          <div class="row">
            <span class="row__k">签发时间</span>
            <span class="row__v">{{ stamp(facts.issued_at) }}</span>
          </div>
          <div class="row">
            <span class="row__k">设备 ID</span>
            <span class="row__v selectable">{{ facts.device_id }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">叶子证书有效期</span>
            <span class="row__v">
              {{ stamp(facts.leaf_not_before) }} → {{ leafLabel(facts.leaf_not_after, now) }}
            </span>
          </div>
          <div class="row">
            <span class="row__k">keybox 哈希</span>
            <span class="row__v mono">{{ shortHash(facts.keybox_sha256) }}</span>
          </div>
          <div class="row">
            <span class="row__k">内容哈希</span>
            <span class="row__v mono">{{ shortHash(facts.plaintext_sha256) }}</span>
          </div>
          <div class="row row--stack">
            <span class="row__k">部署目标（本地配置）</span>
            <span v-if="!facts.targets.length" class="row__v">尚未配置（不会自动写入任何位置）</span>
            <template v-else>
              <span v-for="target in facts.targets" :key="target" class="row__v selectable">{{ target }}</span>
            </template>
          </div>
        </div>

        <div class="sheet-group">
          <MiuixCheckboxPreference
            v-model="checked"
            title="我已与发送端核对上述信息"
            summary="设备 ID、seq、签发时间与哈希来源一致；确认后会以这份内容哈希作为接受值。"
          />
        </div>

        <MiuixText type="footnote1" color="var(--m-color-on-surface-variant-actions)">
          只做了结构与密钥匹配校验；证书签名、根信任与吊销状态未验证。部署目标来自本机配置，远端不能指定路径或命令。
        </MiuixText>

        <MiuixButton type="primary" :disabled="!canAccept" @click="accept">确认并拉取这一版</MiuixButton>
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
