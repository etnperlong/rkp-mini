#!/system/bin/sh
MODDIR=${0%/*}
exec "$MODDIR/bin/rkp-mini" issue --acknowledge
