<script setup lang="ts">
// 部署 tab: remote deploy card in the issuer SyncStatusCard style. Every shown
// fact comes from the local `rkp-mini-sync status` read plus its cached
// `check-remote` record; the corner state doubles as the check-remote control
// (read-only: it never fetches or deploys). The page never renders a keybox,
// passphrase, token or WebDAV password.
import { computed } from 'vue'
import { MiuixButton, MiuixCard, MiuixIcon, MiuixIconButton, MiuixProgressIndicator, MiuixText } from 'miuix-vue'
import { ChevronForward, Close } from 'miuix-vue/icons'
import CardStateButton from '../../../shared/components/CardStateButton.vue'
import SvgIcon from '../../../shared/components/SvgIcon.vue'
import { ICON_CHECK_CIRCLE, ICON_ERROR, ICON_HELP, ICON_WARNING } from '../../../shared/lib/icons'
import { formatAgo, formatElapsed } from '../../../shared/lib/format'
import {
  syncCodeLabel,
  syncErrorText,
  targetLabel,
  type FetchResult,
  type RemoteCheck,
  type SecretKind,
  type SecretStatus,
  type SyncConfig,
  type SyncOp,
  type SyncRunner,
  type SyncStatus,
} from '../../../shared/lib/sync'
import { t, type MessageKey } from '../../../shared/lib/i18n'
import { deployCodeLabel, pendingTitle, providerLabel, receiverCheckLabel } from '../lib/display'
import { runOutcomeLabel, runRebootHint, type RunDeployOutcome } from '../lib/run'

const props = defineProps<{
  status: SyncStatus | null
  /** `status` failed with `not_initialized`: the `[sync]` table does not exist yet. */
  notInitialized: boolean
  loading: boolean
  /** Read failure with no usable status. */
  error: string | null
  /** `config show`: only `auto_deploy` and the WebDAV user matter here. */
  config: SyncConfig | null
  runner: SyncRunner
  /** Op started from this page; null when the task predates the page. */
  busyOp: SyncOp | null
  now: number
  /** A check may start now (no task running, no stale lock). */
  canRefresh: boolean
  /** Deploy outcome of the last combined run (null when there is no run snapshot). */
  runOutcome: RunDeployOutcome | null
}>()
const emit = defineEmits<{
  import: []
  check: []
  run: []
  deploy: []
  risk: []
  secret: [kind: SecretKind]
  result: []
}>()

const r = props.runner

type Tone = 'ok' | 'warning' | 'error' | 'neutral'
const TONE_VISUAL: Record<Tone, { color: string; icon: string }> = {
  ok: { color: 'var(--app-ok)', icon: ICON_CHECK_CIRCLE },
  warning: { color: 'var(--app-warning)', icon: ICON_WARNING },
  error: { color: 'var(--m-color-error)', icon: ICON_ERROR },
  neutral: { color: 'var(--m-color-on-surface-variant-actions)', icon: ICON_HELP },
}
const TONE_LABEL: Record<Tone, MessageKey> = {
  ok: 'deployer.statusCard.toneOk',
  warning: 'deployer.statusCard.toneWarning',
  error: 'deployer.statusCard.toneError',
  neutral: 'deployer.statusCard.toneNeutral',
}
const BUSY_LABEL: Record<string, MessageKey> = {
  check: 'deployer.common.busyCheck',
  fetch: 'deployer.common.busyFetch',
  run: 'deployer.common.busyRun',
}

const busyText = computed(() => {
  const label = t(BUSY_LABEL[props.busyOp ?? 'run'] ?? 'deployer.common.busyRun')
  const elapsed = formatElapsed(r.elapsedMs.value)
  return r.overdue.value
    ? t('deployer.common.busyUnknown', { label, elapsed })
    : t('deployer.common.busyTimer', { label, elapsed })
})

const needsImport = computed(() => props.notInitialized || (!!props.status && !props.status.stream_id))
const passwordModePrompt = computed(() => props.status?.password_mode === 'prompt')
const webdavNeeded = computed(() => props.status?.provider === 'webdav' && !!props.config?.webdav_user)

/** Why automatic deploy is off, or null when the loop deploys. */
const autoDeployOff = computed<string | null>(() => {
  const s = props.status
  if (!s) return null
  if (!s.enabled) return t('deployer.common.syncDisabled')
  if (!s.risk_ack_current) return t('deployer.common.riskStale')
  if (props.config?.auto_deploy === false) return t('deployer.common.autoDeployOff')
  if (!s.loop_running) return t('deployer.common.loopOff')
  return null
})

const rows = computed(() => {
  const s = props.status
  if (!s) return []
  const verified = s.sync_state?.verified_at ?? null
  return [
    { k: t('deployer.common.storage'), v: providerLabel(s.provider) },
    { k: t('deployer.statusCard.syncTime'), v: verified ? formatAgo(verified, props.now) : t('deployer.statusCard.notFetched') },
    {
      k: t('deployer.common.autoDeploy'),
      v: autoDeployOff.value
        ? t('deployer.statusCard.autoDeployOff', { reason: autoDeployOff.value })
        : t('deployer.statusCard.autoDeployOn'),
    },
  ]
})

type WarningAction = 'deploy' | 'check' | 'risk' | 'deploy-sheet' | 'secret-age' | 'secret-webdav'
interface WarningItem {
  id: string
  text: string
  action: WarningAction | null
}

function secretWarning(value: SecretStatus | undefined, name: string, action: WarningAction): WarningItem | null {
  if (!value) return null
  if (!value.present) return { id: `${action}-missing`, text: t('deployer.statusCard.secretMissing', { name }), action }
  if (!value.ok) return { id: `${action}-mode`, text: t('deployer.statusCard.secretBadMode', { name }), action }
  return null
}

const warnings = computed<WarningItem[]>(() => {
  const s = props.status
  if (!s) return []
  const out: WarningItem[] = []

  const check = s.remote_check
  if (check && check.outcome !== 'in_sync') {
    if (check.outcome === 'remote_ahead') {
      const seq = check.remote?.seq
      out.push({
        id: 'remote-ahead',
        text:
          seq != null
            ? t('deployer.statusCard.remoteAheadSeq', { seq, time: formatAgo(check.at, props.now) })
            : t('deployer.statusCard.remoteAhead', { time: formatAgo(check.at, props.now) }),
        action: 'deploy',
      })
    } else {
      const detail =
        check.outcome === 'error' && check.error ? syncErrorText(check.error) : receiverCheckLabel(check.outcome)
      out.push({
        id: `remote-${check.outcome}`,
        text: t('deployer.statusCard.checkDetail', { detail, time: formatAgo(check.at, props.now) }),
        action: check.outcome === 'error' ? 'check' : null,
      })
    }
  }

  const pending = s.deploy_state?.pending
  if (pending) {
    out.push({
      id: 'pending',
      text: t('deployer.statusCard.pendingReview', { seq: pending.seq, reason: pendingTitle(pending.reason) }),
      action: 'deploy-sheet',
    })
  }

  if (s.password_mode === 'secret-file') {
    for (const w of [
      secretWarning(s.secrets.age, t('deployer.common.password'), 'secret-age'),
      webdavNeeded.value
        ? secretWarning(s.secrets.webdav_password, t('deployer.common.webdavPassword'), 'secret-webdav')
        : null,
    ]) {
      if (w) out.push(w)
    }
  }
  if (passwordModePrompt.value) out.push({ id: 'prompt', text: t('deployer.statusCard.promptMode'), action: null })

  if (!s.risk_ack_current) out.push({ id: 'risk', text: t('deployer.statusCard.riskStale'), action: 'risk' })

  const last = s.sync_state?.last_error
  if (last) {
    out.push({
      id: 'last-error',
      text: t('deployer.statusCard.lastSyncFailed', { reason: syncCodeLabel(last.code), time: formatAgo(last.at, props.now) }),
      action: null,
    })
  }

  for (const tg of s.deploy_state?.targets ?? []) {
    if (tg.last_error) {
      out.push({
        id: `target-${tg.id}`,
        text: t('deployer.statusCard.targetFailed', {
          target: targetLabel(tg.id),
          reason: deployCodeLabel(tg.last_error.code),
        }),
        action: null,
      })
    }
  }
  return out
})

const tone = computed<Tone>(() => {
  const s = props.status
  if (!s) return 'neutral'
  if (s.sync_state?.last_error || (s.deploy_state?.targets ?? []).some((t) => t.last_error)) return 'error'
  if (warnings.value.length) return 'warning'
  if (s.remote_check?.outcome === 'in_sync' && s.enabled) return 'ok'
  return 'neutral'
})

/** Last finished task, stale lock or failed launch (busy shows in the corner). */
const lastTask = computed<{ text: string; bad: boolean; steps: boolean; reboot: string | null } | null>(() => {
  if (!r.showLast.value) return null
  const result = r.result.value
  if (!result) return null
  if (result.op === 'run') {
    if (result.code !== 0) {
      return {
        text: t('deployer.statusCard.runFailedExit', { code: result.code }),
        bad: true,
        steps: true,
        reboot: null,
      }
    }
    const outcome = props.runOutcome
    return {
      text: outcome ? runOutcomeLabel(outcome) : t('deployer.run.done'),
      bad: outcome ? outcome.kind === 'failed' : false,
      steps: true,
      reboot: outcome ? runRebootHint(outcome) : null,
    }
  }
  const env = result.envelope
  if (!env) {
    return {
      text: t('deployer.statusCard.taskAbnormal', { code: result.code }),
      bad: result.code !== 0,
      steps: false,
      reboot: null,
    }
  }
  if (!env.ok) return { text: syncErrorText(env.error), bad: true, steps: false, reboot: null }
  if (result.op === 'check') {
    const data = env.data as RemoteCheck
    return {
      text: t('deployer.statusCard.checkLabel', { detail: receiverCheckLabel(data.outcome) }),
      bad: false,
      steps: false,
      reboot: null,
    }
  }
  const data = env.data as FetchResult
  return {
    text: data.changed
      ? t('deployer.statusCard.fetched', { seq: data.seq })
      : t('deployer.statusCard.noNewVersion', { seq: data.seq }),
    bad: false,
    steps: false,
    reboot: null,
  }
})

function clickWarning(action: WarningAction | null): void {
  if (action) onWarning(action)
}

function onWarning(action: WarningAction): void {
  switch (action) {
    case 'deploy':
      emit('run')
      break
    case 'check':
      emit('check')
      break
    case 'risk':
      emit('risk')
      break
    case 'deploy-sheet':
      emit('deploy')
      break
    case 'secret-age':
      emit('secret', 'age')
      break
    case 'secret-webdav':
      emit('secret', 'webdav-password')
      break
  }
}
</script>

<template>
  <!-- KernelSU Miuix home style: state-tinted card with an oversized icon clipped at the bottom-right corner. -->
  <MiuixCard class="deploy-card" :class="status && `deploy-card--${tone}`" press-feedback="none">
    <SvgIcon
      v-if="status"
      class="deploy__mark"
      :path="TONE_VISUAL[tone].icon"
      :color="TONE_VISUAL[tone].color"
    />
    <div class="deploy">
      <div class="deploy__head">
        <MiuixText type="title4" weight="medium">{{ t('deployer.statusCard.title') }}</MiuixText>
        <MiuixProgressIndicator v-if="loading && !status" type="infinite" :size="18" />
        <!-- Corner status doubles as the refresh control: tap runs check-remote. -->
        <CardStateButton
          v-else-if="status"
          :label="r.busy.value ? busyText : t(TONE_LABEL[tone])"
          :busy="r.busy.value"
          :disabled="!canRefresh"
          @refresh="emit('check')"
        />
      </div>

      <!-- Never imported: the link must be imported before anything can run. -->
      <template v-if="needsImport">
        <MiuixText type="body2" color="var(--m-color-on-surface-variant-summary)">
          {{ t('deployer.statusCard.unimportedHint') }}
        </MiuixText>
        <MiuixButton type="primary" @pointerdown.stop @click.stop="emit('import')">{{ t('deployer.statusCard.importLink') }}</MiuixButton>
      </template>

      <MiuixText v-else-if="error && !status" type="body2" color="var(--m-color-error)">
        {{ t('deployer.common.readStatusFailedDetail', { error }) }}
      </MiuixText>

      <template v-else-if="status">
        <div class="rows">
          <div v-for="row in rows" :key="row.k" class="row">
            <span class="row__k">{{ row.k }}</span>
            <span class="row__v">{{ row.v }}</span>
          </div>
        </div>

        <div v-if="warnings.length" class="warnings">
          <div v-for="w in warnings" :key="w.id" class="warning">
            <button
              v-if="w.action"
              type="button"
              class="warning__btn"
              @pointerdown.stop
              @click.stop="clickWarning(w.action)"
            >
              <MiuixText type="footnote1" color="var(--app-warning)">{{ w.text }}</MiuixText>
              <MiuixIcon :icon="ChevronForward" :size="16" color="var(--app-warning)" />
            </button>
            <MiuixText v-else type="footnote1" color="var(--app-warning)">{{ w.text }}</MiuixText>
          </div>
        </div>

        <div v-if="!r.busy.value && lastTask" class="task" @pointerdown.stop @click.stop>
          <button v-if="lastTask.steps" type="button" class="task__link" @click.stop="emit('result')">
            <span class="task__body">
              <MiuixText type="body2" :color="lastTask.bad ? 'var(--m-color-error)' : undefined">{{ lastTask.text }}</MiuixText>
              <MiuixText v-if="lastTask.reboot" type="footnote1" color="var(--app-warning)">{{ lastTask.reboot }}</MiuixText>
            </span>
            <MiuixIcon :icon="ChevronForward" :size="16" color="var(--m-color-on-surface-variant-actions)" />
          </button>
          <MiuixText v-else type="body2" :color="lastTask.bad ? 'var(--m-color-error)' : undefined">{{ lastTask.text }}</MiuixText>
          <MiuixIconButton :aria-label="t('deployer.statusCard.dismiss')" @click.stop="r.dismiss()">
            <MiuixIcon :icon="Close" :size="16" color="var(--m-color-on-surface-variant-actions)" />
          </MiuixIconButton>
        </div>
        <div v-else-if="r.phase.value === 'launch_failed'" class="task" @pointerdown.stop @click.stop>
          <MiuixText type="body2" color="var(--m-color-error)">{{ t('deployer.statusCard.launchFailed') }}</MiuixText>
        </div>
        <div v-else-if="r.phase.value === 'stale'" class="task" @pointerdown.stop @click.stop>
          <MiuixText type="body2" color="var(--m-color-error)">{{ t('deployer.statusCard.staleLock') }}</MiuixText>
          <MiuixButton :disabled="r.busy.value" @click.stop="r.clearStaleLock()">{{ t('deployer.statusCard.clearLock') }}</MiuixButton>
        </div>
      </template>

      <MiuixText v-else type="body2" color="var(--m-color-on-surface-variant-summary)">{{ t('deployer.statusCard.reading') }}</MiuixText>
    </div>
  </MiuixCard>
</template>

<style scoped>
/* MiuixCard paints its background from --m-card-color; the corner icon still
   shows the state where color-mix() is unavailable. */
.deploy-card--ok {
  --m-card-color: var(--app-ok-container);
}
@supports (color: color-mix(in srgb, red 50%, blue)) {
  .deploy-card--warning {
    --m-card-color: color-mix(in srgb, var(--app-warning) 16%, var(--m-color-surface-container));
  }
  .deploy-card--error {
    --m-card-color: color-mix(in srgb, var(--m-color-error) 12%, var(--m-color-surface-container));
  }
}
/* `hidden` still lets focus/scrollIntoView pan the card sideways toward the
   oversized corner icon; `clip` never scrolls. */
.deploy-card :deep(.m-card) {
  overflow: clip;
}
/* KernelSU: 110dp icon, offset (27dp, 31dp) past the bottom-end corner. */
.deploy__mark {
  position: absolute;
  right: -27px;
  bottom: -31px;
  width: 110px;
  height: 110px;
  pointer-events: none;
}
.deploy,
.deploy ~ * {
  position: relative;
}
.deploy {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.deploy__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 28px;
  margin-bottom: 4px;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 4px 0;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  font-size: 14px;
  line-height: 1.45;
}
.row__k {
  flex: none;
  width: 84px;
  color: var(--m-color-on-surface-variant-summary);
}
.row__v {
  flex: 1;
  min-width: 0;
  word-break: break-all;
}
.warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.warning {
  display: flex;
}
.warning__btn {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.warning__btn:active {
  opacity: 0.7;
}
.task {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--m-color-secondary-variant);
}
.task__link {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.task__body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
}
</style>
