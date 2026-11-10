#!/system/bin/sh
# Deployer loop: `sh run.sh auto` (verified gate + fetch + deploy) about every
# 8h. Exits as soon as sync is disabled. Doze/vendor power saving may delay or
# freeze it; the module Action or the WebUI run task always works by hand.
# Only step envelopes/diagnostics are appended below (no secrets or keybox
# contents; the CLIs never emit them).
MODDIR=${0%/*}
DATA=/data/adb/rkp_sync
CONFIG=$DATA/config.toml
LOG=$DATA/logs/sync.log
LOCK=$DATA/tmp/sync-loop.pid
RESULT=$DATA/tmp/sync-auto
INTERVAL=28800
SYNC="$MODDIR/bin/rkp-mini-sync"

umask 077
mkdir -p "$DATA/logs" "$DATA/tmp"
chmod 0700 "$DATA" "$DATA/logs" "$DATA/tmp"

if [ -f "$LOCK" ] && kill -0 "$(cat "$LOCK" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi
echo $$ >"$LOCK"
trap 'rm -f "$LOCK"' EXIT

rotate() {
  if [ "$(wc -c <"$LOG" 2>/dev/null || echo 0)" -gt 262144 ]; then
    mv -f "$LOG" "$LOG.1"
  fi
}

log() {
  echo "$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*" >>"$LOG"
  rotate
}

# Append the last 16 KiB of a step file (bounded, same window the WebUI
# snapshot uses), ending on a newline.
append_block() {
  [ -f "$1" ] && [ -s "$1" ] || return 0
  tail -c 16384 "$1" >>"$LOG"
  if [ "$(tail -c 1 "$1" | wc -l)" -ne 1 ]; then
    printf '\n' >>"$LOG"
  fi
}

append_step() {
  # $1 prefix, $2 step name; absent sidecars mean the step did not run.
  [ -f "$1.code" ] || return 0
  log "auto $2 exit=$(cat "$1.code")"
  append_block "$1.json"
  append_block "$1.err"
  rotate
}

jitter() {
  n=$(od -An -N2 -tu2 /dev/urandom 2>/dev/null | tr -d ' ')
  echo $(( ${n:-0} % 1800 ))
}

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 10; done
sleep $(( 60 + $(jitter) / 10 ))

while :; do
  if ! "$SYNC" --role receiver --config "$CONFIG" check-auto >/dev/null 2>>"$LOG"; then
    log "sync disabled or not ready; loop exits"
    exit 0
  fi
  sh "$MODDIR/run.sh" auto >>"$LOG" 2>&1
  log "auto exit=$?"
  append_step "$RESULT.preflight" preflight
  append_step "$RESULT.fetch" fetch
  append_step "$RESULT.deploy" deploy
  sleep $(( INTERVAL + $(jitter) ))
done
