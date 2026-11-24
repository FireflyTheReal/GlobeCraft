// The Rust globe for the iPadOS app (crates/globecraft-mobile/src/ffi.rs).
// Call everything from the main thread. Gestures are in points.
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct Globe Globe;

// Starts the globe with caches and settings below app_dir (UTF-8). NULL on failure.
Globe *gc_globe_new(const char *app_dir);
void gc_globe_free(Globe *globe);

// The view in pixels and the screen's pixels per point.
void gc_set_size(Globe *globe, uint32_t width, uint32_t height, float scale);

// One finger drag by (dx, dy) points.
void gc_pan(Globe *globe, float dx, float dy);
// Pinch by scale (> 1: fingers apart, closer) around (x, y) points.
void gc_pinch(Globe *globe, float scale, float x, float y);
// Two-finger turn, clockwise on screen, in degrees.
void gc_rotate(Globe *globe, float degrees);
// Lean toward the horizon by degrees (negative: back toward straight down).
void gc_tilt(Globe *globe, float degrees);
void gc_set_relief(Globe *globe, bool on);

// Once per display refresh. True when gc_frame holds a new frame.
bool gc_tick(Globe *globe);
// True while gc_tick should keep being called.
bool gc_busy(Globe *globe);
// The newest frame: premultiplied RGBA, rows of width * 4 bytes, valid until the next
// gc_tick or gc_globe_free. NULL (0 x 0) before the first frame.
const uint8_t *gc_frame(Globe *globe, uint32_t *width, uint32_t *height);
// Copies the statistics line (NUL-terminated, cut to fit) into out; returns its length.
size_t gc_stats(Globe *globe, char *out, size_t capacity);
