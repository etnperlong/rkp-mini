#!/system/bin/sh
# One scheduled round now (auto-issue if due + auto-push), same as the loop.
# A forced issuance is the WebUI's 立即签发 or `rkp-mini-issuer issue --acknowledge`.
MODDIR=${0%/*}
exec sh "$MODDIR/auto-loop.sh" once