#!/usr/bin/env bash
# Cross-compile rkp-mini for arm64 Android and package the KernelSU module zip.
set -euo pipefail

cd "$(dirname "$0")"

TARGET=aarch64-linux-android
API=26
VERSION="v$(grep -m1 '^version' crates/rkp-mini/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
SYNC_VERSION="v$(grep -m1 '^version' crates/rkp-sync/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ZIP="dist/rkp_mini-${VERSION}.zip"
SYNC_ZIP="dist/rkp_sync-${SYNC_VERSION}.zip"

# --- Locate the NDK -----------------------------------------------------------
NDK="${NDK:-}"
if [ -z "$NDK" ]; then
  for d in "$HOME/Android/Sdk/ndk"/*; do
    [ -d "$d/toolchains/llvm/prebuilt" ] && NDK="$d"
  done
fi
if [ -z "$NDK" ] || [ ! -d "$NDK" ]; then
  echo "! NDK not found. Set \$NDK to your Android NDK path." >&2
  exit 1
fi
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
echo "- NDK: $NDK"

export CC_aarch64_linux_android="$TOOLCHAIN/bin/aarch64-linux-android${API}-clang"
export CXX_aarch64_linux_android="$TOOLCHAIN/bin/aarch64-linux-android${API}-clang++"
export AR_aarch64_linux_android="$TOOLCHAIN/bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC_aarch64_linux_android"
# ring needs these generic vars too
export CC="$CC_aarch64_linux_android"
export AR="$AR_aarch64_linux_android"
export TARGET_CC="$CC_aarch64_linux_android"
export TARGET_AR="$AR_aarch64_linux_android"

# --- Build --------------------------------------------------------------------
rustup target add "$TARGET" >/dev/null 2>&1 || true

# --- Neutral build paths ------------------------------------------------------
# Ship no personal build paths: remap the checkout, cargo registry/git caches,
# the rustup toolchain and the rest of $HOME for rustc and for C/C++ sources.
# RUSTFLAGS is kept; CARGO_ENCODED_RUSTFLAGS, when set, is what cargo reads.
# The last matching prefix wins, so the widest comes first and the checkout last;
# shortcut: paths containing spaces split the flag lists; add quoting support before using such paths.
CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
RUST_REMAP=
C_REMAP=
for p in "$HOME=/home" "$CARGO_HOME=/cargo" "$RUSTUP_HOME=/rustup" "$(pwd -P)=/build"; do
  RUST_REMAP="$RUST_REMAP --remap-path-prefix=$p"
  C_REMAP="$C_REMAP -ffile-prefix-map=$p"
done
export CFLAGS="${CFLAGS:+$CFLAGS }$C_REMAP"
export CXXFLAGS="${CXXFLAGS:+$CXXFLAGS }$C_REMAP"
if [ -n "${CARGO_ENCODED_RUSTFLAGS+set}" ]; then
  sep=$(printf '\x1f')
  remap=${RUST_REMAP# }
  CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS}${CARGO_ENCODED_RUSTFLAGS:+$sep}${remap// /$sep}"
  export CARGO_ENCODED_RUSTFLAGS
else
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }${RUST_REMAP# }"
fi

echo "- Building release for $TARGET"
cargo build --release --target "$TARGET" -p rkp-mini -p rkp-sync

BIN="target/$TARGET/release/rkp-mini"
SYNC_BIN="target/$TARGET/release/rkp-sync"
[ -f "$BIN" ] || { echo "! build produced no rkp-mini binary" >&2; exit 1; }
[ -f "$SYNC_BIN" ] || { echo "! build produced no rkp-sync binary" >&2; exit 1; }

# --- WebUI --------------------------------------------------------------------
command -v bun >/dev/null 2>&1 || { echo "! bun is required to build the WebUI" >&2; exit 1; }
echo "- Building WebUI"
( cd webui && bun install --frozen-lockfile && bun run build )
[ -f webui/dist/index.html ] || { echo "! webui build produced no index.html" >&2; exit 1; }
[ -f webui/dist-receiver/index.html ] || { echo "! receiver webui build produced no index.html" >&2; exit 1; }

# --- Package: sender (rkp_mini) -----------------------------------------------
echo "- Packaging $ZIP"
rm -rf dist/module "$ZIP"
mkdir -p dist/module/bin dist/module/webroot
cp "$BIN" dist/module/bin/rkp-mini
cp "$SYNC_BIN" dist/module/bin/rkp-sync
cp module/module.prop module/customize.sh module/action.sh \
   module/service.sh module/uninstall.sh module/sync-loop.sh dist/module/
cp -R webui/dist/. dist/module/webroot/
chmod 755 dist/module/bin/rkp-mini dist/module/bin/rkp-sync dist/module/*.sh
( cd dist/module && zip -rq "../rkp_mini-${VERSION}.zip" . )

# --- Package: receiver (rkp_sync) ---------------------------------------------
# Receiver module carries only rkp-sync and the receiver page; never the issuer.
echo "- Packaging $SYNC_ZIP"
rm -rf dist/module-sync "$SYNC_ZIP"
mkdir -p dist/module-sync/bin dist/module-sync/webroot
cp "$SYNC_BIN" dist/module-sync/bin/rkp-sync
cp module-sync/module.prop module-sync/customize.sh module-sync/action.sh \
   module-sync/service.sh module-sync/uninstall.sh module-sync/sync-loop.sh dist/module-sync/
cp -R webui/dist-receiver/. dist/module-sync/webroot/
chmod 755 dist/module-sync/bin/rkp-sync dist/module-sync/*.sh
( cd dist/module-sync && zip -rq "../rkp_sync-${SYNC_VERSION}.zip" . )

# --- Verify -------------------------------------------------------------------
echo "- Verify:"
for b in "$BIN" "$SYNC_BIN"; do
  file "$b" | sed 's/^/    /'
  readelf -h "$b" | grep -E 'Machine|Class' | sed 's/^/    /'
done
check_zip() {
  local zip=$1; shift
  echo "    $zip contents:"
  local listing
  listing=$(unzip -l "$zip")
  sed 's/^/      /' <<<"$listing"
  for f in "$@"; do
    grep -q " $f\$" <<<"$listing" || { echo "! $f missing from $zip" >&2; exit 1; }
  done
}
check_zip "$ZIP" module.prop customize.sh service.sh sync-loop.sh \
  bin/rkp-mini bin/rkp-sync webroot/index.html webroot/runner/issue.sh webroot/runner/sync.sh
check_zip "$SYNC_ZIP" module.prop customize.sh service.sh sync-loop.sh \
  action.sh uninstall.sh bin/rkp-sync webroot/index.html webroot/runner/sync.sh
if unzip -l "$SYNC_ZIP" | grep -qE ' (bin/rkp-mini|webroot/runner/issue\.sh)$'; then
  echo "! receiver zip must not contain the issuer (rkp-mini / issue.sh)" >&2; exit 1
fi
echo "- Done: $ZIP $SYNC_ZIP"
