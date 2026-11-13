#!/system/bin/sh
# RKP-Mini Deployer composition entry point. Accepts only:
#   action  foreground fetch + apply (manual, root shell; TTY prompts preserved)
#   run     WebUI fetch + apply task, results in <tmp>/sync-task
#   auto    unattended fetch + apply, results in <tmp>/sync-auto
# The receiver data root stays /data/adb/rkp_sync; binaries come from this
# module only (no PATH fallback).
set -u

MODDIR=${0%/*}
[ "$MODDIR" != "$0" ] || MODDIR=.

SYNC_BIN=$MODDIR/bin/rkp-mini-sync
DEPLOY_BIN=$MODDIR/bin/rkp-mini-deployer
DATA=/data/adb/rkp_sync
CONFIG=$DATA/config.toml
TMP=$DATA/tmp

. "$MODDIR/lib/run.sh"

MODE=${1:-}
case "$MODE" in
  action) RESULT_BASE= ;;
  run) RESULT_BASE=$TMP/sync-task ;;
  auto) RESULT_BASE=$TMP/sync-auto ;;
  *) exit 2 ;;
esac

umask 077

if [ "$MODE" = action ]; then
  run_receiver_steps action "$SYNC_BIN" "$DEPLOY_BIN" "$CONFIG" ""
  exit "$?"
fi

run_receiver_steps "$MODE" "$SYNC_BIN" "$DEPLOY_BIN" "$CONFIG" "$RESULT_BASE"
overall=$?
publish_run_result "$RESULT_BASE" "$overall" || exit 1
exit "$overall"
