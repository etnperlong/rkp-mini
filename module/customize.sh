#!/system/bin/sh
SKIPUNZIP=0

[ -n "$ARCH" ] || ARCH=$(getprop ro.product.cpu.abi)
case "$ARCH" in
  arm64|arm64-v8a) ;;
  *) abort "! rkp-mini: arm64 only (current: $ARCH)" ;;
esac

ui_print "- RKP Mini (Qualcomm KeyMint CSR_V2)"
ui_print "- Output: /data/misc/keystore/rkp-mini/output"

set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm "$MODPATH/bin/rkp-mini" 0 0 0755
set_perm "$MODPATH/bin/rkp-sync" 0 0 0755
set_perm "$MODPATH/action.sh" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/sync-loop.sh" 0 0 0755

mkdir -p /data/misc/keystore/rkp-mini/output
mkdir -p /data/misc/keystore/rkp-mini/logs

# Optional: create default config (only if absent)
[ -f /data/misc/keystore/rkp-mini/config.toml ] || \
  "$MODPATH/bin/rkp-mini" config init --config /data/misc/keystore/rkp-mini/config.toml || \
  ui_print "! rkp-mini: could not write default config (non-fatal)"

# First status refresh (install succeeds regardless)
if ! "$MODPATH/bin/rkp-mini" status --update-description --quiet; then
  ui_print "! rkp-mini: status refresh failed (check binary ABI / prereqs)"
fi

ui_print "- Remote keybox sync (rkp-sync) is OFF by default."
ui_print "  Opt in from the WebUI (远程同步) or: rkp-sync --role sender init"
