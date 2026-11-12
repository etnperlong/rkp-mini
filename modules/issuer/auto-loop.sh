#!/system/bin/sh
# Issuer module loop: auto-issue and auto-push as two independent jobs.
#   auto-issue  while the top-level `auto_issue_days` (config.toml) is > 0:
#               `rkp-mini-issuer issue --acknowledge --json --if-expiring-days N`
#               under the WebUI's tmp/issue.lock, result in
#               tmp/auto-issue.last.{at,json,err,code} (code written last).
#   auto-push   while `rkp-mini-sync check-auto` passes: `push --auto`.
# Sync being off no longer disables auto-issue; issue runs before the push.
#
# Ticks are short (30 min) and compare wall-clock `date +%s` with the time
# stored in tmp/auto-loop.next: `sleep` does not count suspended time, so one
# 12 h sleep would run late and drift. Doze/vendor power saving can still delay
# or freeze a tick, so this is not a precise scheduler: a due round that could
# not run happens on the next tick. A tick where both jobs are off exits 0.
#
# The RKP_MINI_* overrides exist only for the host test harness; production
# always uses the fixed paths below.
MODDIR=${RKP_MINI_MODDIR:-${0%/*}}
DATA=${RKP_MINI_DATA:-/data/misc/keystore/rkp-mini}
TICK=${RKP_MINI_TICK:-1800}

CONFIG=$DATA/config.toml
TMP=$DATA/tmp
LOG=$DATA/logs/auto.log
PID=$TMP/auto-loop.pid
NEXT=$TMP/auto-loop.next
AUTO=$TMP/auto-issue.last
INTERVAL=43200
ISSUER="$MODDIR/bin/rkp-mini-issuer"
SYNC="$MODDIR/bin/rkp-mini-sync"

umask 077
mkdir -p "$DATA/logs" "$TMP"
chmod 0700 "$DATA/logs" "$TMP"

if [ -f "$PID" ] && kill -0 "$(cat "$PID" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi
echo $$ >"$PID"
trap 'rm -f "$PID"' EXIT

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
  n=$(auto_issue_days)
  if "$SYNC" --role sender --config "$CONFIG" check-auto >/dev/null 2>>"$LOG"; then
    push=1
  else
    push=0
  fi
  if [ "${n:-0}" -le 0 ] 2>/dev/null && [ "$push" = 0 ]; then
    log "auto-issue off and sync off/not ready; loop exits"
    exit 0
  fi

  due=$(cat "$NEXT" 2>/dev/null)
  case $due in ''|*[!0-9]*) due=0 ;; esac
  if [ "$(date +%s)" -ge "$due" ]; then
    if [ "${n:-0}" -gt 0 ] 2>/dev/null; then
      if mkdir "$TMP/issue.lock" 2>/dev/null; then
        rm -f "$AUTO.code"
        rm -f "$AUTO.json" "$AUTO.err" "$AUTO.at"
        date +%s >"$AUTO.at"
        "$ISSUER" issue --acknowledge --json --if-expiring-days "$n" >"$AUTO.json" 2>"$AUTO.err"
        code=$?
        printf '%s\n' "$code" >"$AUTO.code"
        rmdir "$TMP/issue.lock" 2>/dev/null
        log "issue exit=$code"
      else
        log "issue skipped: lock held"
      fi
    fi
    if [ "$push" = 1 ]; then
      "$SYNC" --role sender --config "$CONFIG" --json push --auto >>"$LOG" 2>&1
      log "push exit=$?"
    fi
    echo $(( $(date +%s) + INTERVAL + $(jitter) )) >"$NEXT"
  fi

  sleep "$TICK"
done
