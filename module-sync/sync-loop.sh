#!/system/bin/sh
# Receiver loop: `rkp-sync run --auto` (fetch + deploy) about every 8h.
# Exits as soon as sync is disabled. Doze/vendor power saving may delay or
# freeze it; `rkp-sync run` by hand (or the module Action) always works.
MODDIR=${0%/*}
DATA=/data/adb/rkp_sync
LOG=$DATA/logs/sync.log
LOCK=$DATA/tmp/sync-loop.pid
INTERVAL=28800
SYNC="$MODDIR/bin/rkp-sync"

umask 077
mkdir -p "$DATA/logs" "$DATA/tmp"
chmod 0700 "$DATA" "$DATA/logs" "$DATA/tmp"

if [ -f "$LOCK" ] && kill -0 "$(cat "$LOCK" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi
echo $$ >"$LOCK"
trap 'rm -f "$LOCK"' EXIT

log() {
  echo "$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*" >>"$LOG"
  if [ "$(wc -c <"$LOG" 2>/dev/null || echo 0)" -gt 262144 ]; then
    mv -f "$LOG" "$LOG.1"
  fi
}

jitter() {
  n=$(od -An -N2 -tu2 /dev/urandom 2>/dev/null | tr -d ' ')
  echo $(( ${n:-0} % 1800 ))
}

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 10; done
sleep $(( 60 + $(jitter) / 10 ))

while :; do
  if ! "$SYNC" --role receiver check-auto >/dev/null 2>>"$LOG"; then
    log "sync disabled or not ready; loop exits"
    exit 0
  fi
  "$SYNC" --role receiver --json run --auto >>"$LOG" 2>&1
  log "run exit=$?"
  sleep $(( INTERVAL + $(jitter) ))
done
