#!/system/bin/sh
# Issuer module loop: auto-issue and auto-push as two independent jobs.
#   auto-issue  while the top-level `auto_issue_days` (config.toml) is > 0:
#               `rkp-mini-issuer issue --acknowledge --json --if-expiring-days N`
#               under the WebUI's tmp/issue.lock, result in
#               tmp/auto-issue.last.{at,json,err,code} (code written last).
#   auto-push   while `rkp-mini-sync check-auto` passes: `push --auto`.
# Sync being off no longer disables auto-issue; issue runs before the push.
#
# `auto-loop.sh once` (the module Action) runs one round in the foreground:
# the same round the loop runs, printed to the console and appended to
# logs/auto.log, and it schedules the loop's next round INTERVAL later.
# A round holds tmp/auto-round.pid, so the Action and the loop never overlap.
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
ROUND=$TMP/auto-round.pid
NEXT=$TMP/auto-loop.next
AUTO=$TMP/auto-issue.last
INTERVAL=43200
ISSUER="$MODDIR/bin/rkp-mini-issuer"
SYNC="$MODDIR/bin/rkp-mini-sync"

umask 077
mkdir -p "$DATA/logs" "$TMP"
chmod 0700 "$DATA/logs" "$TMP"

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

# Last 16 KiB of a file, ending on a newline.
show() {
  [ -f "$1" ] && [ -s "$1" ] || return 0
  tail -c 16384 "$1"
  if [ "$(tail -c 1 "$1" | wc -l)" -ne 1 ]; then
    printf '\n'
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

# Round lock: a pid file, so a killed round never blocks the next one.
take_round() {
  if [ -f "$ROUND" ] && kill -0 "$(cat "$ROUND" 2>/dev/null)" 2>/dev/null; then
    return 1
  fi
  echo $$ >"$ROUND"
}

round() {
  n=$(auto_issue_days)
  say "round start (auto_issue_days=${n:-0})"
  if [ "${n:-0}" -gt 0 ] 2>/dev/null; then
    if mkdir "$TMP/issue.lock" 2>/dev/null; then
      rm -f "$AUTO.code"
      rm -f "$AUTO.json" "$AUTO.err" "$AUTO.at"
      date +%s >"$AUTO.at"
      "$ISSUER" issue --acknowledge --json --if-expiring-days "$n" >"$AUTO.json" 2>"$AUTO.err"
      code=$?
      printf '%s\n' "$code" >"$AUTO.code"
      rmdir "$TMP/issue.lock" 2>/dev/null
      say "issue exit=$code"
      show "$AUTO.json"
      show "$AUTO.err"
    else
      say "issue skipped: lock held (an issuance is running)"
    fi
  else
    say "issue skipped: auto_issue_days = 0"
  fi
  if why=$("$SYNC" --role sender --config "$CONFIG" check-auto 2>&1 >/dev/null); then
    "$SYNC" --role sender --config "$CONFIG" --json push --auto 2>&1
    say "push exit=$?"
  else
    say "push skipped: check-auto exit=$?"
    [ -z "$why" ] || printf '%s\n' "$why"
  fi
  echo $(( $(date +%s) + INTERVAL + $(jitter) )) >"$NEXT"
  say "round end"
}

if [ "${1:-}" = once ]; then
  if ! take_round; then
    echo "另一轮自动任务正在运行，请稍后再试。"
    exit 9
  fi
  trap 'rm -f "$ROUND"' EXIT
  echo "手动执行一轮自动任务（自动签发 + 自动推送），日志同时写入 $LOG"
  round 2>&1 | tee -a "$LOG"
  rotate
  echo "完成。下一轮定时任务约 12 小时后执行。"
  exit 0
fi

if [ -f "$PID" ] && kill -0 "$(cat "$PID" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi
echo $$ >"$PID"
trap 'rm -f "$PID"' EXIT

until [ "$(getprop sys.boot_completed)" = "1" ]; do sleep 10; done
sleep $(( 60 + $(jitter) / 10 ))

while :; do
  n=$(auto_issue_days)
  if [ "${n:-0}" -le 0 ] 2>/dev/null && \
    ! "$SYNC" --role sender --config "$CONFIG" check-auto >/dev/null 2>&1; then
    log "auto-issue off and sync off/not ready; loop exits"
    exit 0
  fi

  due=$(cat "$NEXT" 2>/dev/null)
  case $due in ''|*[!0-9]*) due=0 ;; esac
  if [ "$(date +%s)" -ge "$due" ]; then
    if take_round; then
      round >>"$LOG" 2>&1
      rm -f "$ROUND"
      rotate
    else
      log "round skipped: another round is running"
    fi
  fi

  sleep "$TICK"
done
