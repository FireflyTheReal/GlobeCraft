//! C functions for the iPadOS app: Swift imports `include/globecraft_mobile.h`.
//!
//! Every function takes the pointer `gc_globe_new` returned; a null pointer is ignored
//! (or answered with nothing). All calls must come from one thread (the main thread).
//! A panic inside aborts the app, as Rust does for `extern "C"` functions.

use crate::Globe;
use std::ffi::{CStr, c_char};

/// # Safety
/// `globe` is null or a pointer from `gc_globe_new` not yet freed, used on one thread.
unsafe fn globe<'a>(globe: *mut Globe) -> Option<&'a mut Globe> {
    // SAFETY: the caller's contract above.
    unsafe { globe.as_mut() }
}

/// Starts the globe with caches and settings below `app_dir` (a UTF-8 path).
/// Returns null if `app_dir` is not valid UTF-8 or the network runtime cannot start.
///
/// # Safety
/// `app_dir` is a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_globe_new(app_dir: *const c_char) -> *mut Globe {
    if app_dir.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller passes a NUL-terminated string.
    let Ok(dir) = unsafe { CStr::from_ptr(app_dir) }.to_str() else {
        return std::ptr::null_mut();
    };
    match Globe::new(dir.into()) {
        Ok(globe) => Box::into_raw(Box::new(globe)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `globe` is null or from `gc_globe_new`; it must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_globe_free(globe: *mut Globe) {
    if !globe.is_null() {
        // SAFETY: from `Box::into_raw` in `gc_globe_new`, freed once.
        drop(unsafe { Box::from_raw(globe) });
    }
}

/// # Safety
/// See `gc_globe_free`'s contract for `g` (here it stays valid).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_set_size(g: *mut Globe, width: u32, height: u32, scale: f32) {
    if let Some(g) = unsafe { globe(g) } {
        g.set_size(width as usize, height as usize, scale);
    }
}

/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_pan(g: *mut Globe, dx: f32, dy: f32) {
    if let Some(g) = unsafe { globe(g) } {
        g.pan(dx, dy);
    }
}

/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_pinch(g: *mut Globe, scale: f32, x: f32, y: f32) {
    if let Some(g) = unsafe { globe(g) } {
        g.pinch(scale, x, y);
    }
}

/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_rotate(g: *mut Globe, degrees: f32) {
    if let Some(g) = unsafe { globe(g) } {
        g.rotate(degrees);
    }
}

/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_tilt(g: *mut Globe, degrees: f32) {
    if let Some(g) = unsafe { globe(g) } {
        g.tilt(degrees);
    }
}

/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_set_relief(g: *mut Globe, on: bool) {
    if let Some(g) = unsafe { globe(g) } {
        g.set_relief(on);
    }
}

/// True when `gc_frame` holds a new frame to show.
///
/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_tick(g: *mut Globe) -> bool {
    unsafe { globe(g) }.is_some_and(Globe::tick)
}

/// True while the host should keep calling `gc_tick` every display refresh.
///
/// # Safety
/// `g` is null or a live globe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_busy(g: *mut Globe) -> bool {
    unsafe { globe(g) }.is_some_and(|g| g.busy())
}

/// The newest frame: premultiplied RGBA, rows of `width * 4` bytes. The pointer stays
/// valid until the next `gc_tick` or `gc_globe_free`. Null (and 0 × 0) before the first.
///
/// # Safety
/// `g` is null or a live globe; `width` and `height` are valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_frame(g: *mut Globe, width: *mut u32, height: *mut u32) -> *const u8 {
    let frame = unsafe { globe(g) }.map(|g| g.frame());
    let (w, h, pixels) = match frame {
        Some(f) if !f.pixels.is_empty() => (f.width as u32, f.height as u32, f.pixels.as_ptr()),
        _ => (0, 0, std::ptr::null()),
    };
    // SAFETY: the caller passes writable pointers.
    unsafe {
        *width = w;
        *height = h;
    }
    pixels
}

/// Copies the statistics line (UTF-8, NUL-terminated, cut to fit) into `out`; returns
/// its full length in bytes.
///
/// # Safety
/// `g` is null or a live globe; `out` is valid for `capacity` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gc_stats(g: *mut Globe, out: *mut c_char, capacity: usize) -> usize {
    let Some(g) = (unsafe { globe(g) }) else {
        return 0;
    };
    let text = g.stats();
    if !out.is_null() && capacity > 0 {
        // Cut on a character boundary, leaving room for the NUL.
        let mut n = text.len().min(capacity - 1);
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        // SAFETY: `out` holds `capacity` bytes and `n < capacity`.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), out.cast::<u8>(), n);
            *out.add(n) = 0;
        }
    }
    text.len()
}
