#!/system/bin/sh
# Manual fetch + deploy. A first deployment or an identity change outside the
# rotation policy is held as pending; review it in a root shell with
# `rkp-sync deploy apply`.
MODDIR=${0%/*}
exec "$MODDIR/bin/rkp-sync" --role receiver run
