# Native CPU baseline, 2026-10-13

One macOS ARM64 native run used the optimized development profile (`opt-level=2`,
no debug information/incremental output), eight Rayon threads and an isolated
cache. Cached geographic data files and z0–4 satellite files were copied;
the real user cache/settings were not changed. Live detail downloads and lazy
municipal loading remained enabled. Compilation/checks finished before measured
phases. This is an instrumented observational baseline, not a controlled FPS
benchmark or a cross-platform result.

| Phase / operation | Samples | Median | p95 |
| --- | ---: | ---: | ---: |
| Warm rotation: 808×601 raster | 1,619 | 5.812 ms | 20.064 ms |
| Warm rotation: overlay preparation | 1,628 | 7.572 ms | 13.920 ms |
| Warm rotation: request-to-UI delivery, 808×601 | 1,505 | 24.798 ms | 64.656 ms |
| Warm rotation: app UI update | 2,836 | 0.605 ms | 1.339 ms |
| Flight/detail loading: 2000×1488 raster | 36 | 64.282 ms | 133.633 ms |
| Flight/detail loading: app UI update | 588 | 0.189 ms | 0.489 ms |
| Flight/detail loading: decode queue wait | 442 | 0.024 ms | 3.639 ms |
| Flight/detail loading: feature queue wait | 28 | 0.024 ms | 5.843 ms |
| Municipal/metropolitan loading: index build | 2 | 584.466 ms | 633.301 ms |
| Municipal/metropolitan loading: app UI update | 67 | 0.414 ms | 2.944 ms |
| Boundary-heavy close rotation: 808×601 raster | 1,277 | 16.180 ms | 187.076 ms |
| Boundary-heavy close rotation: app UI update | 3,218 | 0.215 ms | 0.922 ms |
| Boundary-heavy close rotation: decode queue wait | 4,027 | 0.186 ms | 10.460 ms |

Phase lengths were about 43 seconds warm rotation, 53 seconds flight/detail
loading, 79 seconds stationary boundary loading, and the recorded interval in
the JSON for close rotation. The coordinate flight targeted São Paulo at 3 km.
Country/state/name layers stayed on; municipal/metropolitan layers were enabled
for the final two phases. Automatic rotation at close zoom is a tile-churn stress
case, not a typical manual pan. World warming did not eliminate higher-detail
requests. Eleven of the 28 feature jobs overlapped decode work during the flight
phase, exercising concurrent services; the later stationary boundary phase had
no active tile requests.

Timed tile-admission lock acquisition rounded below 1 microsecond at p95; admission
itself stayed below 0.04 ms at p95 across the moving phases. This provides no
justification for DashMap in that measured path. It does not measure every cache
miss-path lock or rule out Moka/internal contention under different workloads.

The decoded-cache estimate reached 256 MiB during close rotation, with admission
bounded at 48. Four `ps` RSS snapshots were approximately 209, 52, 52 and 185 MiB;
an additional moving-view sample was 275 MiB. These are sparse resident-memory
observations, not total allocations, peak memory, or an equivalent of Moka's byte
estimate. They cannot establish a hard application memory ceiling.

Outliers matter: the stress phase had a 1.267-second active-resolution raster
maximum, 4.282 seconds among 25 full-resolution rasters, and a 497 ms app-update
maximum. Warm rotation also had large spikes. No causal attribution of those
outliers was established; scheduling, loading and memory pressure are not held
constant. Short typical UI updates do not establish uniformly smooth input.

## Interpretation

Full-resolution raster work remains a substantial cost. World-view overlay
preparation is also significant, and large dataset changes rebuild indexes. Keep
those as distinct optimization candidates.

The report measures app update wall time, not egui's later tessellation, actual
driver texture upload, GPU execution, screen presentation or OS input-to-display
latency. `frame.delivery` ends when the UI polls a completed frame. Profile events
can overlap across threads, so medians must not be summed into a frame budget.
No profile events were dropped. Capture adds instrumentation overhead.

Aggregate data and phase intervals are in
[the JSON report](benchmarks/2026-10-13-native-profile.json). Reproduction instructions
are in [contributing](../CONTRIBUTING.md). Raw capture for this run was retained at
`/tmp/globecraft-native-profile.csv`, outside Git; temporary native cache files were
removed after the owned app closed cleanly.

## Isolated aircraft fixtures, 2026-10-14

These are separate headless operation measurements on an Apple M1 Pro with
Rust 1.99.0 and the workspace's optimized development profile (`opt-level=2`).
Five rounds of 100 iterations use 20,000 deterministic records, an 800×600
logical-point overview and 6,375 visible aircraft. The baseline is upstream's
projector/snapshot implementation at `e9f193b`. No public service, native window
or user cache/settings participates.

| Operation | Before | After |
| --- | ---: | ---: |
| UI snapshot acquisition | 8.153 ms median | under 0.001 ms; shared `Arc` |
| Uncached geographic projection | 3.478 ms median | 2.727 ms median; background |
| Unchanged-view geometry acquisition | no reuse path | under 0.001 ms |
| Initial icon mesh construction | not measured | 2.605 ms, one cached mesh |
| Cached headless egui pass | not measured | 0.002 ms median |

Uncached projection samples ranged 3.469–3.727 ms before and 2.618–2.828 ms in
the final run. An earlier inline run ranged 3.134–4.690 ms; scheduler/thermal state
was not controlled, so these do not establish a general projection-kernel speedup. An initial
cross-crate extraction measured 4.705 ms median; marking the small core destination
function inline restored the shared optimization boundary. The main improvement
is removing repeated source cloning/sorting and geographic projection from UI
passes, then sharing cached meshes. Snapshot/geometry accesses measured about
0.010/0.024 microseconds; the table uses conservative sub-microsecond reporting
because these short measurements are sensitive to timer resolution.

The egui result includes headless pass setup and cached shape submission. It
excludes a native event loop, final frame tessellation, driver upload, GPU execution,
presentation, live-service latency and complete app-frame time. First geometry
preparation and changed-view mesh construction remain real costs. Telemetry,
geometry and mesh residency are additional to the decoded-tile cache target.
Native interaction and Windows/Linux runtime behavior were not measured.

Reproduce with the manual fixture tests:

```sh
cargo test -p globecraft-data aircraft_snapshot_cost --locked -- --ignored --nocapture --test-threads=1
cargo test -p globecraft-render aircraft_projection_cost --locked -- --ignored --nocapture --test-threads=1
cargo test -p globecraft-render aircraft_geometry_reuse_cost --locked -- --ignored --nocapture --test-threads=1
cargo test -p globecraft aircraft_mesh_repaint_cost --locked -- --ignored --nocapture --test-threads=1
```

Headless regressions cover source priority/expiry and request cadence, failure wakeups, coalescing, displayed-frame
alignment and shared mesh identity. Optional profiling adds `aircraft.snapshot`,
`aircraft.project` and `ui.aircraft_shapes` through the bounded background recorder.


## Earth-model CPU fixture, 2026-10-14

On the same M1 Pro / Rust 1.99 optimized development profile, the isolated warm
512×384 CPU fixture uses eight Rayon threads, 341 local 256×256 tiles (85.25 MiB),
grid, atmosphere and sunlight. Each invocation has five rounds of 200 frames.
The sphere comparison alternates the existing pre-model test executable and the
new executable (both profiling-enabled, with no active capture). Neither executable
opens a native window or uses user settings/caches or public services.

| Model/run | Median CPU frame |
| --- | ---: |
| Sphere before, initial invocations 1 / 2 | 2.224 / 2.390 ms |
| Sphere after, initial invocations 1 / 2 | 2.158 / 2.132 ms |
| Sphere before / after merging KML, before the inline hint | 2.186 / 2.307 ms |
| Sphere before / final combined implementation | 2.057 / 2.102 ms |
| WGS 84, final combined implementation | 2.460 ms |

The final WGS 84 round samples were 2.437–2.561 ms. The final sphere invocation
ranged 2.049–2.184 ms before and 2.073–2.169 ms after. The earlier second
invocation included a 3.078 ms after sample; it was retained in its round results.
After the merge, specializing the sphere's normal use and retaining the inline
ray-kernel boundary avoids carrying ellipsoid-only work into that pixel path.
Paired results vary in both directions; the last pair is about 2% slower. These
limited runs do not establish zero overhead or a general speedup/regression.
Scheduling, thermal state and code layout remain uncontrolled. The selected
ellipsoid performs additional intersection/normal/curvature work; that math is
not imposed on sphere pixel loops. Other views, GUI latency, GPU execution and
total app FPS are not established by this fixture.

Reproduce the current model runs separately, without concurrent compilation:

```sh
RAYON_NUM_THREADS=8 cargo test -p globecraft-render warm_cpu_frame_time --locked -- --ignored --nocapture --test-threads=1
RAYON_NUM_THREADS=8 cargo test -p globecraft-render warm_ellipsoid_cpu_frame_time --locked -- --ignored --nocapture --test-threads=1
```

See [Earth models](features/EARTH_MODELS.md) for numerical checks and extension boundaries.

### Earth-model review rerun

The review additionally tested stable spherical antipodal paths/distances,
forward/inside/on-surface ray intersections and ellipsoidal grid widths. It reused
geodesic route coefficients and cached quadric ray coefficients per frame.
The existing warm-frame workload above was rerun against a saved pre-review
all-features test executable and the final reviewed executable, without concurrent
Cargo builds. Each entry is the median of five rounds, in milliseconds:

| Model | Execution order | Before review | After review |
| --- | --- | --- | --- |
| Perfect Sphere | Before, then after | 2.412 | 2.499 |
| Perfect Sphere | After, then before | 2.432 | 2.448 |
| WGS 84 | Before, then after | 2.837 | 3.071 |
| WGS 84 | After, then before | 2.597 | 2.580 |

Individual final-run round samples ranged from 2.126–3.044 ms for the sphere and
2.502–3.826 ms for WGS 84 across the two binaries. Earlier invocations also varied
substantially. These overlapping, variable samples do not establish a speedup or
zero overhead. This remains a CPU raster benchmark; it does not measure ruler
projection, native UI interaction, GPU execution or live service readiness.

### Ellipsoid sampling microbenchmarks

On the same M1 Pro / Rust 1.99 optimized development profile, matched
`globecraft-render --all-features` test executables compare the pre-optimization
code with normal reuse and all-sky ray-batch short-circuiting. No native window,
capture recorder or service is active. Seven rounds of 32 repetitions exercise
three 128×96 views: 150 m altitude near the dateline, 500 km near the north pole,
and the 14,000 km overview. Surface timing uses 29,137 WGS 84 hits (29,140 sphere
hits), with grid, sunlight and atmosphere enabled; sampling and tile access are
outside that isolated phase. The ray harness includes model dispatch and complete
four-lane sample construction. Before/after and after/before invocation pairs
give these round medians, in nanoseconds:

| Phase | Before, two invocations | After, two invocations |
| --- | ---: | ---: |
| Sphere, four-ray batch | 45.177 / 44.695 | 45.120 / 44.856 |
| Sphere, surface hit | 56.506 / 56.112 | 56.290 / 56.119 |
| WGS 84, four-ray batch | 71.754 / 71.108 | 68.749 / 68.995 |
| WGS 84, surface hit | 68.328 / 67.858 | 48.503 / 48.228 |

The WGS 84 surface phase is about 29% cheaper in these runs. Reusing the existing
unit normal removes repeated angle/trigonometric conversions; f64 precision and
the sphere arithmetic are retained. Sky-only batches avoid unused surface work.
The existing eight-thread warm-frame fixture measured WGS 84 at 2.635 → 2.204 ms
and the sphere at 2.165 → 2.201 ms in the final matched run. Earlier intermediate
invocations varied more widely, including 3.200 → 2.643 ms for WGS 84; scheduling,
thermal state and code layout remain uncontrolled. These figures do not establish
application FPS, a general improvement for every view, or zero sphere overhead.

Before/after differential captures cover both models, four views (including poles,
dateline and altitude limits) and all four providers. All 32 127×95 RGBA frames and
32 sorted missing-tile/priority lists matched byte for byte. These temporary
captures stay outside Git; the original raster fixtures are unchanged. Independent
tests check Mercator coordinates and curvature from surface normals. This does
not validate native UI delivery, public imagery or GPU execution.

```sh
cargo test -p globecraft-render --all-features earth_model_kernel_time --locked -- --ignored --nocapture --test-threads=1
GLOBECRAFT_BENCH_OUTPUT=/tmp/globecraft-kernel-capture cargo test -p globecraft-render --all-features earth_model_raster_capture --locked -- --ignored --nocapture --test-threads=1
```

Use different isolated capture directories for before and after versions. Compile
each version with the same package/features/profile, save its executable, and
alternate runs without concurrent builds. Workspace feature unification can
produce a different test executable from the focused package command above.

## Hidden illustration construction (2026-10-14)

An isolated optimized-dev CPU comparison on macOS ARM64 measured seven rounds
of 200 scene constructions at 800×600, yaw 1.2, tilt 0.62 and zoom 1.0.
The baseline was the scene builder extracted from `432426f`; the current builder
uses the same colors, draw order, rim, UVs and 6,144 triangles. Expanded triangle
positions, UVs and colors matched exactly on this host for three rotation/tilt/
zoom combinations. The permanent corner samples allow f32 libm rounding on other
platforms. No original globe raster fixtures were regenerated.

| CPU scene construction | Median per construction | Surface vertices |
| --- | ---: | ---: |
| Original | 72.679 µs | 12,288 |
| Shared topology | 22.534 µs | 3,201 |

These timings exclude GUI tessellation, texture upload, networking and frame
presentation, and do not establish an application FPS gain. Stationary scenes
also reuse tessellated meshes; cache identity/invalidation is tested headlessly.
The original comparison helper was temporary and is not part of the source.
Rerun the current production construction workload without concurrent builds:

```sh
CARGO_PROFILE_DEV_INCREMENTAL=false cargo test -p globecraft flat_scene_construction_time --locked -- --ignored --nocapture --test-threads=1
```

## GlobeCraft 0.3 interface and terrain (2026-10-22)

Linux x86-64 container, 2 cores, release profile, before = `559438c`, after =
the RustGIS-style interface rewrite. Old and new executables were alternated six
times; interface frames count the measuring thread's CPU time
(`/proc/thread-self/schedstat`), so tile loaders competing for cores do not count.

| Workload | Before (median) | After (median) |
| --- | ---: | ---: |
| Interface frame, 1480×900, stationary, borders shown | 0.47–0.56 ms | 0.19–0.24 ms |
| Warm 512×384 CPU globe frame | 11.58 ms | 11.78–12.07 ms (noise) |
| Warm 512×384 CPU frame with 3D terrain | 38.1 ms | 33.5–34.9 ms |

Interface gains come from two changes. The cursor readout now remembers its fitted
font size instead of laying out every width sample at each candidate size every frame
(that loop was 28 % of the old interface's instructions under callgrind). The cached
border overlay is tessellated once per view into a single mesh instead of cloning
~11,000 line-segment shapes into the frame, sorting them into layers and tessellating
them again each frame; `worker_meshes_tinted_on_the_ui_match_the_triangles_egui_would_draw`
checks that the triangles are identical. The harness above skips egui's final
tessellation, so the real application saves additional time per frame.

```sh
cargo test --release -p globecraft interface_frame_time -- --ignored --nocapture
GLOBECRAFT_UI_PARTS=1 cargo test --release -p globecraft interface_frame_time -- --ignored --nocapture
cargo test --release -p globecraft-render warm_terrain_cpu_frame_time -- --ignored --nocapture --test-threads=1
```

After merging the boundary/LOD branch (same harness, same machine, three runs):
interface frame 0.27–0.32 ms (default), 0.20 ms with the ribbon collapsed, 0.12–0.14 ms
without the left pane. The increase over the pre-merge figure has not been attributed
yet; the total stays well under the 0.47–0.56 ms of `559438c`. Overlay shapes per frame: 384, borders drawn as two cached meshes.

### CPU frame after the merge (2026-10-22, later session)

The container was slower in this session, so earlier absolute numbers do not compare.
Measured in the same session, warm 512×384 CPU frame:

| Build | Median |
| --- | ---: |
| `559438c` (original, LOD bias 0.5, bilinear) | 15.4–16.2 ms (3 interleaved runs) |
| Boundary branch as handed off (LOD bias 1.0, area filter) | 15.0 ms |
| Rewrite before the merge | 15.4 ms |
| Rewrite after the merge, separable area filter | 15.1–16.5 ms (3 interleaved runs with `559438c`) |

The handed-off branch renders one level sharper (`LOD_SHARPNESS_BIAS` 0.5 → 1.0) and
area-filters minified texels so the sharper level does not alias. The area filter now
computes its column weights once per sample and sums each row before applying the row
weight (separable box filter), which removed about 1.8 ms of the 3.2 ms the filter
cost in this workload. Net result: the sharper, filtered imagery costs the same as
the original bilinear frame. 3D terrain: 41–42 ms after the merge vs 44.9 ms for the
pre-merge rewrite in the same session.

### Overlay rebuilds while descending through the boundary bands (2026-10-22)

`boundary_streaming_descent` (ignored, needs network) descends from 9,000 to 150 km
over 40 s above central Europe and reports the slowest UI passes. Each new overlay
view used to rebuild its shapes on the UI thread: 30–70 ms with 40 k border segments
and ~900 label candidates. Split: tessellation 10–30 ms, labels 8–35 ms, because egui
drops galleys not used in the previous frame and rebuilds happen a few frames apart,
so every rebuild laid out every name again (~50 µs each).

| Per rebuild, same descent | Before | After |
| --- | ---: | ---: |
| Border tessellation on the UI thread | 10–30 ms | 0 (worker); tint 0.3–0.9 ms |
| Label layout, names already seen | 8–35 ms | < 1 ms |
| Label layout, new layer arriving (~1,000 names) | 45 ms in one frame | ≤ 120 names per frame |
| UI pass p99 over the descent | 19.5 ms | 9.1–9.8 ms |

The container has two cores shared with the Rayon raster pool, so single passes still
show 15–50 ms outliers that the in-function timers attribute to preemption rather
than work (a build timed at 0.1 ms inside took 10 ms outside). Recheck on the Mac.

```sh
cargo test --release -p globecraft boundary_streaming_descent -- --ignored --nocapture
```


### Instant state-border handover (2026-10-22)

The Natural Earth state lines are now cut exactly where municipal state borders replace
them (13.12), instead of being split at vertices near a 0.05° cell and cross-faded.
Synthetic Germany-sized view on the 2-core container, release build: 60 state lines of
301 points (0.02° spacing), half of each traced by municipal borders.

| Step | Before (vertex runs, 3×3 cells) | After (exact cut) | Where it runs |
| --- | --- | --- | --- |
| Coverage per view | 3.0 ms | 1.5 ms | overlay worker, every rebuilt view |
| Coverage index | 1.0 ms | 5.0 ms | municipal publisher, once per publish |

The cut tests each coarse vertex once (steps of 3 km on longer segments) and bisects
only where coverage changes. Run with:

```sh
cargo test --release -p globecraft-render handover_cut_time -- --ignored --nocapture
```

Not yet measured on the Mac.

### State membership for geoBoundaries countries (2026-10-22)

Release build, 2-core container, on the feature pool when a country loads: reading the
municipal and state files and assigning every municipality its state took 0.8–1.0 s for
Czechia (6,361 rings, 14 regions) and Poland (2,584, 16), 0.3 s for Austria and Slovakia,
and 2.3 s for Italy (8,561 rings, 107 provinces). The tracing reference that this replaced
(0.4 s and about 60 MB, once per Natural Earth detail change) is gone.

### Municipal country borders (2026-10-22)

Assembling a municipal area now also decides which side draws each country border
(outline segments tested against an index of Natural Earth's land-border lines, built
once per detail level on the CPU pool). Release build, 2-core container, real
geoBoundaries data: 8 ms for Italy and Austria (2,300 rings in view), 15 ms for Czechia
alone and 30 ms for Czechia, Poland, Austria and Slovakia together (14,600 runs). It runs
on the municipal publisher, not the frame.

### Municipal classification and publishing (2026-10-23)

Release build, 2-core container, real geoBoundaries files, loaded in this order so
later countries are classified against their stored neighbours. Classification is the
step that compares every outline segment with the rings around it; file reading and
state membership are unchanged.

| Country | Rings | Before | After |
| --- | --- | --- | --- |
| Czechia | 6,361 | 1,331 ms | 433 ms |
| Poland (Czechia stored) | 2,584 | 1,507 ms | 344 ms |
| Austria | 2,115 | 506 ms | 181 ms |
| Slovakia | 2,985 | 488 ms | 145 ms |
| Italy | 8,561 | 1,244 ms | 547 ms |
| India (districts) | 1,029 | 333 ms | 301 ms |
| Bangladesh (unions) | 5,706 | 1,267 ms | 444 ms |

Changes: 0.02° search cells instead of 0.05°, neighbours' segments that already have a
class are not searched again, and rings are classified in parallel on the feature pool
(2 threads). Every class came out identical except one Italian segment that the old
search missed (longitude padding, see ROADMAP 14.9). The assembled layer is identical.

Assembling a published area (10–45 ms for 2,000–15,000 rings) now runs on the feature
pool from a snapshot instead of under the store lock that the UI thread takes every
frame. Run with:

```sh
GLOBECRAFT_MUNICIPAL_CHECK=<dir> GLOBECRAFT_MUNICIPAL_COUNTRIES=CZE,POL,AUT,SVK,ITA,IND,BGD \
  cargo test --release -p globecraft-data real_countries_classify -- --ignored --nocapture
```
