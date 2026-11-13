#!/system/bin/sh
# One scheduled round now (verified gate + fetch --auto + apply --auto), same as
# the loop. A first deployment or an identity change outside the rotation policy
# stays pending; review it from the WebUI, or deploy by hand from a root shell
# with `sh run.sh action`.
MODDIR=${0%/*}
exec sh "$MODDIR/sync-loop.sh" once