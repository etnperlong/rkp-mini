#!/system/bin/sh
# rkp-mini WebUI issue runner. Started by the WebUI as root via
# `setsid sh <this file>` (or nohup), detached from the page lifecycle.
# State lives in files under $TMP; the page polls them:
#   issue.lock/       directory, present while running (mtime = start)
#   issue.last.code   exit code, written last (mtime = end)
#   issue.last.json   stdout: single --json envelope
#   issue.last.err    stderr diagnostics
set -u

# <module>/webroot/runner/issue.sh -> <module>
RUNNER_DIR=${0%/*}
MODDIR=${RUNNER_DIR%/webroot/runner}
BIN=$MODDIR/bin/rkp-mini
DATA=/data/misc/keystore/rkp-mini
TMP=$DATA/tmp

mkdir -p "$TMP" || exit 9
mkdir "$TMP/issue.lock" 2>/dev/null || exit 9   # a task is already running
trap 'rmdir "$TMP/issue.lock" 2>/dev/null' EXIT

rm -f "$TMP/issue.last.json" "$TMP/issue.last.err" "$TMP/issue.last.code"
"$BIN" issue --acknowledge --json >"$TMP/issue.last.json" 2>"$TMP/issue.last.err"
code=$?
printf '%s\n' "$code" >"$TMP/issue.last.code"
