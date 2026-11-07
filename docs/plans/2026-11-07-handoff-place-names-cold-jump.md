# Handoff: place names after a cold jump (roadmap 13.24 follow-up)

**Status:** complete and verified 2026-11-07. No commit or push was made.

This is the operational handoff for the work described in
`docs/plans/2026-11-07-place-names-cold-jump.md`. That document holds the full
implementation rationale, behavior changes and critique; this one records what a
successor needs to know to pick the work up safely.

## Repository context

- Work was done in `/Users/julienfoss/Downloads/EarthCraft/_rewrite/GlobeCraft`.
- That directory has no `.git` of its own. It sits untracked inside the outer
  `/Users/julienfoss/Downloads/EarthCraft` repository, which also has unrelated
  modified files. Do not stage or commit the outer repository's changes as part of
  this work.
- No commit or push was requested or made. If a commit is wanted later, the rewrite
  tree must first be placed under version control or explicitly staged by the owner.
- All generated screenshot and isolated-cache folders created for this work were
  deleted from `target/`. The TomTom test key was never written to the repository,
  settings or documentation.

## The problem

Roadmap 13.24 gave all map names one screen-space label space. Places of interest
(POIs) already kept their room across zoom steps so their names did not flicker, but
that memory made them outrank place names. After a cold jump straight to street level,
the faster POI layer could be placed before the slower place-name tiles. The POI names
then kept city and district names hidden until the view changed.

The fix gives place names the same kind of memory and reserves their screen room before
the shown POIs place their names. The priority order of 13.24 is preserved; only the
cold-start ordering changed.

## What was implemented

### Place-name memory

New file: `apps/globecraft/src/app/overlays/place_memory.rs`.

- `PlaceNameKey` = lowercased English name + `LabelKind`. English is the stable identity
  across bundled, border-tile and close-up place sets and across both UI languages.
  `LabelKind` now derives `Debug`, `Eq` and `Hash`.
- `PlaceNameMemory::finish` records the names drawn by a build.
- A name that leaves the screen or data is kept for `FORGET_AFTER = 2 s`.
- `tier` puts names drawn by the last pass before names remembered from earlier passes.
  Memory only reorders within a `LabelKind`; the existing country → state → city order
  and the projection's rank tie-break are unchanged.
- `revision` lets the reservation cache notice a changed memory.
- `is_empty` skips reservation work before any name has been shown.
- Unit tests cover tiers, 2 s forgetting, and matching across datasets/translations.

### Reservation before shown POIs

- `apps/globecraft/src/app/overlays/projected.rs`
  - `BuiltOverlay` now carries `drawn: Vec<PlaceNameKey>`.
  - `build_prepared` feeds those keys to the memory.
  - `remembered_rects` measures the remembered names for the current view against the
    user's markers only, returning `PlacedLabels`.
  - An `include_new` flag admits names that were never drawn to the reservation.
- `apps/globecraft/src/app/overlays/cache.rs`
  - `OverlayCache` owns the `PlaceNameMemory`.
  - `remembered_placed` caches the reservation by prepared geometry, style, user-marker
    label space, memory revision and `include_new`.
  - The reservation is cleared on overlay invalidation, but the place-name memory
    survives font-layout invalidation (it stores only identity and time).
- `apps/globecraft/src/app/overlays.rs`
  - `OverlayFrame` bundles the view and style for one frame.
  - `reserve_place_names` replaces the earlier narrower
    `reserve_remembered_place_names`.
- `apps/globecraft/src/app/globe.rs`
  - One `OverlayFrame` is built per frame.
  - Place names are reserved before `PoiShapes::prepare_shown`.
  - New names are included only when `poi_shapes.has_shown_name()` is true and
    `!poi_shapes.has_settled_name()`.
  - The drawing-order comment was updated.

### Ten-second settle rule

- `apps/globecraft/src/app/live_layers/poi_memory.rs`
  - `SETTLED_AFTER = 10 s`.
  - `has_shown_name` reports whether the latest POI pass drew any name or marker.
  - `has_settled_name` reports whether a name drawn by the latest pass has been on
    screen for 10 s.
- `apps/globecraft/src/app/live_layers/poi.rs`
  - Exposes both queries to the app.

When POI names are on screen but none has settled (the first view after a jump), every
place name reserves its room, including names never drawn. Once a POI name has held its
place for 10 s, the old rule returns: the settled POI keeps its room from names arriving
later. When no POI name is on screen, the extra all-name pass is skipped.

### Drawing order

The order is unchanged except for the new reservation step:

1. markers of the user's data;
2. room of place names already on screen (new: before shown POIs);
3. names of POIs already on screen;
4. map place names and grid labels;
5. names of the user's points, street names, airports, then new POIs.

## Files changed

- `crates/globecraft-data/src/boundaries/mod.rs` — `LabelKind` derives `Debug`, `Eq`,
  `Hash`.
- `apps/globecraft/src/app/overlays/place_memory.rs` (new) — key, memory, tiers,
  forgetting, tests.
- `apps/globecraft/src/app/overlays/projected.rs` — memory-aware ordering,
  `BuiltOverlay::drawn`, `remembered_rects`, `include_new`.
- `apps/globecraft/src/app/overlays/cache.rs` — place memory, cached reservation,
  invalidation and key.
- `apps/globecraft/src/app/overlays/cache/tests.rs` — deterministic cold-jump test,
  `draw_ready_with` helper.
- `apps/globecraft/src/app/overlays.rs` — `OverlayFrame`, `reserve_place_names`.
- `apps/globecraft/src/app/globe.rs` — reservation wiring, `has_shown_name` guard.
- `apps/globecraft/src/app/live_layers/poi.rs` — exposed `has_shown_name` and
  `has_settled_name`.
- `apps/globecraft/src/app/live_layers/poi_memory.rs` — settle policy and test.
- `docs/ROADMAP.md` — 13.24 cold-jump note; queue item marked done.
- `docs/releases/v0.4.0.md` — fix note; removed the known limit.
- `docs/plans/2026-11-07-place-names-cold-jump.md` — implementation, verification and
  critique record.
- `docs/plans/2026-11-05-network-policy.md`, `docs/NETWORK_POLICY.md` — completed the
  unrelated TomTom authenticated-tile verification from 10.6.

## Verification

Run from the rewrite root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python3 -m unittest discover -s scripts -p 'test_profile_summary.py' -v
```

Final results after the last `has_shown_name` change:

- formatting, clippy and workspace build passed;
- workspace tests: **572 passed, 61 ignored**;
- Python profile-summary tests: **2/2 passed**.

Deterministic regression tests:

- `place_names_reserve_their_room_before_names_that_loaded_first`
  (`apps/globecraft/src/app/overlays/cache/tests.rs`) covers a cold cache, the
  `include_new` reservation, a remembered name from before a jump, and the name coming
  back once the POI yields.
- `names_keep_their_tier_and_are_forgotten_later`,
  `names_are_matched_across_datasets_and_catalogs` (`place_memory.rs`).
- `only_a_name_shown_for_a_while_is_settled` (`live_layers/poi_memory.rs`).

Live/offscreen checks:

- `poi_zoom_stability`, 30 zoom steps from 6 km to 1.8 km over Berlin Mitte:
  **3,033 names drawn, 0 vanished, 0 side changes**. Rerun after the final guard with
  an isolated `XDG_CACHE_HOME` and `GLOBECRAFT_SHOTS`.
- A deliberately broader variant that reserved every place name on every frame
  regressed to 4 vanished names and 1 side change; that is why the all-name reservation
  is limited to the unsettled-POI window.
- Cold jump to 3 km over Munich with POIs, roads, railways, parks and waters on:
  captured at 2, 4, 8 and 30 seconds. The 4 s frame shows “Munich”, “Old Town”,
  “Hackenviertel” and “Angerviertel” among POI names.
- The full interactive desktop smoke checklist was **not** run. The visual checks above
  are the existing offscreen screenshot harnesses, not a manual GUI session.

Reproduction commands:

```sh
mkdir -p target/shots-poi target/cache-poi
XDG_CACHE_HOME=$PWD/target/cache-poi \
GLOBECRAFT_SHOTS=$PWD/target/shots-poi \
cargo test -p globecraft --locked poi_zoom_stability -- --ignored --nocapture

mkdir -p target/shots-cold target/cache-cold
XDG_CACHE_HOME=$PWD/target/cache-cold \
GLOBECRAFT_SHOTS=$PWD/target/shots-cold \
GLOBECRAFT_SHOT_ALT_KM=3 \
GLOBECRAFT_SHOT_LAYERS=poi,roads,railways,parks,waters \
cargo test -p globecraft --locked map_data_close_up_screenshot -- --ignored --nocapture
```

These create network-backed screenshots. Delete the generated `target/shots-*` and
`target/cache-*` folders afterwards; they must not be committed.

## TomTom follow-up from roadmap 10.6

The authenticated TomTom Orbis traffic tile check is complete. Using the app's exact
request shape (`https://api.tomtom.com/maps/orbis/traffic/flow/raster/tile/...`,
`TomTom-Api-Key` header, `GlobeCraft/1.0 (traffic; TomTom)` agent) and a user-supplied
test key, the response was HTTP 200, `image/png`, 256×256, 65,316 bytes. The key was
used only for that live request and was not stored. The network-policy documents now
record the result.

## Known limitations and suggested follow-ups

These are documented in detail in the critique section of
`docs/plans/2026-11-07-place-names-cold-jump.md`. In priority order:

1. **The 10 s rule is a proxy for a view generation.** On a slow network, a place-name
   tile can arrive after 10 s and still lose to a settled POI. Prefer tagging names and
   POIs with the view/generation that produced them instead of relying on elapsed time.
2. **Grid labels are not reserved.** A grid coordinate label can still be hidden by a
   POI name on a cold jump. Reserving grid labels with place names is a small follow-up
   but changes `GridLabels` priority.
3. **Name + kind is not a unique place identity.** Two places with the same English name
   and kind share one memory entry. A stable OSM id or rounded coordinates would fix it.
4. **Reservations can be conservative.** A remembered low-priority name can reserve room
   in the dry pass and then be hidden in the full pass, blocking a POI for a frame or
   two. Publishing the previous frame's actual rectangles would avoid this.
5. **The reservation repeats candidate sorting and font lookups.** Measure a dense
   street-level workload before optimizing; reuse the projection's ordered candidates or
   cache the reservation longer if needed.
6. **The deterministic test uses a synthetic rectangle.** It does not exercise
   `PoiShapes::prepare_shown` or the real `globe.rs` wiring. A synthetic `LiveGeometry`
   integration test would catch future wiring mistakes.
7. **The 2 s and 10 s values are unmeasured policy.** They are named constants with
   comments; replace the guesses if real tile-arrival timing is measured.
8. **Boundary exchanges only remember the incoming set.** A name that appears only in
   the outgoing cross-fade set can lose its room during the 500 ms exchange.
9. **Memory is not persisted.** A restart starts empty; this is intentional.

## State at handoff

- 13.24 cold-jump follow-up: complete.
- Roadmap queue item 13.24: marked done.
- v0.4.0 release notes: fixed note added; cold-jump known limit removed.
- Network policy / TomTom 10.6: complete and documented.
- No commit, no push, no persisted key, no generated artifacts left behind.
