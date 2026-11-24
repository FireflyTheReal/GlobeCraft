# GlobeCraft for Android and iPad (Phase 0)

Native apps around the Rust globe. The interface is native: Kotlin on Android, Swift and
SwiftUI on iPadOS. The globe itself is the desktop app's renderer, reached through
`crates/globecraft-mobile`:
- [`src/lib.rs`](../crates/globecraft-mobile/src/lib.rs): the `Globe` (camera, tiles,
  renderer).
- [`src/android.rs`](../crates/globecraft-mobile/src/android.rs): JNI functions; draws
  into a `Surface`.
- [`src/ffi.rs`](../crates/globecraft-mobile/src/ffi.rs) and
  [`include/globecraft_mobile.h`](../crates/globecraft-mobile/include/globecraft_mobile.h):
  the C functions for Swift.

Phase 0 is a measuring build. It shows the globe full screen with a statistics line
(renderer, frame size and time, downloads, altitude) and a Relief switch, and takes
these gestures:
- One finger turns the globe.
- Two fingers pinch to zoom and twist to rotate.
- Two fingers dragged up or down together tilt the view.
- Double-tap zooms in; a two-finger tap zooms out.

It does not have borders, names, live layers, search or settings yet.

## Android

Install the APK the design session left in `~/Downloads/globe/mobile/`:
1. Copy it to the phone.
2. Allow installs from your file manager (Settings › Apps › Special access › Install
   unknown apps).
3. Open the APK.

It needs Android 11 or later on a 64-bit ARM phone. Logs: `adb logcat -s GlobeCraft`.

Build it yourself:
1. Install the Android SDK and NDK r29 (`ANDROID_HOME`, `ANDROID_NDK_HOME`).
2. `rustup target add aarch64-linux-android` and `cargo install cargo-ndk`.
3. Run:

```sh
mobile/android/build-rust.sh          # libglobecraft_mobile.so into app/src/main/jniLibs
cd mobile/android && gradle assembleRelease   # Gradle 9.9 (AGP 9.5)
# → app/build/outputs/apk/release/app-release.apk
```

The release APK is signed with the debug key, which is enough for sideloading. A Play
Store release needs its own key (Phase 4).

## iPad

1. `brew install xcodegen`, then run `xcodegen` in `mobile/ios`. It writes
   `GlobeCraft.xcodeproj`.
2. Open the project. Under Signing & Capabilities pick your team; a free Apple ID works.
3. Connect the iPad. On the iPad, turn on Settings › Privacy & Security › Developer Mode.
4. Press Run.
   - The build phase runs `build-rust-ios.sh`, which builds the Rust library for the iPad
     (or the simulator) with your `cargo`. It needs `rustup` on the Mac.
   - The first build takes a few minutes.
5. With a free Apple ID, the iPad trusts the app for 7 days; then run it from Xcode again.

## What to measure in Phase 0

Watch the statistics line, and check these:
- Which renderer runs: the GPU adapter's name, or "CPU" with the reason.
- Frame time while turning the globe, and at rest.
- With Relief on and off, close to the ground.
- Whether the phone gets warm after five minutes of turning.
- Memory: Android Studio's profiler or `adb shell dumpsys meminfo app.globecraft.mobile`.
