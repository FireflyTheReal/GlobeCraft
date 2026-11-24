//! JNI functions for the Android app's `app.globecraft.mobile.GlobeNative`, and the
//! frame's way onto the screen: copied into the `Surface` with `ANativeWindow_lock`.
//! The window's buffer is sized to the frame, so the system's scaler stretches the
//! half-size motion frames to the view.
//!
//! shortcut: a CPU copy per frame (about 10 MB at 1080 × 2400); hand the GPU's texture
//! to the window instead once Phase 0 shows the copy matters.

use crate::Globe;
use jni_sys::{JNIEnv, jboolean, jclass, jfloat, jint, jlong, jobject, jstring};
use ndk_sys::{
    ANativeWindow, ANativeWindow_Buffer, ANativeWindow_fromSurface, ANativeWindow_lock,
    ANativeWindow_release, ANativeWindow_setBuffersGeometry, ANativeWindow_unlockAndPost,
};
use std::ffi::{CStr, CString};

/// `AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM`: bytes in the frame's order.
const RGBA_8888: i32 = 1;

struct Host {
    globe: Globe,
    window: *mut ANativeWindow,
    /// The buffer size last given to the window.
    geometry: (usize, usize),
}

impl Host {
    fn release_window(&mut self) {
        if !self.window.is_null() {
            // SAFETY: acquired by `ANativeWindow_fromSurface`, released once.
            unsafe { ANativeWindow_release(self.window) };
            self.window = std::ptr::null_mut();
        }
    }

    /// Copies the newest frame into the window; false if the window refused it.
    fn present(&mut self) -> bool {
        let frame = self.globe.frame();
        if self.window.is_null() || frame.pixels.is_empty() {
            return false;
        }
        let (w, h) = (frame.width, frame.height);
        // SAFETY: `window` is a live window from `ANativeWindow_fromSurface`; the buffer
        // it lends is `stride` × `height` pixels of 4 bytes until `unlockAndPost`.
        unsafe {
            if self.geometry != (w, h) {
                ANativeWindow_setBuffersGeometry(self.window, w as i32, h as i32, RGBA_8888);
                self.geometry = (w, h);
            }
            let mut buffer: ANativeWindow_Buffer = std::mem::zeroed();
            if ANativeWindow_lock(self.window, &mut buffer, std::ptr::null_mut()) != 0 {
                return false;
            }
            let rows = h.min(buffer.height.max(0) as usize);
            let row_bytes = w.min(buffer.width.max(0) as usize) * 4;
            let stride = buffer.stride.max(0) as usize * 4;
            let target = buffer.bits.cast::<u8>();
            for y in 0..rows {
                std::ptr::copy_nonoverlapping(
                    frame.pixels.as_ptr().add(y * w * 4),
                    target.add(y * stride),
                    row_bytes,
                );
            }
            ANativeWindow_unlockAndPost(self.window);
        }
        true
    }
}

fn log(message: &str) {
    let (Ok(tag), Ok(text)) = (CString::new("GlobeCraft"), CString::new(message)) else {
        return;
    };
    // SAFETY: two NUL-terminated strings; 4 is ANDROID_LOG_INFO.
    unsafe { ndk_sys::__android_log_write(4, tag.as_ptr(), text.as_ptr()) };
}

/// # Safety
/// `handle` is 0 or a value `create` returned and `destroy` has not freed.
unsafe fn host<'a>(handle: jlong) -> Option<&'a mut Host> {
    // SAFETY: the caller's contract.
    unsafe { (handle as *mut Host).as_mut() }
}

/// # Safety
/// `env` is the calling thread's JNI environment and `s` a live Java string.
unsafe fn string(env: *mut JNIEnv, s: jstring) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; the characters are released before returning.
    unsafe {
        let functions = &**env;
        let chars = (functions.GetStringUTFChars?)(env, s, std::ptr::null_mut());
        if chars.is_null() {
            return None;
        }
        let text = CStr::from_ptr(chars).to_str().ok().map(str::to_owned);
        (functions.ReleaseStringUTFChars?)(env, s, chars);
        text
    }
}

/// Sends Rust panics to logcat as well: the default hook writes to stderr, which Android
/// drops, so a panicking background task would otherwise fail without a trace.
fn log_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log(&format!("panic: {info}"));
            default(info);
        }));
    });
}

/// Hands the app's Context to rustls-platform-verifier, which reqwest uses to check server
/// certificates with Android's own trust store. Without it every TLS handshake panics.
///
/// # Safety
/// `env` is the calling thread's JNI environment and `context` a live
/// android.content.Context local reference of this native call.
unsafe fn trust_android_certificates(env: *mut JNIEnv, context: jobject) -> Result<(), String> {
    // SAFETY: the caller's contract; the reference stays owned by the JVM's call frame.
    let mut unowned = unsafe { jni::EnvUnowned::from_raw(env.cast()) };
    let outcome = unowned
        .with_env(|env| {
            // SAFETY: the caller's contract; no other wrapper owns this reference.
            let context = unsafe { jni::objects::JObject::from_raw(env, context.cast()) };
            rustls_platform_verifier::android::init_with_env(env, context)
        })
        .into_outcome();
    match outcome {
        jni::Outcome::Ok(()) => Ok(()),
        jni::Outcome::Err(error) => Err(error.to_string()),
        jni::Outcome::Panic(_) => Err("panicked".to_owned()),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_create(
    env: *mut JNIEnv,
    _class: jclass,
    context: jobject,
    app_dir: jstring,
) -> jlong {
    log_panics();
    // SAFETY: called by the JVM with its environment, a Context and a String argument.
    if let Err(error) = unsafe { trust_android_certificates(env, context) } {
        log(&format!("certificate checks could not start: {error}"));
        return 0;
    }
    // SAFETY: as above.
    let Some(dir) = (unsafe { string(env, app_dir) }) else {
        return 0;
    };
    match Globe::new(dir.into()) {
        Ok(globe) => Box::into_raw(Box::new(Host {
            globe,
            window: std::ptr::null_mut(),
            geometry: (0, 0),
        })) as jlong,
        Err(error) => {
            log(&format!("globe failed to start: {error}"));
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_destroy(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: from `Box::into_raw` in `create`; Kotlin drops its handle after this.
        let mut host = unsafe { Box::from_raw(handle as *mut Host) };
        host.release_window();
    }
}

/// Attaches the `Surface` to draw into, or detaches with null (surface destroyed).
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_setSurface(
    env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    surface: jobject,
) {
    // SAFETY: `handle` from `create`; `surface` is null or a live android.view.Surface.
    let Some(host) = (unsafe { host(handle) }) else {
        return;
    };
    host.release_window();
    host.geometry = (0, 0);
    if !surface.is_null() {
        // SAFETY: as above; the window is released in `release_window`.
        host.window = unsafe { ANativeWindow_fromSurface(env.cast(), surface.cast()) };
        host.present();
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_setSize(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    width: jint,
    height: jint,
    scale: jfloat,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe
            .set_size(width.max(1) as usize, height.max(1) as usize, scale);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_pan(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    dx: jfloat,
    dy: jfloat,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe.pan(dx, dy);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_pinch(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    scale: jfloat,
    x: jfloat,
    y: jfloat,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe.pinch(scale, x, y);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_rotate(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    degrees: jfloat,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe.rotate(degrees);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_tilt(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    degrees: jfloat,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe.tilt(degrees);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_setRelief(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
    on: jboolean,
) {
    // SAFETY: `handle` from `create`.
    if let Some(host) = unsafe { host(handle) } {
        host.globe.set_relief(on != 0);
    }
}

/// One display refresh: asks for a frame if needed, shows a finished one. Returns
/// whether to keep ticking.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_tick(
    _env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
) -> jboolean {
    // SAFETY: `handle` from `create`.
    let Some(host) = (unsafe { host(handle) }) else {
        return 0;
    };
    if host.globe.tick() {
        host.present();
    }
    host.globe.busy() as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_globecraft_mobile_GlobeNative_stats(
    env: *mut JNIEnv,
    _class: jclass,
    handle: jlong,
) -> jstring {
    // SAFETY: `handle` from `create`; `env` is the calling thread's environment.
    unsafe {
        let Some(host) = host(handle) else {
            return std::ptr::null_mut();
        };
        let Ok(text) = CString::new(host.globe.stats()) else {
            return std::ptr::null_mut();
        };
        match (**env).NewStringUTF {
            Some(new) => new(env, text.as_ptr()),
            None => std::ptr::null_mut(),
        }
    }
}
