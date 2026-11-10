#!/system/bin/sh
MODDIR=${0%/*}

# A lingering old same-role module (id rkp_sync) must not run beside this one.
# Its uninstall deletes /data/adb/rkp_sync, so it must be uninstalled
# deliberately after a backup, never left running in parallel.
if [ -d /data/adb/modules/rkp_sync ] || [ -d /data/adb/modules_update/rkp_sync ]; then
  echo "rkp_mini_deployer: old same-role module rkp_sync is installed; not starting." >&2
  echo "  Back up /data/adb/rkp_sync, uninstall rkp_sync in KernelSU, then reboot." >&2
  exit 0
fi

# Unattended fetch+deploy only when enabled with accepted risk, a secret-file
# passphrase and an imported stream (`rkp-mini-sync check-auto`).
if "$MODDIR/bin/rkp-mini-sync" --role receiver \
    --config /data/adb/rkp_sync/config.toml check-auto >/dev/null 2>&1; then
  if command -v setsid >/dev/null 2>&1; then
    setsid sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  else
    nohup sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  fi
fi
