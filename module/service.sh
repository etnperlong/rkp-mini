#!/system/bin/sh
MODDIR=${0%/*}
"$MODDIR/bin/rkp-mini" status --update-description --quiet >/dev/null 2>&1 &

# Remote keybox sync loop: only when enabled with accepted risk and a
# secret-file passphrase (`rkp-sync check-auto`). Disabled by default.
if "$MODDIR/bin/rkp-sync" --role sender check-auto >/dev/null 2>&1; then
  if command -v setsid >/dev/null 2>&1; then
    setsid sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  else
    nohup sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  fi
fi
