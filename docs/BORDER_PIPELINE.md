# How country borders are drawn

A walk through the code, from download to pixels, as of `3076ff0` (the border code
before the reverted disputed-border style). Distances in degrees are latitude degrees
(1° ≈ 111 km); longitude is scaled by cos(latitude) everywhere.

## 1. Two kinds of source

| Layer | Source | Where | When |
| --- | --- | --- | --- |
| Coarse country lines | Natural Earth `admin_0_boundary_lines_land` 1:110m (bundled), 1:50m, 1:10m | `load.rs::load_countries`, `sources.rs` | Always. 1:110m above 10,000 km, 1:50m to 1,500 km, 1:10m below (15 % hysteresis on the way up, `lod.rs`) |
| Country outlines (lookup only) | Natural Earth `admin_0_countries`, same level | `load.rs::load_polygon_layer` | Never drawn: they answer "which country is this point in" and "is this land" |
| Municipal outlines | One file per country: national services for DEU, FRA, ESP, USA, CAN (grid cells) and BRA (IBGE per state); geoBoundaries gbOpen ADM1–ADM4 for ~190 countries (whole country, simplified); OSM Overpass `admin_level` 8/9 in 1° cells where neither exists | `municipal.rs::units_at`, `national.rs`, `adm.rs`, `world.rs` | Below 643 km (prefetch); drawn below 402 km |

Each Natural Earth border line carries `ADM0_A3_L` / `ADM0_A3_R`, so the app knows which
two countries it separates (`border_pair`). Natural Earth's disputed map units are
appended to the same list without a pair.

The basic problem: municipal data has **no border lines**, only closed polygons per
country, and the two countries' files are different surveys that disagree by 0–10 km.
Everything below decides which polygon edge is "the border" and who draws it.

## 2. Streaming municipal data (`municipal.rs`)

1. `MunicipalStreamer::update` runs every frame from the app. Below 643 km it probes a
   grid every 0.5° over a radius of altitude × 0.8 (× 1.25 margin, max 6°), asks
   Natural Earth's outlines which country each probe is in, and maps it to a *unit*
   (cell, Brazilian state, whole country, OSM cell). At most 3 loads run at once, 1 of
   them Overpass; failures retry after 90 s, and a failed geoBoundaries country switches
   to OSM cells.
2. Each loaded unit is classified immediately (`UnitData::new` → `topology::classify_known`):
   every segment of every ring is compared with the segments of its own rings and of
   already stored neighbouring rings (0.02° spatial grid). A segment is *shared* if
   another ring has a segment within **0.012°** (~1.3 km), running within ~35° of the
   same direction, overlapping it. Classes:
   - `OUTER` 0: nothing shared, an outline (coast, country border, edge of loaded data);
   - `INTERIOR` 1: shared with a municipality of the same state;
   - `MAJOR` 2: shared with another state of the same country;
   - `COUNTRY` 4: shared with a municipality of **another country**;
   - `DROP` 3: shared, and the neighbour (smaller ring id) draws it.
   Inside one exact file the tolerance is 0.0005°, so narrow notches aren't mistaken for
   shared edges. When a neighbour loads later, its `OUTER` segments are upgraded
   (`upgrade_outer`). The store holds up to 4 million vertices; the farthest units go first.
3. A publisher task (`publish`) wakes on changes, waits 250 ms, snapshots the store and
   calls `assemble` on the feature pool.

## 3. Assembly and who draws a border (`assemble` → `borders::resolve`)

`assemble` merges copies of the same ring (same `ring_id`; `DROP` wins, otherwise the
higher class) for everything intersecting the area, then runs `resolve` over all
rings. Candidates are only `OUTER` and `COUNTRY` segments, i.e. each country's outline.

**First pass, per outline segment of country `own`:**
1. `CountryBorders::across` finds the Natural Earth border line of `own` nearest to the
   segment's middle and both ends: within **0.05°** (5.5 km), or **0.15°** (16.7 km) when
   the segment is `COUNTRY`-class (both countries' outlines meet there). All three points
   must name the same neighbour, and must not lie past a free end of the Natural Earth
   line (where it reaches the sea).
2. An unconfirmed (`OUTER`) segment must have land about 1 km to **both** sides (Natural
   Earth country outlines, `Land::on_both_sides`), so a coast near a border is not taken.
3. Who draws: a side whose neighbour isn't loaded draws; a national source (DEU, FRA,
   ESP, USA, CAN, BRA) beats geoBoundaries/OSM; otherwise the alphabetically lower ISO
   code draws.
4. Drawing side → class `COUNTRY` with the pair, unless the ring is **contested** (an
   interior point of it lies inside both countries' files, `Contested::claimed_by`), in
   which case it stays a municipal line. Waiting side → queued for the second pass. An
   outline that matched no border (coast, area edge) and was `COUNTRY`-class becomes
   `INTERIOR`.
5. `bridge`: open stretches of ≤ **2 km** between two drawn stretches of the same border
   are drawn too.
6. All drawn stretches are joined into runs and indexed (`drawn`).

**Second pass, the waiting side's segments:**
- drawn line within 0.05° (5.5 km) → `DROP` (hidden);
- drawn line within 0.15° (16.7 km) → `INTERIOR` (plain municipal line, so its
  municipalities stay closed);
- no land on both sides (if unconfirmed), or the drawing country's own land isn't found
  1, 2.5 or 5 km across, or contested → `INTERIOR`;
- otherwise the drawing side has a hole in its file: this side draws the border (`COUNTRY`).

## 4. Splitting into layers (`topology::split`)

Each ring is cut into runs by class: `COUNTRY` runs (with their pair) →
`world_cities_country`; `MAJOR` → `world_cities_major`; `INTERIOR`/`OUTER` →
`world_cities`; `DROP` → nothing. Two indexes are built for the renderer:
- `MajorCells` of the state runs (cover 0.03°);
- `CountryCells`: per pair of countries a cover of **0.1°** (11 km), plus an index of all
  country runs with 0.03° for Natural Earth lines without a pair.
A run end that continues another run within 0.024° counts as joined; a free end covers
only up to the perpendicular through its last point.

## 5. Drawing (`globecraft-render/src/overlays/mod.rs::project_borders`)

Order: states, municipal, then Natural Earth country lines on top.
- Natural Earth country line `i`: below 402 km, `handover::uncovered_runs` walks it every
  0.03°, asks `CountryCells::for_pair(pair[i]).covers(point)` and bisects each change to
  ~100 m. Covered stretches are not drawn. An uncovered piece < 30 km between two covered
  ones is dropped only if every point (sampled every 0.02°) lies within 0.15° of a
  municipal border (`bridged`).
- `world_cities_country` is drawn as `CityCountry` in the country style.
- Button rules: Countries off + States on → Natural Earth country lines in state style
  (cut the same way) and municipal country runs as `CityMajor`; both off → municipal
  country runs as plain municipal lines. State lines are cut by `MajorCells` the same way.
- The app (`apps/globecraft/src/app/overlays/projected.rs`) tessellates the segments and
  applies the per-kind fades; the handover itself is a cut, never a cross-fade.

## 6. Where it goes wrong, and why

Every threshold above (5.5 km, 11 km, 16.7 km, 2 km bridges, 30 km detours, land probes,
contested tests) exists to reconcile **two independent surveys of one border** using a
**third, coarse line** (Natural Earth) as the referee. Wherever the three disagree by
more than a threshold, something shows: doubled lines, gaps, stray stubs, municipal
outlines taken for borders, or coarse pieces left over. The global audit measures this
(96.9 % replaced, ~1,400 km doubled, ~2,100 breaks on 199,000 km).

A source with **one shared line per border** removes the reconciliation step entirely:
Overture Maps' `division_boundary` (OpenStreetMap + geoBoundaries, ODbL, monthly) holds
one line per pair of neighbouring countries/regions/counties, with `is_land` and
`is_disputed` flags. See ROADMAP 13.19.
