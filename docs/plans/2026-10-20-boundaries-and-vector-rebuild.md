# Plan: border polish → municipal loading → vector rebuild

Status: active plan, written 2026-10-20 from thread `thr_bqypminmt3` (which
errored mid-turn; nothing below is committed unless stated). Branch:
`feat/boundary-coverage-15.1` on the fork remote
(`fork` = FireflyTheReal/GlobeCraft, `origin` = FireflyTheReal/GlobeCraft).

Owner crates (from AGENTS.md): `globecraft-data` = sources/loading/snapshots,
`globecraft-render` = projection/visibility/occupancy (emath, no egui),
`apps/globecraft` = colors, widths, widgets, dialogs. Never reverse this.

## 0. Current state (verified 2026-10-20)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -D
  warnings`, `cargo test --workspace --locked`, `cargo check` all **pass** on
  the working tree.
- The working tree contains an **uncommitted, user-unseen polish pass** that
  addresses earlier feedback:
  - Altitude fades for state and municipal borders/labels
    (`apps/globecraft/src/app/overlays/projected.rs`: `layer_opacity`,
    smoothstep bands, `with_opacity`).
  - Municipal outlines gated to ≤ `MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM` (160 km),
    states suppressed while municipal outlines are visible, country outlines
    drawn last (`crates/globecraft-render/src/overlays/mod.rs`).
  - Text-map zoom crossfade: `floor(lod)` + a short end-of-band fade
    (`TEXT_ZOOM_FADE_START = 0.9`) in `render/src/pixel.rs`, `lib.rs` and
    `gpu/shaders/overview.wgsl`; the shader/`sample_chain` only blend when the
    finer level actually resolves, which removes the "flashing levels while
    zooming" bug the user reported.
  - Municipal patches now `replace: true` (brazil.rs, local.rs, world.rs) so
    reloads do not stack outlines; `updates.rs` semantics unchanged.
  - `load.rs`: retry cooldown applies only after failures (`local_last_failed`),
    success no longer blocks a moved viewport for 75 s; load timings ("in N ms")
    added to every boundary status log.
  - Two request-priority fixtures (`dateline-topo.misses`,
    `polar-streets.misses`) were regenerated; the justification is documented in
    `tests/fixtures/README.md`. **Do not regenerate any fixture again without an
    equally explicit justification.**
- `docs/DATA_SOURCES.md` (tracked, older) and root `DATA_SOURCES.md` +
  `data_sources_triage.csv` (untracked, newer) are diverged — see §7.

## 1. User's wishes (canonical list — do not lose these)

1. **Border styling, Google-Maps-like:** country borders **white** and a little
   **thicker**; state/province borders **dashed** — high-res states fully visible
   at ~**500 mi** (≈ 805 km), low-res states fading in over **1000 → 500 mi**
   (≈ 1609 → 805 km, quick fade); municipal borders **slimmer and more
   dashed** than states. All layers fade rather than pop.
2. **Municipal loading:** download only the municipal borders actually visible,
   for **all** countries (today only DE/FR/ES are viewport-limited; Brazil is
   very laggy when the whole country loads); widen the request area so small
   pans do not trigger refetches; when zooming out, municipal outlines must
   **fade out and unload**, restoring state/province outlines. Country borders
   always stay on top.
3. **Smoothness first:** data should be fetched/rendered *before* it becomes
   visible so fades are graceful ("render it before its even visible"). Weather,
   cloud, satellite and vessel overview may stay whole-dataset.
4. **Upstream-only:** no self-hosting, no vendoring big datasets into the repo;
   consume public endpoints. Pre-baking is "enthusiastic for now" — do it only
   where trivial, otherwise say so and defer.
5. **The big rebuild** (agreed): keep `main.rs`, `globecraft-core` and the
   rasterizers as they are; rebuild the boundaries/overlay subsystem per
   `docs/features/VECTOR_MAP_DATA.md` (registry + tiles + LOD + cross-fade).
   Poles: user prefers a dedicated polar map eventually; the `PolarCap` cap
   layer is the accepted interim.
6. The user **values the download/efficiency/lag analyses** ("worth gold") —
   every perf-motivated change must ship with measured numbers.

## 2. Workstream A — land and verify the in-flight pass (small, first)

1. Have the user run the app (`cargo run -p globecraft`) and walk the desktop
   smoke checklist in `CONTRIBUTING.md`: zoom in/out over DE, FR, ES and Brazil,
   toggle country/state/city/metro borders, switch OSM/satellite while zooming.
   The previous thread errored before any visual verification.
2. Fix what the eye finds, then commit this pass as its own commit
   (styling+fade+fixtures together, message must mention the fixture
   regeneration rationale). Only commit when the user asks.

## 3. Workstream B — border styling and thresholds (small)

Status 2026-10-21: **landed** (uncommitted). Country outlines now use Natural
Earth `admin_0_boundary_lines_land` (bundled 1:110m, fetched 1:50m/1:10m), so
coastlines and lake shores are no longer outlined; polygon rings still feed
`country_at`. Countries are solid white 1.6 pt and drawn last, states dashed
7/5 pt light blue, municipal dashed 2.5/2.5 pt at 0.6 pt. Render emits the dash
fragments (`project_borders`) with phase continuity and tests cover the pattern.
States gate at 1,609 km and fade to full at 805 km, labels included.

All constants stay in `km` with `_KM` names (AGENTS.md); convert to miles only
at the UI boundary (core `units.rs` already formats miles).

1. **Colors/widths** live in `apps/globecraft/src/app/overlays/projected.rs`
   (`build_parts`): country → white, ~1.5 pt (keep the thin close-zoom variant
   if it reads better); state → dashed, same hue but dimmer; city (municipal) →
   slimmer (≤ 0.6 pt) and a denser dash than state.
2. **Dashes belong in render, not the app.** `project_borders`
   (`render/src/overlays/mod.rs`) already walks each polyline with
   `array_windows`; accumulate screen-space distance along the line and emit
   dash fragments there, so dash phase stays continuous across segment joins
   and simplification. Carry a per-`BorderKind` dash pattern (on/off lengths in
   points) through the existing `emit` callback — do **not** add an egui
   dependency to render, and do **not** let the app re-dash per segment (phase
   breaks look like noise). Watch `MIN_BORDER_SEGMENT_POINTS` simplification:
   dashes must survive it (emit dashes before the short-segment drop, or use a
   dash length ≥ the simplification threshold at that altitude).
3. **Thresholds**: today `OTHER_STATES_MAX_ALTITUDE_KM = 3000` and the app fade
   band is 3000 → 1500 km, which is far above the user's 1609 → 805 km request.
   Rework so: coarse/big-country states fade in 1609 → 805 km and are fully
   visible below 805 km; the detailed (50m/10m) states take over below the
   existing world-detail switch (`lod.rs`); municipal fade band stays ~160 →
   110 km. Keep line gating (render) and label visibility
   (`MapLabel::visible_at`, data) using the **same** constants — one source of
   truth in `globecraft-data/src/boundaries/mod.rs`.
4. Country outlines remain drawn last (already in the diff) and never fade.

## 4. Workstream C — municipal loading redesign (medium)

Status 2026-10-21: C1, C2 (partly) and C4 **landed** (uncommitted); C3 and the
Brazil-specific lag work are still open. Measurements: at 0.75° vs 0.30° a DE
view costs 1.6/0.34 MB (270/63 features), ES 0.33/0.06 MB (530/97), US 11 KB
(6), CA 113 KB (17). FR stays at 0.30° (5.4 MB, 464 features): 0.50° already
hits the 1,000-feature cap at 11.5 MB. USA and Canada were confirmed to accept
an ArcGIS envelope (HTTP 200), so both now use viewport queries; the claim in
§7 that Canada does not support a bbox was wrong.
C4 unloads above 250 km and skips while a load is in flight. C3 (client-side
clip for whole-country sources) still needs the cached country geometry and a
re-clip on movement to be correct, which is the same state the Overture tile
path replaces; it was not faked with a one-shot clip.

All knobs are in `globecraft-data/src/boundaries/` (`load.rs` timers,
`national.rs` `WFS_RADIUS_DEG`/`uses_viewport`, `world.rs` `REQUEST_RADIUS_DEG`,
`local.rs` chain, `brazil.rs` IBGE).

1. **Prefetch above the fade band** (fixes an inverted threshold the user
   explicitly complained about): requests currently start only below
   `LOCAL_MAX_ALTITUDE_KM = 110`, i.e. *after* the 160 → 110 km fade has
   finished, so outlines pop in at full opacity. Request at a prefetch altitude
   above 160 km (≈ 250 km), keep the visible/fade band at 160 → 110 km, and
   update the doc comments that state "requested below 110 km".
2. **Wider, hysteresis-free request area:** raise `WFS_RADIUS_DEG` /
   `REQUEST_RADIUS_DEG` (0.30° today ≈ 33 km) so a pan inside the loaded box
   does not refetch; tie the movement quantization (`LOCAL_MOVEMENT_DEG`) to
   half the radius. Measure the payload growth for DE/FR/ES before/after using
   the new "in N ms" logs and report bytes/feature counts.
3. **Viewport-limited everywhere (client-side clip first):** geoBoundaries and
   IBGE serve whole-country files. Download once (they are cached), then clip
   lines **and labels** to the viewport + margin in `local.rs`/`brazil.rs`
   before emitting the `Patch`, with `replace: true` on each clipped patch.
   This bounds draw cost and label occupancy for Brazil without new endpoints.
   Keep the OSM Overpass fallback as the true viewport source.
4. **Unload on zoom-out:** add an explicit clear — above ~250 km send a
   replacing empty `Layer::WorldCities`/cities patch (respect the
   `updates.rs` rule that local labels ride on WorldCities) and reset
   `local_requested` so re-entry re-fetches. Combined with the existing
   `states_suppressed` logic this restores state outlines automatically.
5. **Diagnose before optimizing further.** Brazil lag may be patch/dedupe/label
   cost, not download. Use the new per-layer timings plus the opt-in profiler
   (`AGENTS.md`: bounded background recorder, no UI-path timing logs) and put
   concrete numbers in the reply. If clip+replace does not fix it, profile
   `dedupe_city_labels` and the egui shape rebuild (`build_parts` runs per
   frame; consider caching the border shapes per geometry+view identity).

## 5. Workstream D — the vector rebuild (big; only after A–C are visually approved)

Follow `docs/features/VECTOR_MAP_DATA.md` migration steps 2–5 in order; every
step must keep the fixture comparisons green and the crate contract intact.

1. Introduce seams behind the current API: `LayerId`, `LayerSpec`, `TileKey`,
   `LayerTile`, `MapSnapshot`, `CountryIndex` in `globecraft-data`; identical
   output through the existing `Patch`/`BoundaryData` path.
2. Move source knowledge out of `load.rs` (693 lines — over the 400-line review
   signal) into per-source adapters; the national → geoBoundaries → OSM chain
   becomes adapter metadata, not control flow.
3. Layer registry: the app's per-layer booleans (`show_city_boundaries` etc.)
   become a data-driven layer list; settings/UI follow the existing
   localization rules (en + pt-BR `.ftl` IDs named by feature).
4. Per-tile refresh + cross-fade for detailed layers, using the upstream
   Overture `divisions` PMTiles (measured: 13–43 KB/tile, HTTP 206 range
   reads). A minimal PMTiles v3 reader was prototyped in the old thread's temp
   storage — recover it if present, otherwise rewrite it small and test it
   against a captured tile payload (tests must not hit the network). Vector
   tiles get their own byte-bounded cache, separate from the 256 MiB imagery
   budget.
5. PolarCap layer stays the interim pole answer; a dedicated polar projection
   (GIBS epsg3413/3031 tiles, DATA_SOURCES 2.1) is the follow-up the user
   wants.

## 6. Workstream E — parked until D lands

Terrarium terrain (1.1), GIBS layers (2.1) and the other rows in
`docs/DATA_SOURCES.md` "Suggested build order". They become registry entries
after step D3; building them earlier re-creates the one-off wiring the rebuild
exists to delete. This matches the user's "keep self-hosting out, use the given
resources".

## 7. Housekeeping (small, do opportunistically)

- Replace `docs/DATA_SOURCES.md` with the newer root `DATA_SOURCES.md` (it
  self-describes as belonging in `docs/`), move `data_sources_triage.csv`
  alongside it, delete the root copies. Ask before committing.
- `national.rs::uses_viewport` now covers FRA/DEU/ESP/USA/CAN. Canada does
  accept an ArcGIS envelope (verified 2026-10-21), so the earlier "no bbox"
  note was wrong. geoBoundaries still has no bbox and gets the §4.3 clip
  treatment (open).

## 8. Rules for whoever executes this (AGENTS.md essentials)

- Verification before every claim, from the repo root:
  `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`, `cargo build --workspace --locked`,
  plus the desktop smoke checklist for anything interactive. Never claim a GUI
  check passed unless it ran.
- Preserve the fixture snapshots; regenerate only with a written justification
  appended to `crates/globecraft-render/tests/fixtures/README.md`.
- Tests must not touch public map services, user settings or caches.
- New UI strings: same semantic ID and variables in
  `crates/globecraft-core/locales/en.ftl` and `pt-BR.ftl`. Error/diagnostic
  strings stay English.
- Comments in English. Name constants with units
  (`_KM`, `_PX`, `_SECS`). No new dependency without a demonstrated need; no
  unsafe; keep `Cargo.lock` tracked; workspace dependency versions.
- Perf claims need workload-specific evidence — reuse the "in N ms" boundary
  logs and the bounded profiler; report numbers, not adjectives.
- Commit/push/PR only on explicit request; stage explicit file groups; never
  commit `target/`, local settings or unrelated user work.
