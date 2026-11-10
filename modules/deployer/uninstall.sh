#!/system/bin/sh
# KEEP /data/adb/rkp_sync on uninstall: it holds the age passphrase, provider
# secret, sync/deploy state, verified staging and the deployed keyboxes.
# Cleanup is explicit, not a side effect of removing the module:
#   rkp-mini-sync forget            # disable + drop this workspace's state/secret
#   rkp-mini-deployer remove --target omk|teesim|tricky_store --yes
# Full cleanup by hand (keybox copies in OMK/TEESimulator/TrickyStore are not
# touched by this script): rm -rf /data/adb/rkp_sync
exit 0
