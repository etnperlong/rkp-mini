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
DEPLOY="$MODDIR/bin/rkp-mini-deployer"

umask 077
mkdir -p "$DATA/logs" "$DATA/tmp"
chmod 0700 "$DATA" "$DATA/logs" "$DATA/tmp"

rotate() {
  if [ "$(wc -c <"$LOG" 2>/dev/null || echo 0)" -gt 262144 ]; then
    mv -f "$LOG" "$LOG.1"
  fi
}

# Action text lives in the deployer catalogs and follows the WebUI locale.
# A formatter failure is left visible; there is no untranslated fallback.
message() {
  "$DEPLOY" __action-message "$@"
}

log() {
  _rkp_line=$(message "$@" 2>>"$LOG") || return 1
  printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$_rkp_line" >>"$LOG"
  rotate
}

# Round output goes to stdout; the caller sends it to the log (and console).
say() {
  _rkp_line=$(message "$@") || return 1
  printf '%s %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" "$_rkp_line"
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
  case "$2" in
    preflight) _rkp_key=preflight_exit ;;
    fetch) _rkp_key=fetch_exit ;;
    deploy) _rkp_key=deploy_exit ;;
    *) return 0 ;;
  esac
  _rkp_code=$(cat "$1.code")
  say "$_rkp_key" "$_rkp_code"
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
  say round_start
  sh "$MODDIR/run.sh" auto 2>&1
  _rkp_auto=$?
  say auto_exit "$_rkp_auto"
  show_step "$RESULT.preflight" preflight
  show_step "$RESULT.fetch" fetch
  show_step "$RESULT.deploy" deploy
  say round_end
}

if [ "${1:-}" = once ]; then
  if ! take_round; then
    message busy || :
    exit 9
  fi
  trap 'rm -f "$ROUND"' EXIT
  # A banner that cannot be rendered stops the run before any real step.
  message start "$LOG" || exit 1
  round 2>&1 | tee -a "$LOG"
  rotate
  message done
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
    log loop_exit
    exit 0
  fi
  if take_round; then
    round >>"$LOG" 2>&1
    rm -f "$ROUND"
    rotate
  else
    log round_skipped
  fi
  sleep $(( INTERVAL + $(jitter) ))
done
