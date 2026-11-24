#!/bin/sh
# Builds the Rust globe (crates/globecraft-mobile) into app/src/main/jniLibs for Gradle.
# Needs ANDROID_NDK_HOME (NDK r29), `rustup target add aarch64-linux-android` and
# `cargo install cargo-ndk`.
set -e
cd "$(dirname "$0")/../.."
cargo ndk -t arm64-v8a -P 30 -o mobile/android/app/src/main/jniLibs \
    build --release -p globecraft-mobile
