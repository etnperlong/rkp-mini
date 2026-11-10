#!/system/bin/sh
# RKP-Mini Issuer WebUI task runner (sender role only). Started by the WebUI
# as root via `setsid sh <this file> <op> [arg]` (or nohup), detached from the
# page. State lives in $TMP; the page polls:
#   sync-task.lock/      directory, present while running (mtime = start)
#   sync-task.last.code  exit code, written last (mtime = end)
#   sync-task.last.json  stdout: single --json envelope
#   sync-task.last.err   stderr diagnostics (seq/hash/codes only)
#   sync-task.last.op    operation name
# Operations take no secrets: a new passphrase for `rotate` is read by
# rkp-mini-sync from the 0600 inbox file the page wrote beforehand.
set -u

# <module>/webroot/runner/sync.sh -> <module>
RUNNER_DIR=${0%/*}
MODDIR=${RUNNER_DIR%/webroot/runner}
BIN=$MODDIR/bin/rkp-mini-sync
DATA=/data/misc/keystore/rkp-mini
CONFIG=$DATA/config.toml
TMP=$DATA/tmp
op=${1:-}

case "$op" in
  push)
    set -- push
    ;;
  rotate)
    set -- rotate-password --from-file "$DATA/sync/inbox/rotate.hex" --hex
    ;;
  *) exit 2 ;;
esac

umask 077
mkdir -p "$TMP" || exit 9
mkdir "$TMP/sync-task.lock" 2>/dev/null || exit 9   # a task is already running
trap 'rmdir "$TMP/sync-task.lock" 2>/dev/null' EXIT

rm -f "$TMP/sync-task.last.json" "$TMP/sync-task.last.err" "$TMP/sync-task.last.code" "$TMP/sync-task.last.op"
printf '%s\n' "$op" >"$TMP/sync-task.last.op"
"$BIN" --role sender --config "$CONFIG" --json "$@" >"$TMP/sync-task.last.json" 2>"$TMP/sync-task.last.err"
code=$?
printf '%s\n' "$code" >"$TMP/sync-task.last.code"
