#!/bin/sh
# Builds crates/globecraft-mobile as a static library for Xcode (device or simulator).
# Xcode runs this before compiling; it also works on its own: ./build-rust-ios.sh
set -e
here="$(cd "$(dirname "$0")" && pwd)"
case "${PLATFORM_NAME:-iphoneos}" in
  iphonesimulator) target=aarch64-apple-ios-sim; out=ios-sim ;;
  *) target=aarch64-apple-ios; out=ios ;;
esac
# A clean environment: Xcode's SDKROOT (iPhoneOS) would break the Mac-side build scripts.
# Always release: a debug renderer is far too slow to judge anything.
env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  IPHONEOS_DEPLOYMENT_TARGET=17.0 \
  sh -c "cd '$here/../..' && rustup target add $target >/dev/null && cargo build --release --target $target -p globecraft-mobile"
mkdir -p "$here/rust/$out"
cp "$here/../../target/$target/release/libglobecraft_mobile.a" "$here/rust/$out/"
