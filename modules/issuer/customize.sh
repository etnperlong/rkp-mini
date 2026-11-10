#!/system/bin/sh
SKIPUNZIP=0

# The old same-role module (id rkp_mini) must be gone before this one runs:
# two issuer modules would race on the same data root and TA. Its uninstall
# keeps /data/misc/keystore/rkp-mini, so no data is lost.
OLD_ID=rkp_mini
if [ -d "/data/adb/modules/$OLD_ID" ] || [ -d "/data/adb/modules_update/$OLD_ID" ]; then
  abort "! 检测到旧同角色模块 $OLD_ID：请先在 KernelSU 中卸载它（运行数据会保留），然后重新安装 rkp_mini_issuer"
fi

[ -n "$ARCH" ] || ARCH=$(getprop ro.product.cpu.abi)
case "$ARCH" in
  arm64|arm64-v8a) ;;
  *) abort "! rkp-mini-issuer: arm64 only (current: $ARCH)" ;;
esac

ui_print "- RKP-Mini Issuer (Qualcomm KeyMint CSR_V2)"
ui_print "- Output: /data/misc/keystore/rkp-mini/output"

set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm "$MODPATH/bin/rkp-mini-issuer" 0 0 0755
set_perm "$MODPATH/bin/rkp-mini-sync" 0 0 0755
set_perm "$MODPATH/action.sh" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/sync-loop.sh" 0 0 0755

mkdir -p /data/misc/keystore/rkp-mini/output
mkdir -p /data/misc/keystore/rkp-mini/logs

# Optional: create default config (only if absent)
[ -f /data/misc/keystore/rkp-mini/config.toml ] || \
  "$MODPATH/bin/rkp-mini-issuer" config init --config /data/misc/keystore/rkp-mini/config.toml || \
  ui_print "! rkp-mini-issuer: could not write default config (non-fatal)"

# First status refresh (install succeeds regardless)
if ! "$MODPATH/bin/rkp-mini-issuer" status --update-description --quiet; then
  ui_print "! rkp-mini-issuer: status refresh failed (check binary ABI / prereqs)"
fi

ui_print "- Remote keybox sync (rkp-mini-sync) is OFF by default."
ui_print "  Opt in from the WebUI (远程同步) or:"
ui_print "  rkp-mini-sync --role sender --config /data/misc/keystore/rkp-mini/config.toml init"
