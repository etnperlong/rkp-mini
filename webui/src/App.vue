<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixText, MiuixTopAppBar, setThemeMode } from 'miuix-vue'
import { exec, hasKsu } from './lib/ksu'

setThemeMode('system')

const version = ref('…')
const uid = ref('')

async function run(cmd: string): Promise<string> {
  if (!hasKsu()) return '未检测到 KernelSU WebUI 接口'
  try {
    const r = await exec(cmd)
    return r.errno === 0 ? r.stdout.trim() : `失败（${r.errno}）${r.stderr.trim()}`
  } catch (e) {
    return String(e)
  }
}

onMounted(async () => {
  version.value = await run('/data/adb/modules/rkp_mini/bin/rkp-mini --version')
})
</script>

<template>
  <MiuixTopAppBar title="RKP Mini" />
  <main class="page">
    <MiuixCard>
      <div class="pad">
        <MiuixText type="body1">二进制版本：{{ version }}</MiuixText>
        <MiuixText v-if="uid" type="body2">id -u：{{ uid }}</MiuixText>
      </div>
    </MiuixCard>
    <MiuixButton type="primary" @click="run('id -u').then((v) => (uid = v))">检测 root</MiuixButton>
  </main>
</template>

<style scoped>
.page {
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.pad {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
</style>
