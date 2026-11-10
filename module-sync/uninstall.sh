#!/system/bin/sh
# Remove receiver secrets (age passphrase, WebDAV password), staged plaintext
# keybox and sync/deploy state. Keyboxes already deployed to OMK/TEESimulator/
# TrickyStore are NOT touched (removing them could break those tools).
rm -rf /data/adb/rkp_sync
exit 0
