#!/system/bin/sh
MODDIR=${0%/*}

# A lingering old same-role module (id rkp_mini) must not run beside this one.
# Its uninstall keeps the data root, so the fix is uninstall + reboot.
if [ -d /data/adb/modules/rkp_mini ] || [ -d /data/adb/modules_update/rkp_mini ]; then
  echo "rkp_mini_issuer: old same-role module rkp_mini is installed; not starting." >&2
  echo "  Uninstall rkp_mini in KernelSU (data is kept) and reboot." >&2
  exit 0
fi

"$MODDIR/bin/rkp-mini-issuer" status --update-description --quiet >/dev/null 2>&1 &

# Remote keybox sync loop: only when enabled with accepted risk and a
# secret-file passphrase (`rkp-mini-sync check-auto`). Disabled by default.
if "$MODDIR/bin/rkp-mini-sync" --role sender \
    --config /data/misc/keystore/rkp-mini/config.toml check-auto >/dev/null 2>&1; then
  if command -v setsid >/dev/null 2>&1; then
    setsid sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  else
    nohup sh "$MODDIR/sync-loop.sh" </dev/null >/dev/null 2>&1 &
  fi
fi
