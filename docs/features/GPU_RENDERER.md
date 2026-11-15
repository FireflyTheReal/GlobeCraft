# GPU globe renderer (beta)

Builds with the `gpu` feature (on by default when building from source) include a
wgpu 30 compute renderer alongside the CPU renderer and use it for overview frames
on every platform, unless it is turned off in Settings → Graphics. Published release
downloads still exclude it (`--features standard`): add `gpu` to `standard` once
native Windows and Linux runs are reported.
The GPU renderer ray-casts the overview globe, calculates Mercator coordinates and LOD, samples
imagery, blends zoom levels, and applies sunlight and atmosphere on the GPU.
The render crate owns it; the egui presentation backend stays in the app.
The desktop uses eframe's wgpu presentation independently of globe
computation, including in CPU-only globe builds. On Linux and the BSDs it uses
Vulkan only (`WGPU_BACKEND` overrides): wgpu's OpenGL backend opens EGL as soon as
the instance is created, and a driver that returns no display without an error
panics inside `khronos-egl` (issue #1). When wgpu cannot start, the app starts
again with eframe's glow (OpenGL) renderer. Elsewhere eframe's default backends
remain. The compute renderer uses Vulkan, Metal or DX12 only. This UI backend does not accelerate CPU globe calculations.
Adapter/device creation, texture uploads, execution and readback run on the frame
coordinator, never on the UI thread.

## Selection and fallback

The GPU renderer is on by default (`auto`) wherever it is built in, since 2026-11-15.
Before that it was on only on Apple Silicon, and in practice never ran: it refused
the WGS 84 ellipsoid (the default Earth model) and every view of a polar cap under
satellite imagery. Settings → Graphics → "Draw the globe on the GPU" turns it off
(read at startup) and shows the adapter in use, or why the CPU draws instead.
`GLOBECRAFT_RENDERER` overrides the setting.

`GLOBECRAFT_RENDERER=auto` uses the GPU for supported views with at least 262,144
pixels; small frames keep CPU rendering because complete GPU/CPU timings were similar
at 512×384 on the baseline adapter. `GLOBECRAFT_RENDERER=gpu` also uses it for small
supported frames. `GLOBECRAFT_RENDERER=cpu` never creates a GPU device.

GPU rendering requires an altitude of at least 1,000 km, no geographic grid, no
relief, the screen's level blending (not the sharper screenshot modes), at most
4 million pixels, and a checked LOD bound of zoom 10 or lower. Close views and exact
grid rendering use the double-precision CPU renderer. Within that range it draws both
Earth models (a sphere is the ellipsoid whose axes are all one), every provider and
historical release, sunlight, the air in front of the ground (`atmosphere`: two small
read-only tables, the transmittance table's ground row and the scattering table's top
layer, and the same sums as the CPU) and night lights (their tiles share the atlas and
the tile table, with their own block of tile needs). Camera operations,
picking and overlay projection remain double precision. The sky, a narrow horizon
strip, tile-seam strips and the polar caps (latitudes past 83° under satellite
imagery, which have their own projection, tiles and colour matching) are left by the
shader and drawn by the shared precise CPU calculations ("repair"), retaining
deterministic stars and avoiding floating-point changes in tile-edge selection. A
view that is mostly polar cap is therefore mostly CPU work.

Four guards make an untested adapter safe to try:

- **Self-check.** Before its first frame each device draws one small view of a
  made-up tile, and so does the CPU; the adapter is refused when more than 1 % of
  the colour channels differ by more than one level, or any by more than 16 (the
  bounds the frame benchmark holds real imagery to). A driver that computes wrong
  pixels does not draw the globe.
- **Start marker.** The file `gpu-start` in the settings folder is written before
  the device is created and removed after its first frame, or after a start that
  failed cleanly. A crash or forced quit in between leaves it, and the next start
  keeps to the CPU instead of crashing again. Turning the setting off and on, or a
  new GlobeCraft version, allows another try. Not used when `GLOBECRAFT_RENDERER`
  is set.
- **Bounded wait.** A frame the device does not finish within 5 s counts as a device
  failure. This does not cover a driver call that never returns.
- **Failure and replacement.** Device loss, uncaptured GPU errors, a failed
  readback or a frame that never finished drop that device and draw the frame on the
  CPU. Two seconds later a new device is created (a driver restart or a laptop
  waking up), at most three times per session; then the CPU draws until GlobeCraft
  restarts. Rust panics from GPU initialization or rendering are caught at the
  backend boundary so they cannot stop the frame coordinator. A native driver crash
  is not recoverable in-process; the start marker covers the one at start.

Initialization failure selects CPU rendering with an English diagnostic and is not
retried. An oversized tile working set or unusual tile dimensions uses CPU rendering
for that view, preserving the general sampler. Help → diagnostics reports the
renderer, the adapter or the reason, and how many frames each renderer drew: a GPU
that is "on" but draws no frames shows there.

For a desktop build without GPU globe rendering:

```sh
cargo build -p globecraft --no-default-features --features standard --locked
```

CPU globe rendering still uses wgpu presentation and requires a working graphics
driver for the native UI.

## Resource ownership and presentation

The GPU tile atlas has a separate **128 MiB maximum** (512 RGBA 256×256 layers),
reduced to the adapter's supported array-layer limit. The atlas follows keys actually
sampled by the current view, with optional global zooms 0–3 taking leftover space.
When a view needs ready tiles not yet resident, it uploads them and rerenders
before publication, with at most two bounded passes; unchanged decoded objects reuse
their texture slots. Provider/release keys remain distinct. Slots hold weak CPU
tile references, so GPU residency does not pin another decoded cache. The frame
holds a short-lived ready snapshot while uploading. If the visible working set exceeds
atlas capacity, the CPU renders that view. This limit is additional to Moka's
256 MiB decoded target and the 128 MiB MBTiles disk target.

Reusable output, dependency and readback buffers grow to their validated high
water marks. At the maximum 4-million-pixel/zoom-10 view, output and readback total
about 36 MiB, the dependency buffer about 5.3 MiB, and its host reset data another
5.3 MiB, excluding driver allocation overhead and normal frame buffers.

The shader reports sampled and missing parent/desired tile keys and their nearest-pixel
priorities through a bounded zoom pyramid. CPU admission still sorts coarse
levels first, filters pending jobs, and preserves the admission/retry
limits. Precise CPU repair contributes its dependencies too. Compact dependency metadata accompanies pixel readback.

No network or SQLite operation runs in a shader or on the UI thread.

The dependency buffer holds the base map's zoom pyramid and, when night lights show,
theirs after it (to their zoom 8 at most).

Completed RGBA is read back to the app preparation callback, texture
upload and immutable screenshot snapshot. Consequently this is genuine GPU
rasterization, but **not a zero-copy GPU presentation path**. UI texture upload,
label layout and overlays retain their ownership and timing costs.
Global startup warming remains satellite zooms 0–4, 341 tiles, subordinate to
visible requests; other providers stay on demand.

## Verification

Original CPU fixtures remain unchanged. Naga validates the shader and uniform
layout in headless tests. Device-independent tests also verify backend
selection and one-time failure fallback. Native tests are explicit so headless CI
never silently treats a missing adapter as a successful GPU test:

```sh
cargo test -p globecraft-render gpu_overview_matches_cpu --locked -- --ignored --nocapture --test-threads=1
RAYON_NUM_THREADS=8 cargo test -p globecraft-render warm_gpu_frame_time --locked -- --ignored --nocapture --test-threads=1
```

The first exercises every provider on both Earth models, both hemispheres/poles, the
polar caps' edge, the dateline, sunlight, atmosphere, night lights,
loaded/pending/missing imagery, tile coverage and device-loss reporting.
The second uses isolated warm 256×256 fixture tiles and compares complete frames,
including upload, GPU wait/readback, sky/precision repair and tile dependencies.
It excludes egui presentation, overlays and live services. Always report image
error statistics together with timings; initialization and the first upload are
excluded from warm medians. Release validation on Windows and Linux remains separate from local macOS checks. The benchmark also loads 597 decoded tiles and rotates across
three hemispheric views to check that exceeding the atlas in the RAM cache does
not unnecessarily disable GPU rendering. See the [native CPU baseline](../PERFORMANCE_BASELINE.md).

`gpu_device_loss_publishes_a_cpu_frame` (also explicit) destroys a device and checks
that the CPU draws that frame and that a new device takes over after the delay.

## Parity after the default-view fix (2026-11-15)

Software Vulkan (lavapipe/llvmpipe, Linux), so the timings say nothing about a GPU:

- Parity test, 576 frames of 65×49: 7.3 million channels, mean error 0.0001 levels,
  none more than one level off, tile dependencies equal to the CPU's.
- Frame benchmark with the default Earth model (WGS 84) and warm satellite imagery:
  512×384 no channel more than one level off; 2000×1488 one channel in 11.9 million
  two levels off; the 597-tile residency views at most one channel two levels off.
- Before the fix the parity test failed on the same adapter (mean error 0.54, 1.15 %
  of channels over one level): the CPU drew the polar caps and the shader did not.

## Local measurement (2026-10-13)

Apple M1 Pro / Metal, optimized development build without debug information or
incremental output, eight Rayon workers, 341 warm overview fixture tiles,
satellite/sunlight/atmosphere, grid off, 14,000 km. Fifteen warm frames per size,
reused buffers, first frame excluded; no concurrent build during measurement:

| Raster | CPU median | GPU complete-frame median | Maximum channel difference |
| --- | ---: | ---: | ---: |
| 512×384 | 1.983 ms | 2.107 ms | 1 |
| 2000×1488 | 28.417 ms | 8.779 ms | 1 |

This is about 3.2× faster for the larger **warm overview raster**, not an
application FPS guarantee or a close-view improvement. Initial adapter creation,
uploads on a cold cache, overlays, UI presentation, other adapters and platforms
need separate measurements.

## Desktop checks

Local macOS checks used isolated settings and cache copies. Metal rendering,
continuous rotation, Settings during motion, sunlight, PNG export (2000×1488),
coordinate fly-to at 3 km, exact-grid CPU fallback and window resizing were
exercised. A forced-CPU instance displayed the globe without GPU initialization.
Native device destruction also verified publication of a CPU frame and session
fallback. Windows/Linux GPU behavior and packaging are not locally verified.

## Dependency upgrade validation (2026-10-14)

The wgpu 30.0.1 upgrade passed native Metal CPU/GPU parity, tile dependency
coverage and device-loss fallback tests on Apple M1 Pro. The isolated complete-frame
benchmark used the default optimized development profile (debug information enabled),
eight Rayon workers and fifteen warm frames per size; initialization and first
upload were excluded. No concurrent build ran during measurement.

| Raster | CPU median | GPU complete-frame median | Maximum channel difference |
| --- | ---: | ---: | ---: |
| 512×384 | 2.294 ms | 1.438 ms | 1 |
| 2000×1488 | 49.911 ms | 18.744 ms | 1 |

The 597-tile residency case also preserved pixels and dependency coverage across
three hemispheric views. These measurements use different build settings from
the earlier baseline and do not establish an upgrade speedup or application FPS.
Headless UI regressions passed; native desktop interaction, Windows/Linux GPU
behavior, packaging and live map services were not rechecked for this upgrade.
