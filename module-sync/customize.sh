#!/system/bin/sh
SKIPUNZIP=0
DATA=/data/adb/rkp_sync

[ -n "$ARCH" ] || ARCH=$(getprop ro.product.cpu.abi)
case "$ARCH" in
  arm64|arm64-v8a) ;;
  *) abort "! rkp-sync: arm64 only (current: $ARCH)" ;;
esac

ui_print "- RKP Sync receiver"
ui_print "- Remote keybox deployment is OFF by default."
ui_print "- Using a keybox from another device is keybox cloning,"
ui_print "  not legitimate RKP provisioning. See: rkp-sync enable"

set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm "$MODPATH/bin/rkp-sync" 0 0 0755
set_perm "$MODPATH/action.sh" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/sync-loop.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755

mkdir -p "$DATA"
chmod 0700 "$DATA"
[ -f "$DATA/config.toml" ] || \
  "$MODPATH/bin/rkp-sync" --role receiver init >/dev/null 2>&1 || \
  ui_print "! rkp-sync: could not write default config (non-fatal)"

ui_print "- Setup (root shell):"
ui_print "    rkp-sync import '<rkpsync://...>'"
ui_print "    rkp-sync deploy add-target omk|teesim|tricky_store"
ui_print "    rkp-sync fetch && rkp-sync deploy apply"
ui_print "  binary: $MODPATH/bin/rkp-sync"
