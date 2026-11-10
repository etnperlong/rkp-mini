#!/system/bin/sh
# rkp-sync WebUI task runner (sender and receiver modules). Started by the
# WebUI as root via `setsid sh <this file> <op> [arg]` (or nohup), detached
# from the page. State lives in $TMP; the page polls:
#   sync-task.lock/      directory, present while running (mtime = start)
#   sync-task.last.code  exit code, written last (mtime = end)
#   sync-task.last.json  stdout: single --json envelope
#   sync-task.last.err   stderr diagnostics (seq/hash/codes only)
#   sync-task.last.op    operation name
# Operations take no secrets: a new passphrase for `rotate` is read by
# rkp-sync from the 0600 inbox file the page wrote beforehand.
set -u

# <module>/webroot/runner/sync.sh -> <module>
RUNNER_DIR=${0%/*}
MODDIR=${RUNNER_DIR%/webroot/runner}
BIN=$MODDIR/bin/rkp-sync
if [ -x "$MODDIR/bin/rkp-mini" ]; then
  DATA=/data/misc/keystore/rkp-mini
else
  DATA=/data/adb/rkp_sync
fi
CONFIG=$DATA/config.toml
TMP=$DATA/tmp
OP=${1:-}
ARG=${2:-}

valid_sha() {
  case "$1" in
    ''|*[!0-9a-f]*) return 1 ;;
  esac
  [ "${#1}" -ge 16 ] && [ "${#1}" -le 64 ]
}

set -- "$BIN" --config "$CONFIG" --json
case "$OP" in
  push) set -- "$@" push ;;
  rotate) set -- "$@" rotate-password --from-file "$DATA/sync/inbox/rotate.hex" --hex ;;
  fetch)
    if [ -n "$ARG" ]; then
      valid_sha "$ARG" || exit 2
      set -- "$@" fetch --accept-first "$ARG"
    else
      set -- "$@" fetch
    fi
    ;;
  run) set -- "$@" run ;;
  *) exit 2 ;;
esac

umask 077
mkdir -p "$TMP" || exit 9
mkdir "$TMP/sync-task.lock" 2>/dev/null || exit 9   # a task is already running
trap 'rmdir "$TMP/sync-task.lock" 2>/dev/null' EXIT

rm -f "$TMP/sync-task.last.json" "$TMP/sync-task.last.err" "$TMP/sync-task.last.code" "$TMP/sync-task.last.op"
printf '%s\n' "$OP" >"$TMP/sync-task.last.op"
"$@" >"$TMP/sync-task.last.json" 2>"$TMP/sync-task.last.err"
code=$?
printf '%s\n' "$code" >"$TMP/sync-task.last.code"
