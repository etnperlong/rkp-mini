#!/usr/bin/env bash
# Cross-compile rkp-mini for arm64 Android and package the KernelSU module zip.
set -euo pipefail

cd "$(dirname "$0")"

TARGET=aarch64-linux-android
API=26
VERSION="v$(grep -m1 '^version' crates/rkp-mini/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ZIP="dist/rkp_mini-${VERSION}.zip"

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
cargo build --release --target "$TARGET"

BIN="target/$TARGET/release/rkp-mini"
[ -f "$BIN" ] || { echo "! build produced no binary" >&2; exit 1; }

# --- WebUI --------------------------------------------------------------------
command -v bun >/dev/null 2>&1 || { echo "! bun is required to build the WebUI" >&2; exit 1; }
echo "- Building WebUI"
( cd webui && bun install --frozen-lockfile && bun run build )
[ -f webui/dist/index.html ] || { echo "! webui build produced no index.html" >&2; exit 1; }

# --- Package ------------------------------------------------------------------
echo "- Packaging $ZIP"
rm -rf dist/module "$ZIP"
mkdir -p dist/module/bin dist/module/webroot
cp "$BIN" dist/module/bin/rkp-mini
cp module/module.prop module/customize.sh module/action.sh \
   module/service.sh module/uninstall.sh dist/module/
cp -R webui/dist/. dist/module/webroot/
chmod 755 dist/module/bin/rkp-mini dist/module/*.sh
( cd dist/module && zip -rq "../rkp_mini-${VERSION}.zip" . )

# --- Verify -------------------------------------------------------------------
echo "- Verify:"
file "$BIN" | sed 's/^/    /'
readelf -h "$BIN" | grep -E 'Machine|Class' | sed 's/^/    /'
echo "    zip contents:"
LISTING=$(unzip -l "$ZIP")
sed 's/^/      /' <<<"$LISTING"
for f in module.prop bin/rkp-mini webroot/index.html webroot/runner/issue.sh; do
  grep -q " $f\$" <<<"$LISTING" || { echo "! $f missing from zip" >&2; exit 1; }
done
echo "- Done: $ZIP"
