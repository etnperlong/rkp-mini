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

# Auto-issue / auto-push loop. Always started: auto-issue no longer depends on
# remote sync, and the loop exits by itself on a tick where both are off. The
# pid file makes repeat starts harmless.
if command -v setsid >/dev/null 2>&1; then
  setsid sh "$MODDIR/auto-loop.sh" </dev/null >/dev/null 2>&1 &
else
  nohup sh "$MODDIR/auto-loop.sh" </dev/null >/dev/null 2>&1 &
fi
