# Frame-budget watchdog (roadmap 10.4)

**Status:** implemented 2026-11-05, profiling builds only.

## Why this item

The 2026-11-01 handoff records a tilted-relief frame whose worst UI passes
reached 350 ms on Windows and notes that those frames “were not traced”. The
profiling build already records `ui.update` for every pass, but that is one
aggregate number: it says a frame was slow, not which part of the frame was
slow. The roadmap item asks for a bounded trace when a UI pass exceeds a
budget, so a capture explains a freeze instead of only proving one happened.

This work is deliberately outside the active border, polar, live-layer and
timeline threads. It changes no normal build, no rendering and no persisted
data. It is a measurement tool for the next time one of those threads produces
a freeze.

## What it does

`globecraft-data::profiling::PassWatchdog` measures one UI pass through named
checkpoints. It emits nothing while the pass is under 50 ms. When the pass is
slow it emits:

- one `ui.slow` event with the whole pass duration, the viewport in physical
  pixels and the cache's inflight/resident values;
- one `ui.slow.<section>` event for each section that took at least 5 ms, up
  to 24 sections.

Every event of one pass has the same `unix_us` (the pass start), so a report can
group them without a schema change. The normal build compiles the watchdog out;
the profiling build pays one `Instant::now()` per checkpoint and does no file
I/O or heap allocation on the UI thread.

The app checkpoints these sections, in execution order:

| Section | Work |
| --- | --- |
| `setup` | theme, UI scale, settings persistence |
| `input` | native-menu polling, shortcuts, dropped files, flight step |
| `header` | merged title/header row |
| `ribbon` | ribbon |
| `status` | status bar |
| `panes` | left and right panes |
| `windows` | settings, menu, Explorer, altitude, Add Data, here, ruler points, Command Search |
| `live_maps` | weather/radar refresh |
| `boundary` | border-tile request, download log, still-shot poll, ground follow |
| `globe` | central globe or flat-earth panel |
| `native_menu` | macOS native-menu refresh |

The section names are stable so old captures remain readable. `windows` is
intentionally coarse: it is the first split that tells us whether the globe,
the panes or the modal windows dominate a freeze. Splitting it further is
cheap once the App split (roadmap 10.2) gives each window its own state.

## Using it

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_INCREMENTAL=false \
  cargo build -p globecraft --features profiling --locked
GLOBECRAFT_PROFILE=/tmp/globecraft-freeze.csv RAYON_NUM_THREADS=8 \
  ./target/debug/globecraft
python3 scripts/profile_summary.py /tmp/globecraft-freeze.csv
```

The JSON report now includes `slow_passes`:

```json
{
  "slow_passes": [
    {
      "unix_us": 1791904857946195,
      "total_ms": 84.2,
      "viewport": [2560, 1440],
      "sections": [
        {"section": "globe", "ms": 61.5},
        {"section": "windows", "ms": 11.8}
      ]
    }
  ]
}
```

Interpretation:

- one large section points at that section's code;
- a total much larger than the listed sections points at many small sections,
  or at work outside the checkpoints;
- compare `viewport`, inflight and resident bytes with nearby `frame.raster`
  and `ui.texture_submit` events to separate a heavy view from a cache/network
  effect;
- `dropped_events_counter` still reports recorder saturation. A freeze burst
  can fill the 8,192-event queue; the watchdog does not retry or block the UI.

The 50 ms threshold is a policy choice, not a measurement: it is below 20 fps
and well above the 0.44 ms interface pass in the current baseline, so normal
frames stay silent while a visible freeze leaves a trace. The 5 ms section
floor keeps a single trace to a small constant size. Change both constants in
`crates/globecraft-data/src/profiling.rs` deliberately, and record the reason in
`PERFORMANCE_BASELINE.md` if they become part of a comparison.

## Files changed

- `crates/globecraft-data/src/profiling.rs`: `PassWatchdog`, shared event
  emission, two unit tests.
- `apps/globecraft/src/app/mod.rs`: watchdog creation, checkpoints, viewport
  and cache values in the `ui.update` event.
- `scripts/profile_summary.py`: `slow_passes` grouping, exclusion from normal
  timing groups, decoded-cache estimate fix.
- `scripts/test_profile_summary.py`: offline checks for both report changes.
- `CONTRIBUTING.md`, `docs/ROADMAP.md`, this document.

## Verification

Ran:

```sh
cargo fmt --all -- --check
cargo test -p globecraft-data --features profiling --locked profiling
cargo clippy -p globecraft --features profiling --all-targets --locked -- -D warnings
cargo clippy -p globecraft-data --features profiling --all-targets --locked -- -D warnings
python3 -m unittest discover -s scripts -p 'test_profile_summary.py' -v
```

A 12-second profiling capture of the CPU renderer at startup produced 2,816
events, 531 `ui.update` samples, a maximum `ui.update` of 11.3 ms and no
`ui.slow` events. That is the expected silent path: normal startup stayed below
the 50 ms threshold, so the watchdog added no trace noise. The slow path itself
is covered by the unit test that builds a 60 ms trace and checks the summary and
section events; a real 350 ms freeze was not reproduced in this session. Do not
treat these numbers as a new performance baseline.

## Review notes beyond this item

These are observations from the profiling and UI code, not all fixed here.

1. **`Values` is an overloaded bag.** `frame.raster` stores camera altitude in
   `resident_bytes` (metres) and tilt in `count` (tenths of a degree), while
   `ui.update` stores decoded-cache bytes in `resident_bytes`. The summary
   script used `max(resident_bytes)` as the decoded-cache estimate, so raster
   altitude could inflate or distort it. This change excludes `frame.raster`
   from that estimate, but the schema still hides meaning in field names. A
   future capture format should either add named columns or emit separate
   event kinds; changing the CSV header now would break existing scripts and
   captures.
2. **Timestamps are not uniform.** `Span::new` records `unix_us` at the start,
   while `Span::elapsed` records it at the call site (the end). Phase filtering
   therefore assigns a synthetic span to its end time, but an ordinary span to
   its start. The watchdog deliberately records the start so all events of one
   pass group. Documenting `unix_us` as “event time” is not enough; a future
   format should carry both start and end microseconds.
3. **`flush()` can silently lose its request.** It uses `try_send`; if the
   bounded queue is full, the final flush message is dropped and `flush()`
   returns without a word. The profiling documentation says failures appear in
   stderr, but queue saturation is only visible as `dropped_events`. At
   shutdown a bounded blocking send (with the existing two-second timeout)
   would be safer than `try_send`.
4. **The UI path is instrumented coarsely.** `ui.update`, `ui.overlay_shapes`,
   `ui.texture_submit` and `ui.aircraft_shapes` exist, but font layout, label
   occupancy, individual panes and native-menu polling have no spans. The
   watchdog narrows this to a section, not a function. The next useful step is
   a child span at the label/occupancy boundary, because that is the work the
   architecture already moves off the UI thread and the work most likely to
   regress silently.
5. **A UI pass is not a frame.** `ui.update` ends before eframe tessellates and
   submits the presentation backend, and it does not include GPU execution.
   The watchdog therefore cannot explain a slow frame caused by presentation,
   tessellation or the driver. Keep `ui.texture_submit` and the GPU parity
   work separate; do not read `slow_passes` as a frame-time guarantee.
6. **The watchdog can miss the first frame of a freeze.** It measures from the
   start of `App::ui`; input delivery, window events and the previous frame's
   presentation are outside it. If a freeze begins before `ui` is entered, the
   trace starts late. Correlating `stable_dt`/`unstable_dt` with `ui.slow`
   would close that gap cheaply.
7. **The section cap is silent.** At most 24 sections are stored; if a future
   pass has more, the extra sections are not represented. Today the app uses
   11, so this is theoretical. If the cap is approached, emit one
   `ui.slow.uncounted` event instead of silently dropping the tail.
8. **No CI coverage for the Python report.** The tests run with the documented
   command but are not part of a workflow. Roadmap 10.1 is the right place to
   add them, together with the screenshot tests, rather than adding a one-off
   workflow step here.
9. **The App is still the orchestration bottleneck.** The checkpoint labels
   mirror the current `ui()` body and make the cost of that shape visible. They
   are also a map for the 10.2 split: `windows` is the first section that should
   become several independently owned states.
