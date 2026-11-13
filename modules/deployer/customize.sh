#!/system/bin/sh
SKIPUNZIP=0
DATA=/data/adb/rkp_sync

# The old same-role module (id rkp_sync) must be gone before this one runs.
# NOTE: the old module's uninstall.sh deletes /data/adb/rkp_sync, so back up
# that root first (its sync/deploy state and secrets are not kept).
OLD_ID=rkp_sync
if [ -d "/data/adb/modules/$OLD_ID" ] || [ -d "/data/adb/modules_update/$OLD_ID" ]; then
  abort "! 检测到旧同角色模块 $OLD_ID：请先备份 /data/adb/rkp_sync（旧模块卸载会删除该目录），在 KernelSU 中卸载它，然后重新安装 rkp_mini_deployer"
fi

[ -n "$ARCH" ] || ARCH=$(getprop ro.product.cpu.abi)
case "$ARCH" in
  arm64|arm64-v8a) ;;
  *) abort "! rkp-mini-deployer: arm64 only (current: $ARCH)" ;;
esac

ui_print "- RKP-Mini Deployer"
ui_print "- Remote keybox deployment is OFF by default."
ui_print "- Using a keybox from another device is keybox cloning,"
ui_print "  not legitimate RKP provisioning. See: rkp-mini-sync enable"

set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm "$MODPATH/bin/rkp-mini-sync" 0 0 0755
set_perm "$MODPATH/bin/rkp-mini-deployer" 0 0 0755
set_perm "$MODPATH/action.sh" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/sync-loop.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755
# run.sh is executed by the loop (auto), the WebUI runner (run) and by hand (action).
set_perm "$MODPATH/run.sh" 0 0 0755
# lib/run.sh is only sourced; it stays 0644 from set_perm_recursive.

mkdir -p "$DATA"
chmod 0700 "$DATA"
[ -f "$DATA/config.toml" ] || \
  "$MODPATH/bin/rkp-mini-sync" --config "$DATA/config.toml" --role receiver init >/dev/null 2>&1 || \
  ui_print "! rkp-mini-sync: could not write default config (non-fatal)"

ui_print "- Setup: tap Open on this module (WebUI), or in a root shell:"
ui_print "    rkp-mini-sync --config $DATA/config.toml --role receiver import '<rkpsync://...>'"
ui_print "    rkp-mini-deployer --config $DATA/config.toml --role receiver add-target omk|teesim|tricky_store"
ui_print "    sh $MODPATH/run.sh action"
ui_print "  binaries: $MODPATH/bin/rkp-mini-sync, $MODPATH/bin/rkp-mini-deployer"
