# Place names after a cold jump (roadmap 13.24 follow-up)

**Status:** implemented and verified 2026-11-07; no commit made.

## Why this item

Roadmap 13.24 gave every map name one screen-space label space. To stop places of
interest from flashing and jumping while zooming, the names already on screen were
placed before place names. That fixed the reported zoom flicker but left a cold-start
hole: after a jump straight to street level, the places of interest can be projected
and placed before the slower place-name tiles. Their remembered names then keep the
city or district name hidden for as long as the place of interest stays in view.

The item was recorded as an open limit in the 13.24 note and in the v0.4.0 release
notes. A place name needs the same rule the places of interest already have: once it
has been seen, it keeps its room while the view moves and finer tiles replace coarser
ones.

## What shipped

### Place-name memory

`apps/globecraft/src/app/overlays/place_memory.rs` is a small memory of the place names
drawn by the last placement pass and for two seconds after they leave:

- `PlaceNameKey` is the lowercased English name plus `LabelKind`. The English name
  matches the bundled, border-tile and close-up place sets and both UI languages; the
  kind keeps a city named like a state apart. `LabelKind` gained `Debug`, `Eq` and
  `Hash` for the map.
- `PlaceNameMemory::finish` records every name a build drew. Names not drawn stay for
  `FORGET_AFTER` (2 s), so a tile swap or a frame without one does not lose its room.
- `tier` puts names drawn by the last pass before names remembered from earlier passes.
  The projection keeps its country → state → city order; memory only reorders names
  within one kind. A returning name therefore comes before a new name of the same kind,
  and the projection's rank order still breaks ties.

`build_prepared` now returns the keys it drew, and the overlay cache feeds them to the
memory after each rebuild. The memory is kept across font invalidation: it stores no
font geometry, only identity and time, while the cached reservation rectangles are
discarded and measured again.

### Reserving place names before places of interest

`OverlayCache::remembered_placed` measures the place names already remembered for the
displayed view and returns their screen rectangles as `PlacedLabels`, before
`PoiShapes::prepare_shown` places its names. The reservation is measured against the
user's markers only, never against the places of interest, so a lower layer that
arrived first cannot keep a remembered name hidden. The cache keys the result by the
prepared geometry, style, the room taken by user markers, the memory revision and
whether new names are included.

The first view after a jump also reserves place names that were never drawn before.
`PoiMemory` now answers whether it has drawn any name at all and whether one has held
its place for `SETTLED_AFTER` (10 s). When places of interest are on screen but none
has settled, the place-name reservation includes every name on the view. A district
name that arrives after the places of interest can therefore claim its room once; after
it is drawn it is remembered and keeps that room. When a name has been on screen for ten
seconds, the old rule applies again: a settled place of interest keeps its room from
names that arrive later. When no place-of-interest name is on screen, no new-name
reservation is made, so the work disappears with the layer.

### Order in the app

`App::globe_view` now builds one `OverlayFrame` (view and style) for the frame, reserves
place names with it, places the shown places of interest around that reservation, then
prepares the border and place-name overlay and the layers below it. The previous naming
order is unchanged except for the reservation:

1. markers of the user's data;
2. room of place names already on screen (new: before shown places of interest);
3. names of places of interest already on screen;
4. map place names and grid labels;
5. names of the user's points, street names, airports, then new places of interest.

`reserve_place_names` replaces the earlier, narrower
`reserve_remembered_place_names` and is only called when live geometry exists and the
place-label layer is visible.

## Deliberate behavior changes

1. A place name once drawn now yields less often to a place of interest. If the two
   overlap, the place name keeps its room and the place of interest may show only its
   marker or nothing. This is the intended priority of 13.24, applied after the name
   has been seen.
2. During the first ten seconds of a new view, place names outrank places of interest
   even when the names were never drawn. After that, a settled place of interest keeps
   its room.
3. Place-name memory is kept when the font layout changes; the places-of-interest
   memory is still cleared there, so the first frame after a font change can reserve
   every place name once.
4. No persisted file, setting, service address, timeout or key changes. The new
   behavior is in-process only and starts empty on every launch.

## Verification

Ran from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python3 -m unittest discover -s scripts -p 'test_profile_summary.py' -v
```

Formatting, clippy and the workspace build passed. The workspace test run passed 572
tests with 61 ignored manual/network tests; the Python summary tests passed 2 of 2.
These are the final results after the `has_shown_name` fast path (below); the same
commands passed before it as well.

New deterministic tests:

- `place_names_reserve_their_room_before_names_that_loaded_first` in
  `app/overlays/cache/tests.rs` builds a view whose middle is taken by a place of
  interest. A cold cache leaves the place name hidden; `include_new` reserves its room.
  A cache that drew the same name before the jump reserves it again from memory, and
  the name comes back once the place of interest yields.
- `names_keep_their_tier_and_are_forgotten_later` and
  `names_are_matched_across_datasets_and_catalogs` in `place_memory.rs` cover the
  two-pass tiers, the 2 s forgetting and matching across ranks, tile sources and
  translations.
- `only_a_name_shown_for_a_while_is_settled` in `live_layers/poi_memory.rs` covers the
  10 s rule, the `has_shown_name` fast path and the case where the latest POI pass
  drew nothing.

Live checks:

- `poi_zoom_stability` (30 steps, 6 → 1.8 km over Berlin Mitte, places of interest and
  place names on) finished with **0 vanished names and 0 side changes** over 3,033 name
  draws, both with the full reservation and again after the final `has_shown_name`
  guard. This is the same result as the pre-fix memory baseline (0 / 0), so the
  reservation does not reintroduce the zoom flicker the memory was added for.
- A deliberately broader variant that reserved every place name on every frame was
  measured first: 4 names vanished and 1 changed side. That regression is why the
  never-drawn names are admitted only while no place of interest has settled.
- A manual cold jump to 3 km over Munich with places of interest, roads, railways,
  parks and waters on (`map_data_close_up_screenshot`) was captured at 2, 4, 8 and 30
  seconds, before and after the final guard. The 2 s frame is still loading; the 4 s
  frame already shows the city name “Munich” and district names such as “Old Town”,
  “Hackenviertel” and “Angerviertel” among the place-of-interest names. The capture
  folders were temporary and were deleted after inspection.
- The TomTom authenticated-tile row from 10.6 was also completed on 2026-11-07 with
  the user-supplied test key: the app's exact URL, header and agent returned a 200
  `image/png`, 256×256 tile. The key was used only for that request and was not stored.

## Review notes and critique

These are observations from this item, not all fixed here.

1. **The ten-second rule is a proxy for a view generation.** `SETTLED_AFTER` infers
   “this view just appeared” from elapsed time. On a slow network a place-name tile can
   arrive more than ten seconds after the places of interest, and a district name that
   was never drawn can still be hidden. The robust version would tag place names and
   places of interest with the view/generation that produced them and let names from
   the same generation compete by the documented priority. That requires the data
   layer to expose its arrival batch and the POI pass to expose its first-shown
   generation instead of relying on a clock.

2. **Priority is now timing-dependent.** The same pair of overlapping names can resolve
   differently depending on how long the place of interest has been on screen. That is
   the price of keeping both the documented priority and the zoom-stability behavior.
   If users report names appearing only after a pause, the rule should move to an
   event-based generation comparison rather than a longer timeout.

3. **Name plus kind is not a globally unique place identity.** Two different places
   with the same English name and kind share one memory entry, so a remembered “Mitte”
   can reserve room for a different “Mitte” elsewhere in the view. Rounded coordinates
   (the POI memory uses about 100 m) or a stable OpenStreetMap id would separate them.
   The data model currently has no stable place id; adding one is the cleaner fix, but
   it must survive the bundled, border and close-up sets. The 2 s forget window limits
   the damage of a collision.

4. **Reservations can be conservative.** The reservation is measured without the rest
   of the place names. A remembered low-priority name can fit in that pass, reserve its
   room, and then be hidden in the full pass by a higher-priority name that arrived in
   the same frame. Its rectangle still blocks a place of interest for a frame or two
   even though no name is drawn there. A two-pass “reserve only what the full pass
   would draw” scheme is circular with the current API; an iterative dry run or
   publishing the previous frame's actual rectangles would avoid the waste.

5. **The reservation repeats candidate sorting and font lookups.** Every build with
   live geometry sorts all place-name candidates for the reservation and again for the
   full pass. Galleys are cached, the empty-memory fast path avoids the work before any
   name has been shown, and the all-name pass now runs only while a place-of-interest
   name is on screen but none has settled. There is still no workload measurement for a
   dense street level with many remembered names. If this shows up in frame timing, the
   reservation should reuse the projection's ordered candidate list or cache the
   reservation for longer than a memory revision.

6. **Grid labels are still outside the reservation.** Grid coordinate labels are placed
   after place names but before the places of interest; a grid label can still be kept
   hidden by a place-of-interest name on a cold jump. It is the same structural issue
   as the district name, just with a layer that has no memory. Reserving grid labels
   with the place names would be a small follow-up, but it changes the priority of the
   `GridLabels` layer and was not part of this item.

7. **The deterministic test uses a synthetic rectangle.** It exercises the cache's
   reservation and memory, but not `PoiShapes::prepare_shown` or the real app ordering.
   The live zoom test covers the no-regression side, and the manual cold-jump frames
   cover the visual side, but a full integration test with a synthetic `LiveGeometry`
   and the app's render path would catch a future wiring mistake in `globe.rs`.

8. **The two-second and ten-second values are unmeasured policy.** `FORGET_AFTER` is
   copied from the POI memory precedent; `SETTLED_AFTER` was chosen to cover a slow
   tile load without letting new names displace names the user has been reading. Neither
   has telemetry or a timing benchmark behind it. They are named constants with
   comments, which is the current repository convention, but a future investigation of
   actual tile arrival times could replace the guess.

9. **Boundary exchanges only remember the incoming set.** During the 500 ms cross-fade,
   outgoing names are still visible but not recorded as drawn. They are usually still
   remembered from earlier builds, but a name that first appears only in the outgoing
   set can lose its room during the exchange. The exchange is short and the incoming
   data is the future view, so this is a low-impact edge case.

10. **Memory is not persisted.** A restart starts with no memory, so the ten-second
    rule is the only protection for the first cold jump. That is intentional: identity
    and timestamps are ephemeral, and writing them would add a state file with no
    user-visible benefit.

## Files changed

- `crates/globecraft-data/src/boundaries/mod.rs`: `LabelKind` derives `Debug`, `Eq`,
  `Hash`.
- `apps/globecraft/src/app/overlays/place_memory.rs` (new): place-name key, tiers,
  forgetting, tests.
- `apps/globecraft/src/app/overlays/projected.rs`: ordered candidates by kind and
  memory tier; `BuiltOverlay::drawn`; `remembered_rects`; `include_new` rescue pass.
- `apps/globecraft/src/app/overlays/cache.rs`: place memory, cached reservation,
  invalidation and the `include_new` key.
- `apps/globecraft/src/app/overlays/cache/tests.rs`: deterministic cold-jump
  reservation test and `draw_ready_with`.
- `apps/globecraft/src/app/overlays.rs`: `OverlayFrame`, `reserve_place_names`.
- `apps/globecraft/src/app/globe.rs`: one frame style; reservation before shown places
  of interest; updated drawing-order comment.
- `apps/globecraft/src/app/live_layers/poi.rs`: expose whether any POI name is on
  screen and whether a shown POI name has settled.
- `apps/globecraft/src/app/live_layers/poi_memory.rs`: `has_shown_name`,
  `has_settled_name` and their test.
- `docs/ROADMAP.md`: 13.24 cold-jump note and working queue.
- `docs/releases/v0.4.0.md`: fix note; removed the known limit.
- `docs/plans/2026-11-05-network-policy.md`, `docs/NETWORK_POLICY.md`: completed
  TomTom authenticated-tile verification (unrelated 10.6 follow-up).
- This document.
