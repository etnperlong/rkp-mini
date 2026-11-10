#!/system/bin/sh
# Sender loop for remote keybox sync (rkp-mini-sync). Started by service.sh
# only when `rkp-mini-sync check-auto` passes; exits as soon as sync is
# disabled.
#   every ~12h: [rkp-mini-issuer issue --if-expiring-days N] + sync push --auto
# Android Doze/vendor power saving may delay or freeze this loop; it is not a
# precise scheduler. `rkp-mini-sync push` by hand is always available.
MODDIR=${0%/*}
DATA=/data/misc/keystore/rkp-mini
CONFIG=$DATA/config.toml
LOG=$DATA/logs/sync.log
LOCK=$DATA/tmp/sync-loop.pid
INTERVAL=43200
ISSUER="$MODDIR/bin/rkp-mini-issuer"
SYNC="$MODDIR/bin/rkp-mini-sync"

umask 077
mkdir -p "$DATA/logs" "$DATA/tmp"
chmod 0700 "$DATA/logs" "$DATA/tmp"

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

auto_issue_days() {
  # Top-level key only (before the first [table]).
  sed -n '/^\[/q; s/^auto_issue_days[[:space:]]*=[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$CONFIG" 2>/dev/null
}

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 10; done
sleep $(( 60 + $(jitter) / 10 ))

while :; do
  if ! "$SYNC" --role sender --config "$CONFIG" check-auto >/dev/null 2>>"$LOG"; then
    log "sync disabled or not ready; loop exits"
    exit 0
  fi
  n=$(auto_issue_days)
  if [ "${n:-0}" -gt 0 ] 2>/dev/null; then
    "$ISSUER" issue --acknowledge --json --if-expiring-days "$n" >>"$LOG" 2>&1
    log "issue check exit=$?"
  fi
  "$SYNC" --role sender --config "$CONFIG" --json push --auto >>"$LOG" 2>&1
  log "push exit=$?"
  sleep $(( INTERVAL + $(jitter) ))
done
