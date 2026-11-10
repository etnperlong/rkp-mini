#!/system/bin/sh
# RKP-Mini Deployer WebUI task runner (receiver role only). Started by the
# WebUI as root via `setsid sh <this file> <op> [arg]` (or nohup), detached
# from the page. State lives in $TMP; the page polls:
#   sync-task.lock/       directory, present while running (mtime = start)
#   sync-task.last.code   exit code, written last (mtime = end)
#   sync-task.last.json   stdout: single --json envelope  (fetch only)
#   sync-task.last.err    stderr diagnostics (seq/hash/codes only)
#   sync-task.last.op     operation name
#   sync-task.last.steps  composed run: 8-section snapshot written by run.sh
# `run` composes fetch + deploy through the module's run.sh and therefore does
# not publish last.json/last.code itself.
set -u

# <module>/webroot/runner/sync.sh -> <module>
RUNNER_DIR=${0%/*}
MODDIR=${RUNNER_DIR%/webroot/runner}
BIN=$MODDIR/bin/rkp-mini-sync
DATA=/data/adb/rkp_sync
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

# Reject bad requests before touching $TMP or the task lock.
case "$OP" in
  fetch)
    if [ -n "$ARG" ]; then
      valid_sha "$ARG" || exit 2
    fi
    ;;
  run) ;;
  *) exit 2 ;;
esac

umask 077
mkdir -p "$TMP" || exit 9
mkdir "$TMP/sync-task.lock" 2>/dev/null || exit 9   # a task is already running
trap 'rmdir "$TMP/sync-task.lock" 2>/dev/null' EXIT

case "$OP" in
  fetch)
    if [ -n "$ARG" ]; then
      set -- fetch --accept-first "$ARG"
    else
      set -- fetch
    fi
    rm -f "$TMP/sync-task.last.json" "$TMP/sync-task.last.err" "$TMP/sync-task.last.code" "$TMP/sync-task.last.op"
    printf '%s\n' "$OP" >"$TMP/sync-task.last.op"
    "$BIN" --role receiver --config "$CONFIG" --json "$@" \
      >"$TMP/sync-task.last.json" 2>"$TMP/sync-task.last.err"
    code=$?
    printf '%s\n' "$code" >"$TMP/sync-task.last.code"
    ;;
  run)
    # Drop the old completion marker immediately: run.sh publishes the steps
    # snapshot first and the code last, so a stale code cannot outlive a new
    # launch. The UI task lock above remains held for the whole run.
    rm -f "$TMP/sync-task.last.code"
    sh "$MODDIR/run.sh" run
    ;;
  *) exit 2 ;;
esac
