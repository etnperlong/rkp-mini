#!/system/bin/sh
# Deployer loop: `sh run.sh auto` (verified gate + fetch + deploy) about every
# 8h. Exits as soon as sync is disabled. Doze/vendor power saving may delay or
# freeze it; the module Action or the WebUI run task always works by hand.
# Only step envelopes/diagnostics are appended below (no secrets or keybox
# contents; the CLIs never emit them).
#
# `sync-loop.sh once` (the module Action) runs one round in the foreground:
# the same round the loop runs, printed to the console and appended to
# logs/sync.log. A round holds tmp/sync-round.pid, so the Action and the loop
# never overlap (both write the tmp/sync-auto.* sidecars).
#
# The RKP_MINI_* overrides exist only for the host test harness; production
# always uses the fixed paths below.
MODDIR=${RKP_MINI_MODDIR:-${0%/*}}
DATA=${RKP_MINI_DATA:-/data/adb/rkp_sync}
CONFIG=$DATA/config.toml
LOG=$DATA/logs/sync.log
LOCK=$DATA/tmp/sync-loop.pid
ROUND=$DATA/tmp/sync-round.pid
RESULT=$DATA/tmp/sync-auto
INTERVAL=28800
SYNC="$MODDIR/bin/rkp-mini-sync"

umask 077
mkdir -p "$DATA/logs" "$DATA/tmp"
chmod 0700 "$DATA" "$DATA/logs" "$DATA/tmp"

rotate() {
  if [ "$(wc -c <"$LOG" 2>/dev/null || echo 0)" -gt 262144 ]; then
    mv -f "$LOG" "$LOG.1"
  fi
}

log() {
  echo "$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*" >>"$LOG"
  rotate
}

# Round output goes to stdout; the caller sends it to the log (and console).
say() {
  echo "$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"
}

# Last 16 KiB of a step file (bounded, same window the WebUI snapshot uses),
# ending on a newline.
show() {
  [ -f "$1" ] && [ -s "$1" ] || return 0
  tail -c 16384 "$1"
  if [ "$(tail -c 1 "$1" | wc -l)" -ne 1 ]; then
    printf '\n'
  fi
}

show_step() {
  # $1 prefix, $2 step name; absent sidecars mean the step did not run.
  [ -f "$1.code" ] || return 0
  say "auto $2 exit=$(cat "$1.code")"
  show "$1.json"
  show "$1.err"
}

jitter() {
  n=$(od -An -N2 -tu2 /dev/urandom 2>/dev/null | tr -d ' ')
  echo $(( ${n:-0} % 1800 ))
}

# Round lock: a pid file, so a killed round never blocks the next one.
take_round() {
  if [ -f "$ROUND" ] && kill -0 "$(cat "$ROUND" 2>/dev/null)" 2>/dev/null; then
    return 1
  fi
  echo $$ >"$ROUND"
}

round() {
  say "round start"
  sh "$MODDIR/run.sh" auto 2>&1
  say "auto exit=$?"
  show_step "$RESULT.preflight" preflight
  show_step "$RESULT.fetch" fetch
  show_step "$RESULT.deploy" deploy
  say "round end"
}

if [ "${1:-}" = once ]; then
  if ! take_round; then
    echo "另一轮自动任务正在运行，请稍后再试。"
    exit 9
  fi
  trap 'rm -f "$ROUND"' EXIT
  echo "手动执行一轮自动任务（检查 → 拉取 → 部署），日志同时写入 $LOG"
  round 2>&1 | tee -a "$LOG"
  rotate
  echo "完成。"
  exit 0
fi

if [ -f "$LOCK" ] && kill -0 "$(cat "$LOCK" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi
echo $$ >"$LOCK"
trap 'rm -f "$LOCK"' EXIT

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 10; done
sleep $(( 60 + $(jitter) / 10 ))

while :; do
  if ! "$SYNC" --role receiver --config "$CONFIG" check-auto >/dev/null 2>>"$LOG"; then
    log "sync disabled or not ready; loop exits"
    exit 0
  fi
  if take_round; then
    round >>"$LOG" 2>&1
    rm -f "$ROUND"
    rotate
  else
    log "round skipped: another round is running"
  fi
  sleep $(( INTERVAL + $(jitter) ))
done
