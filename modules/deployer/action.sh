#!/system/bin/sh
# Manual fetch + deploy. A first deployment or an identity change outside the
# rotation policy is held as pending; review it from the WebUI or a root shell
# with `rkp-mini-deployer apply`.
MODDIR=${0%/*}
exec sh "$MODDIR/run.sh" action
