#!/system/bin/sh
MODDIR=${0%/*}
# Unattended fetch+deploy only when enabled with accepted risk, a secret-file
# passphrase and an imported stream (`rkp-sync check-auto`).
if "$MODDIR/bin/rkp-sync" --role receiver check-auto >/dev/null 2>&1; then
  if command -v setsid >/dev/null 2>&1; then
    setsid sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  else
    nohup sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  fi
fi
