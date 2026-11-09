#!/system/bin/sh
MODDIR=${0%/*}
"$MODDIR/bin/rkp-mini" status --update-description --quiet >/dev/null 2>&1 &
