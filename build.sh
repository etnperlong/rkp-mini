#!/usr/bin/env bash
# Cross-compile the three RKP-Mini commands for arm64 Android and package the
# two KernelSU modules (issuer + deployer).
#
#   issuer module   : rkp-mini-issuer + shared rkp-mini-sync   (+ issuer WebUI)
#   deployer module : shared rkp-mini-sync + rkp-mini-deployer (+ deployer WebUI)
set -euo pipefail

cd "$(dirname "$0")"

TARGET=aarch64-linux-android
API=26
VERSION="v$(grep -m1 '^version' crates/rkp-mini-issuer/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
DEPLOY_VERSION="v$(grep -m1 '^version' crates/rkp-mini-deployer/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ISSUER_ZIP="dist/rkp_mini_issuer-${VERSION}.zip"
DEPLOYER_ZIP="dist/rkp_mini_deployer-${DEPLOY_VERSION}.zip"

# --- Locate the NDK -----------------------------------------------------------
NDK="${NDK:-}"
if [ -z "$NDK" ]; then
  NDK=$(ls -d "$HOME"/Android/Sdk/ndk/* 2>/dev/null | sort -V | tail -1 || true)
fi
if [ -z "$NDK" ] || [ ! -d "$NDK" ]; then
  echo "! Android NDK not found; set NDK=/path/to/ndk" >&2
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
cargo build --release --locked --target "$TARGET" \
  -p rkp-mini-issuer -p rkp-mini-sync -p rkp-mini-deployer

ISSUER_BIN="target/$TARGET/release/rkp-mini-issuer"
SYNC_BIN="target/$TARGET/release/rkp-mini-sync"
DEPLOY_BIN="target/$TARGET/release/rkp-mini-deployer"
for b in "$ISSUER_BIN" "$SYNC_BIN" "$DEPLOY_BIN"; do
  [ -f "$b" ] || { echo "! build produced no $b" >&2; exit 1; }
done

# --- WebUI --------------------------------------------------------------------
command -v bun >/dev/null 2>&1 || { echo "! bun is required to build the WebUI" >&2; exit 1; }
echo "- Building WebUI"
( cd webui && bun install --frozen-lockfile && bun run build )
for page in dist dist-receiver; do
  [ -f "webui/$page/index.html" ] || {
    echo "! webui build produced no $page/index.html" >&2; exit 1; }
done

# --- Package: issuer ----------------------------------------------------------
# The issuer module carries the issuer and the shared sync command; never the
# deployer.
echo "- Packaging $ISSUER_ZIP"
rm -rf dist/module-issuer "$ISSUER_ZIP"
mkdir -p dist/module-issuer/bin dist/module-issuer/webroot
cp "$ISSUER_BIN" dist/module-issuer/bin/rkp-mini-issuer
cp "$SYNC_BIN" dist/module-issuer/bin/rkp-mini-sync
cp modules/issuer/module.prop modules/issuer/customize.sh modules/issuer/action.sh \
   modules/issuer/service.sh modules/issuer/uninstall.sh modules/issuer/sync-loop.sh \
   dist/module-issuer/
cp -R modules/issuer/webroot/runner dist/module-issuer/webroot/runner
cp -R webui/dist/. dist/module-issuer/webroot/
chmod 755 dist/module-issuer/bin/rkp-mini-issuer dist/module-issuer/bin/rkp-mini-sync \
  dist/module-issuer/*.sh dist/module-issuer/webroot/runner/*.sh
( cd dist/module-issuer && zip -rq "../$(basename "$ISSUER_ZIP")" . )

# --- Package: deployer --------------------------------------------------------
# The deployer module carries the shared sync command and the local deployer,
# the combo runner and the deployer page; never the issuer.
echo "- Packaging $DEPLOYER_ZIP"
rm -rf dist/module-deployer "$DEPLOYER_ZIP"
mkdir -p dist/module-deployer/bin dist/module-deployer/lib dist/module-deployer/webroot
cp "$SYNC_BIN" dist/module-deployer/bin/rkp-mini-sync
cp "$DEPLOY_BIN" dist/module-deployer/bin/rkp-mini-deployer
cp modules/deployer/module.prop modules/deployer/customize.sh modules/deployer/action.sh \
   modules/deployer/service.sh modules/deployer/uninstall.sh modules/deployer/sync-loop.sh \
   modules/deployer/run.sh dist/module-deployer/
cp modules/deployer/lib/run.sh dist/module-deployer/lib/run.sh
cp -R modules/deployer/webroot/runner dist/module-deployer/webroot/runner
cp -R webui/dist-receiver/. dist/module-deployer/webroot/
chmod 755 dist/module-deployer/bin/rkp-mini-sync dist/module-deployer/bin/rkp-mini-deployer \
  dist/module-deployer/*.sh dist/module-deployer/webroot/runner/*.sh
chmod 644 dist/module-deployer/lib/run.sh   # sourced, never executed
( cd dist/module-deployer && zip -rq "../$(basename "$DEPLOYER_ZIP")" . )

# --- Verify -------------------------------------------------------------------
echo "- Verify:"
for b in "$ISSUER_BIN" "$SYNC_BIN" "$DEPLOY_BIN"; do
  file "$b" | grep -q 'ARM aarch64' || { echo "! $b is not an AArch64 ELF" >&2; exit 1; }
  readelf -h "$b" | grep -q 'AArch64' || { echo "! $b has no AArch64 header" >&2; exit 1; }
done

# Every required entry must be at the zip root (no wrapper directory).
check_zip() {
  local zip="$1"; shift
  local list
  list=$(unzip -Z1 "$zip")
  for entry in "$@"; do
    if ! printf '%s\n' "$list" | grep -qx -- "$entry"; then
      echo "! $zip is missing $entry" >&2
      exit 1
    fi
  done
  if printf '%s\n' "$list" | grep -q '^[^/]*/module\.prop$'; then
    echo "! $zip has a wrapper directory" >&2
    exit 1
  fi
}

check_absent() {
  local zip="$1"; shift
  local list
  list=$(unzip -Z1 "$zip")
  for entry in "$@"; do
    if printf '%s\n' "$list" | grep -qE "^${entry}$"; then
      echo "! $zip must not contain $entry" >&2
      exit 1
    fi
  done
}

check_zip "$ISSUER_ZIP" module.prop customize.sh service.sh sync-loop.sh action.sh \
  uninstall.sh bin/rkp-mini-issuer bin/rkp-mini-sync webroot/index.html \
  webroot/runner/issue.sh webroot/runner/sync.sh
check_absent "$ISSUER_ZIP" 'bin/rkp-mini-deployer' 'run\.sh' 'lib/run\.sh' 'bin/rkp-mini' 'bin/rkp-sync'
check_zip "$DEPLOYER_ZIP" module.prop customize.sh service.sh sync-loop.sh action.sh \
  uninstall.sh run.sh lib/run.sh bin/rkp-mini-sync bin/rkp-mini-deployer \
  webroot/index.html webroot/runner/sync.sh
check_absent "$DEPLOYER_ZIP" 'bin/rkp-mini-issuer' 'webroot/runner/issue\.sh' 'bin/rkp-mini' 'bin/rkp-sync'

# Both modules must ship byte-identical sync binaries.
a=$(unzip -p "$ISSUER_ZIP" bin/rkp-mini-sync | sha256sum | cut -d' ' -f1)
b=$(unzip -p "$DEPLOYER_ZIP" bin/rkp-mini-sync | sha256sum | cut -d' ' -f1)
[ "$a" = "$b" ] || { echo "! the two zips ship different rkp-mini-sync binaries" >&2; exit 1; }

# module.prop id/version must match the owning crate; never patch it here.
prop() { unzip -p "$1" module.prop | sed -n "s/^$2=//p" | head -1; }
[ "$(prop "$ISSUER_ZIP" id)" = rkp_mini_issuer ] || {
  echo "! issuer module.prop id must be rkp_mini_issuer" >&2; exit 1; }
[ "$(prop "$ISSUER_ZIP" version)" = "$VERSION" ] || {
  echo "! issuer module.prop version must be $VERSION (crates/rkp-mini-issuer)" >&2; exit 1; }
[ "$(prop "$DEPLOYER_ZIP" id)" = rkp_mini_deployer ] || {
  echo "! deployer module.prop id must be rkp_mini_deployer" >&2; exit 1; }
[ "$(prop "$DEPLOYER_ZIP" version)" = "$DEPLOY_VERSION" ] || {
  echo "! deployer module.prop version must be $DEPLOY_VERSION (crates/rkp-mini-deployer)" >&2; exit 1; }

echo "- Done: $ISSUER_ZIP $DEPLOYER_ZIP"
