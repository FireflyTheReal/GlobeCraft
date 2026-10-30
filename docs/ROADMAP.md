# GlobeCraft roadmap

Last revised 2026-10-22: GlobeCraft rename, second batch of user requests (section 13,
including 13.12 instant border handover),
size and efficiency review (section 14). Earlier context:
`docs/plans/2026-10-20-boundaries-and-vector-rebuild.md`. Data source IDs refer to
the master table in `docs/Data_Sources.pdf` (for example `IMG-03`). The
`DATA_SOURCES.md` file uses the older numbering (`16.3`), given in brackets where it
helps.

**Order of work:** perfect the tile processing and coordinate streaming first (section 1),
then open the door to every compatible data source through Add Data (section 2). The data
sources in the master table then become presets rather than new code.

This is a plan, not a list of shipped features. Every item has a size:

| Size | Meaning |
| --- | --- |
| **S** | Under a day |
| **M** | A few days |
| **L** | One to three weeks |
| **XL** | A milestone that needs its own plan doc |

## Ground rules for every item

- **Performance is a feature.** Any change on the frame, tile or overlay path ships with
  before/after numbers from the existing protocol:
  - idle and motion measured separately, in ms per megapixel;
  - interface frame CPU time;
  - `docs/PERFORMANCE_BASELINE.md`.

  A regression needs an explicit reason in the PR.
- **Upstream first.** Consume public endpoints; pre-bake only where it is trivial; never
  self-host big datasets. Keys come from the user and are never shipped. Show the
  licence and attribution in the UI next to the layer (the Layers pane already has
  credit links).
- **Every action is a command.** New features add an `globecraft_core::commands` entry
  first, so that all of these get it for free:
  - ribbon, menus and shortcuts;
  - Command Search;
  - the AI assistant.
- **Verify on the desktop.** For UI, input and render changes, compiler success and unit
  tests are not enough. The `CONTRIBUTING.md` smoke checklist runs on a real Mac
  before merge.

---

## 0. Right now: stabilise what just landed

| # | Item | Size | Why / notes |
| --- | --- | --- | --- |
| 0.1 | **Desktop smoke pass of the rewrite on macOS**: ribbon, panes, native menus, shortcuts, assistant with a real key, Command Search, Places drag-and-drop, screenshot export | S | Only the automated tests and Linux screenshots have run so far |
| 0.2 | **Re-test the "aircraft freeze"** with Flights on while imagery streams | S | Two fixes are merged; the second is listed in 0.3. See 0.2 below this table |
| 0.3 | Live feeds (aircraft, vessels, satellites, radar, clouds, roads, traffic) start hidden at every launch | S | Done. The rewrite had started remembering them, so the app contacted tracking services on startup |
| 0.4 | Run the frame-time protocol on the Mac for the whole merged batch: LOD, minification filter, borders, terrain, mesh-cached borders | S | Handoff queue item 3. The Linux numbers are in `PERFORMANCE_BASELINE.md` |
| 0.5 | Move the root `DATA_SOURCES.md` and `data_sources_triage.csv` into `docs/` (the new one replaces the old `docs/DATA_SOURCES.md`), with the user's OK | S | Plan housekeeping; the two copies have drifted **Done 2026-10-25** (user's OK): the newer root copy replaces `docs/DATA_SOURCES.md`, with `docs/data_sources_triage.csv` beside it; renamed to GlobeCraft |
| 0.6 | Replace the emoji glyphs still used as icons with the vector icon set, or drop them where an icon is already drawn | S–M | Affects "💾 Save", "📂 Open KML/KMZ…", the globe context menu, the Explorer worlds and "⏳" statuses. The localisation fixture pins the old strings, so update it deliberately **Done 2026-10-25:** context menu, Places (Save, Save as, Open, At center), the Explorer worlds and the folder Browse buttons draw vector icons (`icons::button`); loading notes show a spinner; the emoji are gone from both locale files. The fixture test strips a leading emoji from the original wording. The easter egg keeps its decorative symbols |
| 0.7 | Add a command for the state-border altitude (`layer.stateAltitude`) so the assistant and Command Search can change it | S | The setting exists only as a pane control **Done 2026-10-25:** `layer.stateAltitude` (`km`, 0–8,000), enabled while the state layer is on |
| 0.8 | First GlobeCraft release (0.3.0) notes with the "what changed" screenshots, from the new repository `FireflyTheReal/GlobeCraft` | S | `docs/releases/`. English only: the repository has no translated documents **Notes written 2026-10-27** (`docs/releases/v0.3.0.md`); version 0.3.0 |
| 0.9 | Re-shoot the README screenshot (`assets/repository/globecraft-app.png`) with the GlobeCraft name on screen | S | The current image still shows the old title **Done 2026-10-27** (Wayback imagery, country borders, the Layers ribbon) |

**0.2 in detail.** Two changes are merged:

- The render-wake fix from the handoff. Imagery used to stop updating after the camera stopped, because a deferred UI pass consumed the tile generation. Flights add a repaint every second, which made this much more likely.
- Live feeds no longer auto-start (item 0.3).

If the Mac still stalls, record a profiling capture with the bounded recorder before changing code.

## 1. Foundation first: tile processing and coordinate streaming

The guiding idea: make the way GlobeCraft fetches, decodes, schedules and draws tiles and
coordinates excellent **once**, generically. After that, every compatible data source in
the master table is configuration (a preset in Add Data, section 2), not new code. Nothing
in sections 3–5 should add a one-off code path that this section is about to replace.

What the code does today, and why it is not yet the foundation we want:

- `Provider` is a closed enum of five sources with hard-coded URLs. `TileKey` packs the
  provider into **3 bits** (at most 8 sources), and the request queue special-cases
  Satellite.
- Tiles are requested **reactively**: a raster pass finds missing keys, then the cache
  admits them (6 workers, 48 in flight). Nothing is fetched ahead of a fly-to, a drag or
  a zoom.
- When the view moves, queued requests are retired, but downloads already running finish
  even if they left the view.
- Minified sampling runs a per-pixel area filter, which is faster now (the separable
  version) but still costlier than sampling a pre-built half-resolution level.
- Vector data (borders, labels, roads) loads per dataset or per country, not per tile.
  Live objects jump between refreshes instead of moving.

| # | Item | Size | Why / notes |
| --- | --- | --- | --- |
| 1.1 | **Streaming replay benchmark.** Recorded camera paths (fly across the globe, zoom into a city, drag-pan, orbit) against a local fixture tile server that simulates latency and bandwidth per host. It reports time to first complete view, time to sharp (every visible tile at its target level), hole pixels per frame, wasted downloads (fetched but never shown), bytes, and frame times | M | "Perfect" needs a number. This is the yardstick for every item below; run it before and after each one and keep the results in `PERFORMANCE_BASELINE.md` **Done 2026-10-25:** `globecraft-render/src/benchmarks/replay.rs` (zoom into a city, fly across, drag-pan, orbit; latency and shared bandwidth via `GLOBECRAFT_REPLAY_LATENCY_MS` / `GLOBECRAFT_REPLAY_KBPS`; tiles come through `TileCache::with_fixture_source`). Baseline in `PERFORMANCE_BASELINE.md` |
| 1.2 | **Generic `TileSource`** in data: id, kind (raster imagery, elevation, vector), access (XYZ/TMS template, WMTS REST, WMS GetMap by tile bbox, PMTiles, local MBTiles), format, zoom range, 256 or 512 px tiles, projection, attribution and licence, prefetch policy (OSM stays on demand), key reference, optional time dimension. `TileKey` gets a 16-bit source id and a time index; the disk cache keys by source identity | L | Replaces the closed `Provider` enum. Night lights (3.2) is the pilot: the first source added as data, not code **First stage done 2026-10-26:** sources are data in `globecraft-data/src/tiles/sources.json` (id, code, name, kind, basemap, access: XYZ template or Wayback, zoom range, tile size, cache folder, attribution, prefetch policy), read into a registry; `Provider` is now a source id (5 bits in the tile key, 32 sources) with the built-ins as named constants. Each source's first failed download is written to the run log. Still open: user-defined sources (Add Data), WMS/PMTiles/MBTiles access, a time dimension, per-host limits (1.3). |
| 1.3 | **Scheduling and cancellation**: priority by screen distance and level (coarse first), cancel downloads that leave the view, per-host concurrency (HTTP/2 multiplexing vs HTTP/1.1 hosts), per-host backoff honouring 429 and `Retry-After`, conditional revalidation (`ETag`, `Cache-Control`) for disk-cached tiles | M | The bandwidth goes to what the user will see next **Done in part 2026-10-26:** downloads of tiles the view left stop (loads ahead and screenshot tiles continue), per-source limits (`max_parallel`, OSM and OpenTopoMap 2), 429/503 with `Retry-After` pause the source, the cancelled count is logged. Slow link: a zoom transfers 13 % less and is sharp twice as fast (see `PERFORMANCE_BASELINE.md`). Still open: per-host limits across sources, HTTP/2 tuning, conditional revalidation (`ETag`). **Map-data downloads 2026-10-29:** archive opens are cached per URL and a set's parts are probed concurrently; a batch reads up to 24 tiles, merges exactly contiguous ranges (8 MiB cap) and cancels the remaining probes once every tile has a home. Boundary reads have a 120 s whole-request ceiling and a shared six-permit range limit. |
| 1.4 | **Predictive requests**, always below visible work: coarse tiles along a fly-to path and around its destination (the path is known in advance), extrapolation of drag and inertia velocity, and a ring one level coarser around the viewport | M | Arrive with the picture ready instead of watching it fill in **Fly-to part done 2026-10-27:** the destination view's tiles load during the flight; fly across is sharp 0.26 s after it stops instead of 0.55 s (1.01 instead of 1.23 s on the slow link) for under 1 % more data (PERFORMANCE_BASELINE). Coarser levels around the destination gained nothing. Drag/inertia extrapolation and the coarser ring remain open |
| 1.5 | **Tile processing**: build a half-resolution level at decode time so minified sampling is one bilinear tap on it instead of the area filter; native 512 px tiles; WebP/AVIF; decode cost and bytes accounted per source | M | Faster and the same quality; the filter stays for stride above two **Measured 2026-10-27, deferred:** with the area filter replaced by plain bilinear (the best a half-resolution level could reach) the warm 512×384 frame went from 12.2 to 10.8 ms (−11 %); a pre-built level (two taps, trilinear) would save perhaps half of that while costing a third more decoded memory per tile, so fewer tiles stay cached. Not worth it now; revisit with 512 px or WebP sources |
| 1.6 | **Seamless arrival**: a short time-based cross-fade when a finer tile lands (today it pops in), the existing parent fallback kept, consistent colour across levels | S–M | Removes the last visible "loading" artefacts **Done 2026-10-27 in the compositor:** a new frame of the view on screen (tiles arrived, the sharp frame after motion, a layer switched) fades in over the old picture for 250 ms; two textures swap roles, so it costs no extra raster or upload. A per-tile fade in the renderer was rejected: each step would be a full CPU raster, throttled to 8 a second and up to hundreds of ms with relief |
| 1.7 | **Coordinate streaming** for every geographic coordinate: borders, labels, roads, places, live objects. Request vector data by viewport tile and zoom (MVT/PMTiles or bbox APIs); clip and simplify per zoom off the UI thread; keep stable feature ids so updates are diffs, not replacements; project incrementally (only changed tiles) and keep reusing cached meshes, as the borders now do | L–XL | One streaming model instead of per-dataset loaders. It carries the municipal redesign (3.1) and the vector rebuild (3.3) |
| 1.8 | **Smooth live objects**: time-stamped samples with dead reckoning between updates (aircraft heading and speed, vessel course and speed, satellites by SGP4) so positions move at frame rate; fade by sample age | M | Today aircraft and ships jump every refresh **Done 2026-10-27:** objects move in time steps sized so the fastest one in a layer moves about half a point per step (`globecraft_render::motion`): frame rate when zoomed in, once a second from orbit, where re-projecting thousands of objects would only cost time. Aircraft keep their 25 s dead reckoning; vessels now sail their AIS course at their speed for up to 10 minutes after a report (not below 0.5 kn, which is mostly GPS jitter); satellites follow their orbit at the same steps instead of once a second. Aircraft fade from 25 s to 90 s after their last report and vessels from 10 to 30 minutes, to 35 % opacity, never hidden |
| 1.9 | **Precision at close range**: camera-relative coordinates for overlays and picking, so lines and markers do not jitter when zoomed into a street | S–M | Needed before 3D buildings and street-level work **Done 2026-10-27:** the camera, projection and picking were already f64 end to end (the GPU path runs only above 1,000 km). The jitter came from f32 screen points: a tilted close-up projects lines beside or under the camera millions of points off screen, where f32 steps are tens of points, so the clipped visible part jumped. `Basis::project_precise` keeps f64 screen points until borders, roads and ruler legs are clipped to the view. Tests: a centimetre of camera motion at the closest altitude moves the ground evenly and picking returns within a centimetre, and a far-ended line keeps its place to 0.001 point |
| 1.10 | **Elevation as a first-class tile source**: its own level of detail, shared by terrain, hill shading, cursor elevation, the ruler's elevation profile and picking | M | One DEM pipeline instead of terrain-only lookups **Mostly done 2026-10-27:** the DEM already was its own tile source (`TerrainDem`, own levels, finest-loaded lookup in `TileCache::elevation_m`) shared by terrain, hill shading, the ground-following camera and the raised overlays. Added: picking on the relief (`overlays::pick_on_ground`: the view ray is brought down to the top of the relief, sampled and bisected against the loaded heights, so double-click, right-click, the ruler and the cursor readout hit the mountain that is drawn, not the sea-level point behind it); the ruler and placemarks drawn on the relief, where they are picked (`overlays::ground_position`); the cursor's ground height in the status bar while relief is drawn (metres or feet, from loaded tiles only). Left: the ruler's elevation profile |
| 1.11 | **Streaming diagnostics overlay** (toggle): tile grid, level per tile, pending, failed and cached states, bytes per second and queue length per host | S–M | Makes tuning visible on the Mac, not only in benchmarks **Done 2026-10-27:** command *Tile diagnostics* (`ui.tileDiagnostics`, Command Search): the imagery tiles the view wants, each at the level its pixels use (so a tilted far field shows coarser tiles), outlined in green (ready), amber (loading), grey (missing) or red (failed), labelled z/x/y when few; a panel with tiles by state, downloads running, queued/downloading per source (and a server's pause), and view/ahead/border download rates |

**Exit criteria.** On the replay benchmark:

- no grey holes after a fly-to lands;
- time to sharp and wasted downloads clearly below the first baseline;
- CPU frame and interface frame times no worse than today.

Targets from the first baseline (2026-10-25, slow link: 150 ms, 1 MB/s):

- holes while moving under 1 % on every path (today 1.5–4.9 %);
- wasted downloads under 10 % (today 5–23 %, worst on a zoom);
- sharp after stop under 0.5 s for fly and pan (today 1.36 s and 0.76 s);
- motion frame median no worse than today (14–18 ms on the 2-core container).

## 2. Add Data, the GeoLibre way

Modelled on GeoLibre's Add Data (opengeos/GeoLibre). The parts worth copying:

- **One Add Data entry** with source kinds grouped into sections. GeoLibre uses Files, Web services, Cloud-native, 3D and Databases.
- **A small form per kind**:
  - the URL, a parsed preview (layer picker from GetCapabilities or TileJSON) and a test request;
  - the attribution and licence;
  - a **sample-data dropdown** of verified examples.
- **A saved service library**:
  - built-in presets that are always listed, never persisted and cannot be deleted;
  - user entries with categories;
  - import and export as JSON.
- **A Browser panel**: Services, Recent, Favorites and My Data (saved, fully configured layers).
- **Drag and drop** of files, and deep links that open data.
- **Experience levels** (basic, intermediate, advanced) that hide the long tail of kinds from new users.

In GlobeCraft every kind maps onto the foundation's `TileSource` (1.2) or the coordinate
streaming model (1.7), so adding a kind is a form plus a descriptor, never a new pipeline.

| # | Item | Size | Why / notes |
| --- | --- | --- | --- |
| 2.1 | **Add Data dialog** (a command, so the ribbon, Command Search and the assistant reach it): sections Files (KML/KMZ, GeoJSON, GPX, CSV with coordinates, MBTiles), Web services (XYZ, WMTS, WMS, OGC API Features and Tiles, ArcGIS REST), Cloud-native (PMTiles, later COG and GeoParquet), Live feeds (a GeoJSON or JSON URL polled at an interval) | L | One front door for all data |
| 2.2 | **Per-kind forms** with a capabilities parser and layer picker, a "Test" button that fetches one tile or page, attribution and licence fields, a "needs a key" notice tied to the key manager (adapter D, section 4) | M | Users see a working preview before committing a layer |
| 2.3 | **Service library**: the master table's verified ✅ Easy sources as built-in presets, each with a licence, commercial-use and longevity badge from `Data_Sources.pdf`; user entries with categories; JSON import and export | M | Sections 3 and 5 turn into presets in this library |
| 2.4 | **User layers in the Layers pane**: added data appears as layers with visibility, opacity, order, legend and attribution, saved with the workspace | M | Builds on `MapLayer` and `LayerSet` and on 6.1 and 6.2 |
| 2.5 | **Browser panel** (left pane tab): Services, Recent, Favorites, My Data | M | |
| 2.6 | Drag and drop for all file kinds onto the globe (today: KML/KMZ only), plus an `globecraft://` deep link carrying a data URL | S–M | |
| 2.7 | **Experience levels** in Settings that hide advanced kinds and ribbon groups | S | Keeps the first-run interface calm |
| 2.8 | **Catalogue search** (later): STAC, CSW and CKAN, with results handed to the matching prefilled form | M | Thousands of datasets without curating each one |
| 2.9 | **Assistant tools**: `data.add` with a URL or preset id, `data.search` over the library | S | "Add earthquakes from the last week" works by voice and text |

## 3. The boundary and map-data track (from the handoff queue), on the new foundation

| # | Item | Size | Why / notes |
| --- | --- | --- | --- |
| 3.0 | **State/province streaming and fade** (reported 2026-10-22): state and province lines arrive too late, so they pop instead of fading, especially with a high "States visible below" setting; drawing them feels laggy | M | **Implemented 2026-10-22, needs a check on the Mac.** See 3.0 below. First slice of coordinate streaming (1.7) **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 3.1 | **Municipal redesign**: fade from state lines to municipal lines, fetching only the municipalities in view at a reasonable altitude, with no stutter while panning (see 3.1 below) | L | **First slice implemented 2026-10-22** (streaming units, see below); needs a check on the Mac, then per-source tuning **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 3.2 | **Night lights layer** from NASA GIBS "Earth at Night" (`IMG-03` [16.3]). Black Marble tiles in WMTS `epsg3857` with a fixed date; blended by the sun terminator when Sunlight is on, plain otherwise | M | Wanted by the user. Pilot source for the generic `TileSource` (1.2), plus a blend term in `pixel.rs`. Check the layer identifier and date with GetCapabilities first. `IMG-24` (raw VIIRS) is a pre-bake, so skip it **Done 2026-10-26** as the pilot source: `night_lights` in `sources.json` (GIBS `VIIRS_Black_Marble`, 2016-01-01, `GoogleMapsCompatible_Level8`, PNG; layer, date and resolution checked in Worldview's configuration, the format still to confirm on the first run), layer Night Lights (Sky group), added on the night side when Sunlight is on, the whole surface otherwise; CPU path only. |
| 3.3 | **Vector rebuild, steps 2–5** of `docs/features/VECTOR_MAP_DATA.md` (see 3.3 below) | XL | Unlocks Overture divisions (`REF-18`) and every future vector layer without one-off wiring **Partly superseded 2026-10-25:** borders and names are now per-tile (13.19), `load.rs` is down to the bundled fallback. What remains is the generic seams for other vector layers, together with 1.2 and 1.7 |
| 3.4 | **PMTiles v3 reader** (`pmtiles` crate, HTTP range reads) with its own byte-bounded vector cache, separate from the 256 MiB imagery budget | M | Shared adapter C. Unlocks Mapterhorn as PMTiles (`TER-09`), Protomaps (`IMG-14`), building footprints (`BLD-01`), field parcels (`LND-05`) and EEZ borders (`MAR-03`) **Reader done 2026-10-28** (`globecraft-data/src/pmtiles.rs`, own code: header, Hilbert ids, directories, leaf cache, range reads). First use: GlobeCraft's own border tiles. The `Border tiles` workflow cuts the OpenFreeMap planet (OpenMapTiles schema) to the `boundary` and `place` layers and the ocean, zoom 0–12, and publishes them into the one `map-data` pre-release, whose `manifest.json` names the current files (a new build replaces the older files, keeping only the build before). The app reads only these; there is no OpenFreeMap fallback. Borders and names come to 4.3 GiB, over GitHub's 2 GiB asset limit, so they are split into parts (zoom 0–10, zoom 11, zoom 12 west and east of 0°); the reader asks only the part whose zooms and bounds hold a tile. Sizes in PERFORMANCE_BASELINE.md. Second set (2026-10-28): `transport` (rail, transit, ferries, airports, zoom 0–14, 626 MiB, one file), drawn by the Railways, Ferries and Airports layers. Third (2026-10-28): `roads` (motorway to residential with names, zoom 0–12, 2.5 GiB, three files), the Roads layer's source up to 1,000 km; Overpass only adds service roads and roadworks close up. Open: a disk cache for transport tiles. **Byte budget 2026-10-29:** each vector set's cache is bounded by both tile count and a 64 MiB estimated decoded-heap budget, keeping at least one tile; the estimate excludes allocator overhead |
| 3.5 | **Polar view**: a dedicated polar projection (GIBS epsg3413/3031) in place of the `PolarCap` interim | L | The user prefers a real pole map. Needs per-provider projection in the tile path (`proj4rs`) |
| 3.6 | Disputed borders: a per-country "worldview" choice (Natural Earth map units and geoBoundaries) | M | "As the country represents itself" is a common request and the data is already there **2026-10-25:** OpenMapTiles carries one consensus border plus `claimed_by` lines, not per-country worldviews. The neutral view and the "both countries' claims" option (13.18) cover what the tiles allow; a true worldview needs another source |
| 3.7 | Trim the boundary log to the last N lines in the data crate, not only in the pane | S | The pane now shows the last 12 lines; the log itself still grows **Done 2026-10-25:** the data crate keeps the last 64 lines |

**3.0 State/province streaming.** Causes found in the code:

- Ordinary countries' states exist only in Natural Earth 1:10m. The world detail job requested them only below 1,500 km, after the 10m countries, map units and borders, about 70 MB ahead of them in the queue. They fade in from 1,609 km by default, or from the user's "States visible below" value, so the band starts before the data exists.
- The polygon file (40.7 MB, 1.3 M vertices) draws every shared border twice and outlines every coastline as a state line. The 1:10m lines file has 416 k vertices and no coastlines.
- Each boundary patch rebuilt the overlay index from scratch, recomputing bounds for every line. Panning with municipal data made this happen again and again.
- Data that arrived inside the fade band appeared at full strength (a pop), because the fade depended only on altitude.

Fix (implemented): a separate state job that starts well above the visible band (1.6 × the setting), the lines file plus label points, an index that reuses unchanged line bounds, and a 0.6 s fade when a layer arrives. Measured on the 1:10m data: index 76 ms instead of 235 ms, projection about half, 3.3× fewer segments.

The lag had a second cause on the UI thread, found with a scripted descent (`boundary_streaming_descent`): every new overlay view rebuilt its shapes there, 30–70 ms with 40 k border segments and ~900 names. Fixed (implemented): the overlay workers tessellate borders and the UI only tints them; label galleys are kept across rebuilds; at most 120 new names are laid out per frame. UI pass p99 over the descent went from 19.5 to about 9.5 ms. Still open: check on the Mac, and measure whether the Rayon raster pool should leave one core to the UI thread (on the 2-core container, remaining outliers are preemption, not work).

**3.1 Municipal redesign.** Three parts:

- **(a)** Viewport-first loading for every country. Only FRA, DEU, ESP, USA and CAN do this today; geoBoundaries needs client-side clipping, and Brazil IBGE always loads the whole country.
- **(b)** A size field (area or population) on `MapLabel`, so the biggest municipal names win first instead of sharing `LOCAL_LABEL_RANK=10`.
- **(c)** Staging: states first, municipal labels only from about 200 mi inward, faded in and out.
- **(d)** Takeover (user, 2026-10-22): the accurate municipal outlines also trace the state and country borders, so they take over those lines first. Only at a narrower zoom do the interior municipalities and their names appear. A thousand German municipalities on screen at once is what costs hardware; staging by zoom keeps both the line count and the label count bounded. See 13.1.

Implemented first slice (2026-10-22): municipal data now streams in *units* around the view, nearest first, from about 640 km down (before the 402 km fade): national grid cells with stable requests (FRA 0.25°, others 1°, each within a per-source radius), one Brazilian state at a time from IBGE instead of the whole country, whole-country geoBoundaries files clipped to the view, and OSM cells as the fallback. A bounded store keeps loaded units, so panning back is instant, and a throttled publisher adds cells to the layer instead of swapping it after every request. State lines stay under the municipal outlines. Still open: (b) size-ranked labels for every source, per-country visible altitudes for dense countries, and a bounded on-disk cache for cells.

**France lags behind Germany and Spain** (user, 2026-10-22). Two causes:

- *Fixed 2026-10-22:* IGN ADMIN EXPRESS v4 renamed the département field from `insee_dep` to `code_insee_du_departement`, so every French commune had an unknown parent. No French département border was recognised, nothing was drawn as a municipal state border, and the Natural Earth lines never handed over. Both names are read now, with the commune code's first two characters as a fallback.
- *Open:* French cells come from the full-resolution commune layer (about 5 MB per 0.6° box, the WFS caps at 1,000 features), so France loads only 0.75° around the view centre against 1.5° for Germany and 2° for Spain. Next: a generalised source, either IGN's `ADMINEXPRESS-COG-CARTO` layer on the same WFS or geo.api.gouv.fr contours per département, measured like the 2026-10-21 table before switching. Check the layer name and attribute names with `DescribeFeatureType` first; the build container and the Mac shell cannot reach data.geopf.fr.

**3.3 Vector rebuild.** The rewrite already did half of step 4: `MapLayer` and `LayerSet` replace the per-layer booleans in the UI. Next:

- the data-side seams (`LayerId`, `LayerSpec`, `TileKey`, `LayerTile`, `MapSnapshot`, `CountryIndex`);
- per-source adapters instead of the 693-line `load.rs`;
- per-tile refresh, then cross-fade.

### 3.5 Own OpenStreetMap tile sets (2026-10-27)

Borders and names come from PMTiles archives cut from OpenFreeMap's weekly planet
(`.github/workflows/map-data.yml`, manifest `map-data/manifest.json`), read
tile by tile with range requests (`globecraft-data/src/pmtiles.rs`); OpenFreeMap's full
tiles stay the fallback. Measured on the Mac's cache: the kept layers are 0.4–1.5 KB
per tile at zooms 8–12 against 100–500 KB for the full tile.

The same workflow can cut further OpenMapTiles layers into their own archives, each
fetched only while its layer is on (the loader concatenates archives per tile):
`transportation` and `transportation_name` (roads, railways, ferries, lifts, paths),
`waterway` and `water_name` (rivers, lake and sea names), `landcover` and `park`
(forest, glaciers, protected areas), `landuse`, `mountain_peak` (peaks with heights),
`aerodrome_label` and `aeroway` (airports, runways), `poi`, and `building` (zoom 13–14
only, a base for 3D buildings, 8.6).

## 4. Shared enabling work (build once, unlock many)

These come straight from the data-source triage. Each one turns a column of "moderate" rows
into "easy" rows. With the foundation in section 1, several become part of it: A (WMS) and C
(PMTiles) are access types of the generic `TileSource` (1.2), B is the elevation source (1.10),
G is part of smooth live objects (1.8), and the time index of H lives in the tile key (1.2).
What remains here is the key manager (D), GRIB (E), COG windows (F), the app-wide time model
and timeline (H) and the point-layer engine (I).

| Tag | Work | Size | Unlocks |
| --- | --- | --- | --- |
| A | WMS-as-XYZ adapter (tile bbox → `GetMap`, EPSG:3857) | M | GEBCO seafloor (`TER-01`), EMODnet bathymetry (`TER-04`), DWD radar (`WTH-06`), GloFAS floods (`HAZ-07`), EMODnet shipwrecks (`HER-05`) |
| B | Elevation decoders (Terrarium is done; add GSI and WebP Terrarium) and a hillshade-only mode for overview altitudes | S–M | Japan GSI (`TER-05`), OpenZenith fallback (`TER-03`), AWS Terrain Tiles as a second terrain source (`TER-06`; Mapterhorn should not be the only source) |
| C | PMTiles (item 3.4) | M | See 3.4 |
| D | Key manager, generalised from the assistant's key file: per-service keys from env or user config, 0600, never in `settings.json`, a "Keys" page in Settings | M | OpenWeatherMap (`WTH-04`), AISstream (`TRK-03`), FIRMS fires (`HAZ-09`), OpenAQ (`WTH-11`), ENTSO-E (`INF-06`), Earth Timelapse (`IMG-28`), TomTom (already used) |
| E | `grib` crate prototype for small GRIB2 subsets | M | GFS and ECMWF wind (`WTH-10`, `WTH-08`) |
| F | COG window reads (`async-tiff`) | M | Copernicus DEM (`TER-21`), WorldCover (`LND-06`), ArcticDEM and REMA (`TER-25`, `TER-29`) |
| G | `sgp4` for satellites | S | Real orbits instead of the two-body approximation (`TRK-04`) |
| H | **Time dimension**: one app-wide "time" (now, a date, or a range) that time-aware layers read | M | Wayback, GIBS dailies, night lights by year, historic maps (`IMG-21`, `IMG-25`), Earth Timelapse 1984–2022 (`IMG-28`), IBTrACS storm history (`HAZ-08`), and the sun and stars. **This is the single most valuable new concept** |
| I | **Point-layer engine**: GeoJSON, CSV or JSON feed to clustered, ranked, hover-able markers, with the same occupancy rules as labels and a byte-bounded cache | M | Earthquakes (`HAZ-04`), aurora (`HAZ-01`), NWS alerts (`HAZ-05`), buoys (`WTH-02`), METAR (`WTH-01`), airports (`TRK-02`), power plants (`INF-01`), wind turbines (`INF-02`), Pleiades (`HER-06`), GBIF (`HER-01`), volcanoes (`HAZ-03`) |

## 5. Layers by value: presets in Add Data

**Quick wins (S each once I and H exist):**

- USGS earthquakes, with magnitude-scaled pulses (`HAZ-04`)
- NOAA aurora oval, drawn on the night side (`HAZ-01`)
- OurAirports (`TRK-02`), which also lets flight hover show origin and destination guesses
- Natural Earth urban areas as a low-altitude glow (`REF-04`)
- Time zones, as a toggle with the local time under the cursor (`REF-05`, `tzf-rs`)

**Weather and earth system:**

- GIBS daily true colour and clouds (`IMG-04`) as the honest "today" imagery
- Animated wind particles from GFS (`WTH-10`; GPU work, see section 8)
- Hurricane cones and tracks (`HAZ-10`)
- Active fires (`HAZ-09`)
- Lightning (`HAZ-06`, with `HAZ-11` as fallback)

**Ocean:**

- Seafloor basemap (`TER-01`)
- Exclusive economic zones (EEZ) and high seas borders (`MAR-03`)
- Port traffic (`MAR-01`)
- Undersea feature names (`REF-13`)

**Energy and land:**

- Power plants (`INF-01`), wind turbines (`INF-02`) and Global Energy Monitor trackers (`INF-04`)
- Forest loss (`LND-01`)
- Field parcels for France (`LND-04` and `LND-05`)

**Heritage and nature:**

- GeoNames heritage filter (`HER-02`), Pleiades (`HER-06`) and Wikidata archaeological sites (`HER-04`)
- The Stonehenge LiDAR showcase (`HER-07`)
- Species occurrences on demand at the cursor (`HER-01`)

**Other worlds:**

- Moon, Mars and Vesta Trek tiles (`OTH-01` to `OTH-03`) through a body-aware tile math abstraction (L)
- This turns the Explorer's static images into real browsable planets

## 6. Interface and design

| # | Item | Size | Notes |
| --- | --- | --- | --- |
| 6.1 | **Layer opacity and order**: a slider per layer, drag to reorder, "solo" on alt-click | M | Standard in GIS tools. The ribbon and pane already share one layer identity |
| 6.2 | **Legends**: a compact legend under each active data layer (radar dBZ, quake magnitude, fire confidence…) | M | No data layer currently says what its colours mean |
| 6.3 | **Timeline bar** at the bottom of the globe for adapter H: play/pause, speed, date picker, "now" button | M | The visible half of the time dimension |
| 6.4 | **Status bar redesign**: altitude, scale bar, cursor elevation (from the DEM already in the cache) and the attributions of the visible layers instead of one fixed line | S–M | Free elevation readout: the DEM is already decoded |
| 6.5 | **First-run onboarding**: three cards (navigate, layers, assistant) and an optional sample tour | S | The ribbon has many commands; first contact needs a guide |
| 6.6 | **Accessibility**: full keyboard navigation of the ribbon and panes, a visible focus ring, accesskit labels on every custom widget (most already have them), a reduced-motion setting (no fly animations or fades), high-contrast tokens, UI scale presets | M | egui supports accesskit, and the custom widgets set `widget_info` |
| 6.7 | Theme polish: system accent colour, a Mac "vibrancy" sidebar look, and an icon audit (one stroke weight, a 24-unit grid, no mixed emoji) | S–M | Also item 0.6 |
| 6.8 | **Pane presets**: "Explore", "Measure" and "Present" (hide chrome, big globe, tour controls), saved as workspace layouts | S | `WorkspaceLayout` already persists |
| 6.9 | Layers pane search and filter (by name, category and active state) | S | The catalogue itself is Add Data (section 2); this keeps a long Layers pane usable |
| 6.10 | More languages (es, fr, de, ja) | S each | The Fluent setup makes this cheap; ask native speakers for review |
| 6.11 | Touch and trackpad: pinch to zoom, two-finger rotate and tilt, inertia tuned for Mac trackpads | S–M | Mac users expect these gestures **2026-10-25:** pinch zooms and the two-finger turn rotates the view. **Tilt landed the same day:** a look-at camera with pitch (up to 80°, the camera stays 300 m above the ground), right-drag or Shift-drag up and down, Shift+scroll, Shift+↑/↓, U for straight down, `tilt_deg` on `camera.set` and `camera.flyTo`. Zooming in while tilted stops at that height instead of flattening the tilt. **Navigator tilt control 2026-10-27:** an arc around the left of the ring in both navigator styles, straight down at its top and 80° at its bottom; drag the thumb or click the arc, double-click for straight down. Still open: the scale bar and view disc for tilted views, and GPU parity |
| 6.12 | Cursor info card: hover or long-press a point for elevation, local time, country/state/municipality, nearest place and copyable coordinates in every format | S–M | Everything is already in memory |

## 7. AI assistant

| # | Item | Size | Notes |
| --- | --- | --- | --- |
| 7.1 | **Vision**: send the current screenshot (downscaled) with the question, so the assistant can say what it sees | M | All three providers accept images. Gate it per request and show "shared a screenshot" in the transcript |
| 7.2 | **Tour authoring**: "make me a tour of the Silk Road" produces a placemark folder plus camera keyframes (existing tour engine) | M | Builds on `places.add` and `camera.flyTo` |
| 7.3 | Data-aware tools: `query_layer` (earthquakes in view, flights near the cursor…) and `elevation_at` | M | Read-only tools are cheap once the point engine (I) exists |
| 7.4 | **Local models** first-class: an Ollama preset with auto-discovery, and an "offline mode" that never sends coordinates out | S | The preset already exists; add discovery and a privacy badge |
| 7.5 | Saved conversations per project or placemark file | S | |
| 7.6 | Assistant-proposed layers: "show me where wind farms are" searches the Add Data library (section 2) and offers to enable one, with its licence | M | |
| 7.7 | Guardrails review: a rate limit on `run_command` loops, a cost estimate per turn, and a clear "stop" everywhere | S | |

## 8. Rendering and engine

| # | Item | Size | Notes |
| --- | --- | --- | --- |
| 8.1 | **Terrain occlusion**: true heightfield ray marching (today it is a 2.5D displacement with no occlusion), at least for close views | L | The next visible quality jump. **Done on the CPU path 2026-10-26** (user video: the displacement smeared imagery on tilted views, had no occlusion and went flat toward the horizon). Each ray is marched through the Mapterhorn tiles with a pyramid of block maxima per tile (`tiles::Elevation`, built at decode), skipping open air block by block; the elevation level follows the distance, so relief reaches the horizon and stands against the sky. The camera looks at the terrain height (eased), a tilted camera stays 150 m above the ground beneath it, and borders and names are raised onto the relief (not hidden behind ridges yet). **Faster 2026-10-27:** one cone per 8×8 pixel block walks the open air first (it skips a block only when its whole footprint clears the block's maximum), so pixels start a few texels above the ground: steps per pixel 12.8–23.9 → 7.6–15.3, relief cost per frame down 25–55 % (PERFORMANCE_BASELINE); motion frames over relief render at 0.28 instead of 0.4 of the window. **Live layers on the relief 2026-10-28:** roads, railways, ferries, airports, rivers, parks and place/peak markers use the same `GroundHeights` raising as borders, ruler and placemarks. **Overlay occlusion 2026-10-29:** borders split into visible runs and roads, transport, airports, POI points/lines, vessels, placemarks and satellite subpoints are tested against the relief with a 16-sample eye-to-line check, only when relief is drawn and the view is tilted at least 1°. Still open: the GPU path (meshes, 8.2) |
| 8.2 | GPU renderer to beta: parity tests, device loss, macOS Metal profiling, then "auto" by default on Apple Silicon | XL | Experimental today; CPU stays the fallback. Unlocks wind particles, 3D clouds and smooth terrain |
| 8.3 | Atmosphere v2: physically based scattering (Bruneton-style precomputed tables), with sunset colours following the real sun | M–L | Big wow factor for the cost |
| 8.4 | Day/night: night lights (3.2), a terminator glow, city lights fading in at dusk | S after 3.2 | |
| 8.5 | Ocean specular highlight and a simple wave normal map from a water mask | M | Cheap realism |
| 8.6 | 3D buildings: extrude Overture footprints (`BLD-04`) near the camera on the GPU path; a flat "footprints" overlay first | XL | Pre-bake per metro or use PMTiles footprints (`BLD-01`) |
| 8.7 | Imagery colour correction: match Esri, Wayback and Topo brightness at zoom transitions (no visible seams between providers) | S–M | |
| 8.8 | Frame pacing: decouple overlay preparation from raster publication further, and measure 120 Hz behaviour on ProMotion displays | M | |
| 8.9 | Offline packs: "Download this region" (imagery, DEM and borders to MBTiles) for travel and field work | M | Respect provider prefetch policies; OSM stays on demand |

## 9. Places, files and sharing

- **KML fidelity (L):** lines, polygons, styles, NetworkLinks and time spans. Only placemarks import today (the v0.2.0 plan deferred KML fidelity), and Google Earth users bring KML.
- **GPX and GeoJSON import/export (M):** for hikers and data people.
- **Movie export (L):** render a tour to MP4 frames, like Earth Studio-lite. It needs a deterministic render clock.
- **Poster export (M):** high-resolution offscreen renders with a title, scale and attribution block. **Screenshots, 2026-10-25:** four qualities, all waiting for every tile: Normal (screen size, as on screen), Plus (2×, one zoom level deeper), Pro (4×) and Pro Max (8×, tiles the view never showed). Plus and above show one imagery level per pixel (no level fade), and the same tile tier across the picture: the level most of it would use, applied wherever a pixel's own level is within one of it, so no seams between tiers show (a tilted view's far field keeps its own). The picture is rendered and written to the PNG strip by strip on its own thread pool, so it never needs to fit in memory and the globe stays responsive; *Cancel Screenshot* stops it. The poster layout is still open.
- **Share a view link (S):** a `globecraft://` URL or text snippet carrying the camera, layers and time, which other GlobeCraft users can open.
- **Measurement upgrades (M):** area, elevation profile along a ruler path (from the DEM cache), and line-of-sight and viewshed (after 8.1).

## 10. Engineering quality

| # | Item | Size | Notes |
| --- | --- | --- | --- |
| 10.1 | **CI on macOS, Linux and Windows** running the full gate, the ignored-by-default UI screenshot tests on Linux with lavapipe, and fixture diffs | M | The kittest snapshot infrastructure exists |
| 10.2 | **Split the `App` struct** (about 100 fields) into feature states (camera, layers, live data, places, measure, assistant), each owning its `run` arm | L | It is the remaining "god object"; the command layer makes the split safe |
| 10.3 | Versioned settings migrations with tests per version | S | Settings formats changed several times |
| 10.4 | A frame-budget watchdog: when a UI pass exceeds N ms, record a bounded trace event (profiling builds only) | S | Freezes become diagnosable from a capture |
| 10.5 | Fuzz the decoders (KML, MBTiles rows, Terrarium, JSON feeds) with `cargo-fuzz` | M | They parse untrusted data |
| 10.6 | Make the network policy explicit: one table of hosts, rate limits and user agents per service, and a Settings page listing every host the app may contact | M | Privacy and trust; this also documents attribution |
| 10.7 | Packaging: notarised macOS build, a Windows MSI, a Linux AppImage or Flatpak, and an auto-update channel choice | M–L | |
| 10.8 | Check WebAssembly feasibility of core, render and data without the native bits | M | AGENTS.md notes the data-API split; a web viewer is a big reach multiplier |

## 11. Bigger, stranger ideas worth keeping

- **"Time machine" globe:** with adapter H plus Wayback, Timelapse and historic maps, scrub from 1890s topographic maps to today at one place.
- **Sky mode:** look up from the ground with real stars, planets, ISS passes (SGP4) and aircraft overhead (already loaded). A planetarium inside a globe.
- **Follow mode:** pick a flight, ship or satellite and the camera follows it; a "cockpit" view for flights with terrain.
- **Eclipse and shadow paths:** compute and draw upcoming solar eclipse paths and the Moon's shadow live.
- **Ambient sound (opt-in):** wind, ocean and city hum mixed by what is under the camera; aircraft whoosh when following.
- **Story maps:** a placemark folder becomes a narrated scroll-through presentation (tour plus text plus images); export to HTML.
- **Collaborative view (later):** two people share a camera via a short code, peer-to-peer, for teaching or planning trips.
- **Field notebook:** attach photos and voice notes to placemarks; geotag photos dropped onto the globe from their EXIF data.
- **Accessibility audio globe:** describe what is under the cursor by voice (country, terrain, weather), useful for blind and low-vision users and in classrooms.
- **Classroom kit:** a "teacher mode" with locked layers, guided questions and a quiz tour format.
- **Other bodies as data:** Mars and Moon Trek elevation through the same terrain path, so you can fly Valles Marineris with relief.
- **Archaeology showcase:** a curated set of LiDAR hillshade flyovers (Stonehenge `HER-07`, Maya `HER-19`, Acre geoglyphs `HER-10`) with explanations.

## 12. Explicitly not planned

- **Google Photorealistic 3D Tiles, or scraping Google/Apple tiles:** terms of service.
- **Self-hosting large datasets** (see the user's upstream-only rule); pre-bakes only where they are small and the licence allows redistribution.
- **Shipping any API key**, including demo keys.
- **Non-commercial-only sources as defaults:** RainViewer, Open-Meteo, GFW and openAIP stay opt-in with a visible licence note.
- **Excluded sources:**
  - GADM (`REF-19`) and UN SALB (`REF-21`): licences.
  - ADS-B Exchange (`TRK-08`): paid.
  - FABDEM (`TER-32`) and Viewfinder (`TER-33`): licences.

## 13. Second batch from the user (2026-10-22)

Fix 2026-10-22: state borders had gaps (Texas–New Mexico, Montana–Dakotas, French regions,
Russian oblasts, Chinese provinces). Natural Earth's 1:10m lines file splits real state
borders into several classes and only one was kept, dropping 21 % of them; all real classes
are kept now (99.4 % of shared admin-1 polygon edges covered, checked against the polygons).


| # | Item | Size | Why / notes |
| --- | --- | --- | --- |
| 13.1 | **Municipal takeover and staging** (3.1 d): municipal outlines replace the coarse state and country lines first, where municipalities sit on those borders; interior municipalities and their names appear only at a narrower zoom, biggest first | M | **First slice implemented 2026-10-22.** Segments are classified per loaded unit (state border, interior, shared); state-border segments draw from 402 km, interior lines and small names only below an altitude set by the municipalities' mean size (about 75 km for Germany), and shared borders are drawn once instead of twice. **Second slice 2026-10-22:** each new unit is classified against the rings already stored around it, so borders between units (Brazilian states, two countries' sources) become state or country borders too; and the Natural Earth state lines give way wherever municipal state borders were loaded, so the accurate lines take over without leaving the rest of the view bare (made instant by 13.12). Still open: checking on the Mac **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.2 | **One header row**: merge the logo and Quick Access Toolbar row with the File / Home / Layers… tab row; drop the "placemarks.kml — Earth" title | S | **Done 2026-10-22; title bar merged 2026-10-28.** File, tabs, Quick Access Toolbar, both searches and the collapse button share one 38-pt row; the logo and the duplicated Open KML/Save Quick Access icons were removed on 2026-10-28. The header now acts as the title bar: macOS keeps its native traffic lights over a transparent titlebar, while Windows and Linux remove native decorations and draw minimize, maximize/restore and close at the right. The window title remains **GlobeCraft** for Mission Control, the taskbar and window lists, but no file or body name is drawn in the header |
| 13.3 | **All planets and moons, streamed**: NASA Solar System Treks WMTS (Moon, Mars, Mercury, Vesta, Ceres, Titan…), USGS Astrogeology and GIBS where they serve tiles; one `TileSource` per body (1.2), with elevation where available (1.10) | M–L | Today the Moon and custom planets are single images. Credits per body in CREDITS.md |
| 13.4 | **Borders in the dark**: when Night lights / sunlight are on, draw land borders at full strength all around the night side (today they follow the lit-side styling) | S | Pairs with 3.2 and 8.4 **Done 2026-10-25** (user: coasts are not drawn, so countries vanish into the dark): with Sunlight on, the country outline along the sea (OpenMapTiles `water`, class ocean, tile edges removed) is drawn where the sun is below the horizon, in the country style, or the state style with only states on. Lakes are left out: small ones would ring every pond. Cache format 4 |
| 13.5 | **Navigator in the Fusion 360 style**: a small view cube or mini globe that shows orientation, click a face or pole to snap, with compact move and zoom controls beside it | M | Replaces the large ring, pad and slider stack **Done 2026-10-25**, the mockup's behaviour in the classic navigator's look (user: the mockup was inspiration, not the design): top right, the classic rotation ring with its cardinal letters (click one to turn that direction up, eased) and the classic + / − buttons and zoom slider; a mini globe in the ring's centre, drawn in the controls' colours with a graticule, the equator and the area in view, dragged to turn the Earth; the wheel zooms over the whole navigator. Settings → Navigation → Navigator: Globe (default) or Classic (the original ring, move pad and zoom slider); command `ui.navigatorStyle` |
| 13.6 | **Small, simple live symbols**: one minimal glyph per category for aircraft (by size class), ships (by AIS ship type: cargo, tanker, passenger, fishing, sailing…) and satellites (by group); satellites drawn at their real altitude above the globe | M | Uses the category fields the feeds already send. Pairs with 1.8 smooth motion |
| 13.7 | **"Map data not yet available" at maximum zoom**: detect Esri's placeholder tiles (they come back as normal images) and treat them as missing, so the deepest real level is upsampled instead (overzoom) | S–M | **Done 2026-10-22.** Requests carry `blankTile=false`, so Esri answers 404 where it has no tile; 404s and placeholders already in the disk cache mark the tile unavailable for the session, and renderers show the deepest real level without waiting or re-requesting |
| 13.8 | **Style settings**: colour, thickness and dash pattern per line type (country, state, municipal), label sizes per kind, and a reset button | M | **Done 2026-10-22.** Settings → Map style, saved as `map_style` in settings.json; commands `style.line` (`line`, `color` as `#rrggbb[aa]`, `width`, `dash`: solid, dashed or dotted), `style.labelSize` and `style.reset`. Municipal state borders use the state style, so the handover (13.12) keeps one look whatever the user picks. Still open: checking on the Mac |
| 13.9 | **Wayback**: a drop-down calendar of the available release dates, and a **swipe compare** of two dates with a slider across the globe | M | The release list is already loaded; compare needs two imagery requests per frame. **Done 2026-10-23, not yet seen live** (the Wayback list is unreachable from the build container): the ribbon's date is a drop-down grouped by year; **⇆** in the ribbon, or *Compare with* in the Layers pane, starts a swipe compare. The second date shows right of a white split with a knob that drags left and right, each side labelled with its date. One raster carries both dates (`RenderCfg::compare` picks the release per pixel column), so the cost is one frame; compare frames render on the CPU. Command `layer.waybackCompare` (`enabled`, `date`, `step`, `split`). **Reworked 2026-10-25** (user: split "very rough", both dates adjustable, years as their own drop-downs): the compare release renders as its own frame on a second frame worker (only while a compare is open) and the globe cuts the two finished frames at the split, so dragging renders nothing and no longer drops to motion resolution; each side has a date drop-down at the top of the split plus a swap button (`layer.waybackCompare` with `left_date`, `date`, `swap`); the date menu lists years, each opening that year's releases. The per-column compare in the renderer is gone |
| 13.10 | **Remove the metropolitan regions layer**: municipal borders cover it. Optionally, a country picker for municipal borders (only the selected countries load) | S | **Removal done 2026-10-22** (layer, IBGE metro pass, Overture metro slices and their scripts). **Country picker done 2026-10-22:** `layer.municipalCountries` and a Countries line under Municipal borders in the Layers pane (Only this country, Add this country, All countries); the choice is saved |
| 13.11 | **GlobeCraft repository**: fresh history, English only, straightforward README for non-technical users, every source credited in CREDITS.md | S | Done 2026-10-22 |
| 13.12 | **Instant border handover** (user, 2026-10-22): when one resolution of a border replaces another, the swap happens in a single frame. No frame may show neither line, and none may show both | S–M | **State lines done 2026-10-22.** Municipal state borders no longer cross-fade with the Natural Earth state lines (322–402 km) or fade in on arrival; at 402 km, or when a municipal unit loads, the covered stretch of the coarse line is cut out of the same geometry the municipal border is added to. The cut is exact: a coarse point is covered within 3 km of a municipal state border and only beside it, so the coarse line resumes right where municipal data ends (edge of the loaded area, a coast, an unloaded neighbour) instead of leaving a 5–10 km gap. See 13.12 below. Natural Earth detours between two replaced stretches (under 30 km) are dropped too. **Blank frames fixed 2026-10-22** (user video): while municipal patches streamed in during a zoom, the overlay painted nothing for 2–6 frames because no geometry matched both the new view and the new dataset; it now paints the previous dataset's geometry for exactly that view until the replacement is ready. Country borders: done in a second attempt, see below **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.13 | **German pattern in every country** (user, 2026-10-22): state borders upgraded by municipal borders, then single municipalities closer in, wherever the data allows; a list of the countries where it does not. The municipal button shows only municipalities; the sharper state borders belong to the state button | M | **Done 2026-10-22, second attempt.** geoBoundaries files name no state, so outside the six national sources no state border was upgraded. The first attempt (tracing municipal segments along Natural Earth lines) broke borders into fragments and was removed. Now each municipality gets the geoBoundaries state polygon containing it (`boundaries/parents.rs`), the same rule as the national sources; checked on real data for CZ, PL, AT, SK and IT before shipping. Each country has a reviewed municipal and state level (`adm.rs`). Country borders are drawn from one side's municipal outline (13.12). With the state or country layer off, the municipal lines on those borders stay drawn as ordinary municipal lines. Per-country list: `docs/MUNICIPAL_COVERAGE.md` **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.14 | **Cleaner municipal country borders** (user, 2026-10-23): no rogue blobs or short stray lines where a country's municipal file has holes or slivers near a border (seen at 26.3° N, 89.0° E on the India–Bangladesh border) | S–M | **Done 2026-10-23.** Most of the "blobs" were the real border (the Patgram salient); the stray ones were land both countries' files claim. Contested land is no longer drawn as country border. Details below **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.15 | **Country borders, measured on every land border** (user, 2026-10-23: "fixing them requires global, not local fixes") | M | **Done 2026-10-23.** A global audit (`scripts/border-audit`) runs the app's own code on all 283 land borders with geoBoundaries data on both sides (199,000 km) and measures replaced, doubled, kept and broken stretches. Fixes, each a general rule: an edge both countries' outlines share is border up to 16 km from Natural Earth's line; the coarse line is cut per border, within 11 km of the accurate version of the *same* border; a coarse detour is dropped only where a municipal line really bridges it; holes under 2 km in a drawn border are bridged; where the drawing side has no line, the other side's outline draws the border (if the drawing side's land lies across it); an unconfirmed edge needs land on both sides (no coasts); disputed land stays undrawn on both sides. Results below **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.16 | **Town, village and island names** (user, 2026-10-23) | M | **Done 2026-10-23, not yet seen live** (Overpass is unreachable from the build container). Natural Earth names only about 7,300 places and no islands. Names now stream from OpenStreetMap in 0.5° cells below 160 km: towns and big islands from 150 km, villages and mid-sized islands from 50 km, hamlets and islets from 20 km (`boundaries/places.rs`). A place name is skipped where a label with the same name is already shown nearby **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.17 | **States close at country borders** (user, 2026-10-23) | S | **Done 2026-10-23.** With the state layer on and the country layer off, country borders are drawn in the state style, municipal and coarse alike, with the same handover **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 13.18 | **Disputed borders, dashed** (user, 2026-10-23/24: Arunachal Pradesh missing; "Google shows them dashed") | S | **Done 2026-10-25 for the OpenStreetMap tile layer.** A first try (Natural Earth lines in red, municipal lines giving way) was reverted on 2026-10-23 (branch `snapshot/2026-10-23-disputed-red`). Now OpenMapTiles' own marks decide: `admin_level` 2 lines with `disputed=1`, and the claim lines (`claimed_by`), draw once each (claimants' copies merged) in a new *Disputed borders* style, white and dashed by default (Settings → Map style, `style.line` `disputed`). Nothing else is hidden or cut for them. With the country layer off they close the states or municipalities like other country lines. Neutral view by default: one line where the border actually runs (a disputed line that is a country edge); each country's claimed line only with Settings → Map style → "Show both countries' claims" (`style.disputedClaims`) **Changed 2026-10-25 (user):** the neutral view did not work; every disputed and claimed line is drawn again and the setting and `style.disputedClaims` are gone. Lines that came and went with zoom were the neutral rule hiding OpenStreetMap's claim lines from zoom 5; the Natural Earth (zoom 0–4) and OpenStreetMap (zoom 5+) disputed sets still differ |
| 13.19 | **One shared line per border** (user, 2026-10-23: "if you can fetch the names, couldn't you fetch the borders too?") | L | **Prototype 2026-10-24, not yet seen live** (OpenFreeMap is unreachable from the build container). Layer *Borders and names from OpenStreetMap tiles* (`osm_border_tiles`, off by default): OpenMapTiles vector tiles from OpenFreeMap streamed like imagery (`boundaries/osm_tiles`: protobuf decoder, zoom from altitude, ≤ 96 tiles, missing tiles shown from an ancestor cut to them). OpenStreetMap stores each border once, so nothing is matched or cut: `admin_level` 2 draws as country, 3–4 as state, 5–8 as municipal (from zoom 11, about 40 km); the buttons keep their rules. Place names, islands included, come from the same tiles. Until the first tile arrives the old sources stay on. Next: compare on the Mac (Germany, Kashmir, Pacific islands); then decide on municipal lines between 40 and 400 km and on retiring the municipal pipeline (docs/BORDER_PIPELINE.md). Alternatives kept in view: Protomaps, Overture `division_boundary` tiles built by a GitHub job. **Phase 1 done 2026-10-25** (plan: project doc `claude/osm-borders-plan.md`): tiles fetched only to zoom 12 (deeper tiles add only buildings and roads); only land borders (raw `admin_level`, disputed and claim flags) and names are kept, cut at the tile edge, in a compact format on disk (`osm_borders.sqlite`, Settings → Cache, 64 MiB default, applies at once, least recently used dropped first); disk first, network only for missing tiles or tiles of an older tile set (shown until replaced); the tile URL is cached for offline starts; memory bounded by vertices; parents and the ring around the view load ahead; pieces cut at tile edges are joined again (continuous dashes); the view is composed off the UI thread. **2026-10-25:** decided to stay with OpenStreetMap tiles from OpenFreeMap, no own hosting (plan kept as project doc `claude/github-tile-job-plan.md`). Quick zoom-outs: every level above the view, the rings around the next two levels and the world to zoom 3 load ahead; tiles to zoom 6 stay in memory. Disputed borders: one neutral line by default (the de facto border, dashed), each country's claim as a map-style option. **Loading 2026-10-25** (user: loading slower than before): border tiles loaded ahead (about 300 per view change, whole OpenMapTiles tiles with roads and buildings) took bandwidth from the imagery; they now download only while the view's imagery is complete, on at most 2 of the 4 border slots. The 573 MB the old pipeline downloaded into `geo/` are deleted at startup (the Wayback list stays). **Bandwidth 2026-10-25, second pass** (user: zooming in still slower): border tiles are now one level coarser outside the municipal range (two tiles across the view instead of four, a quarter of the downloads) and are not downloaded while a zoom passes through levels (350 ms settle); Settings → Cache shows the session's downloads per kind, and the run log gets the totals once a minute. **Phase 2 done 2026-10-25:** the old pipeline is removed (Natural Earth 1:50m/1:10m downloads, national municipal services, geoBoundaries, Overpass municipal cells, the matching and handover code, the border audit, the separate tile layer and the municipal-countries setting); only the bundled Natural Earth 1:110m set stays as fallback before the first tile and for the country lookup. State names come from the tiles. Snapshot branch `snapshot/2026-10-25-before-cleanup`. `docs/BORDER_PIPELINE.md` describes the tile pipeline |

**13.12 Instant border handover.** Where two versions of a border meet, exactly one is
drawn, and the switch between them happens between two frames:

- **Natural Earth state lines → municipal state borders**: done (see the table).
- **Natural Earth country lines → municipal country borders**: done 2026-10-22, second
  attempt. The first (matching both countries' versions segment by segment) alternated
  between two sources and was reverted. Now each border has one source
  (`municipal/borders.rs`): only a country's outline segments are candidates, Natural
  Earth's land-border lines (which name both countries) say which border a segment runs
  along, and one side draws it: a national source beats geoBoundaries, otherwise the
  lower ISO code, and an unloaded neighbour leaves the border to the loaded side. The
  other side's outline is hidden where the drawn line runs, and the Natural Earth
  country line is cut there with the same exact cut as the state lines. Inside one
  geoBoundaries file (shared vertices) segments count as shared only within 50 m, so
  a narrow notch of the neighbour stays part of the outline instead of leaving a gap.
  Snapshot of the state-only version: branch `snapshot/2026-10-22-state-borders`.

**13.14 Cleaner municipal country borders.** An outline segment of the drawing country
becomes country border when it runs within 5.5 km of a Natural Earth land-border line
naming both countries (13.12). At 26.3° N, 89.0° E this drew large loops and small
blobs, which looked wrong. Checked on the real files (2026-10-23):

- **The large loops are the real border.** Bangladesh's Patgram salient reaches into
  India's Cooch Behar, and both countries' files agree on it: along most drawn
  segments a Bangladeshi union lies on one side and an Indian district on the other.
  Natural Earth's 1:10m line simplifies this away (it puts both sides in India at
  hundreds of places), which is why the accurate line looked odd.
- **The stray blobs were land both files claim:** union parts in Bangladesh's file
  (Kuchlibari, Baura, Jagatber, Jongra, Dahagram) that India's file also covers. Most
  are pockets exchanged in the 2015 land swap that Bangladesh's file still holds.
  Dahagram is the exception: it is still Bangladeshi, but India's file and Natural
  Earth both put it in India, so no source at hand tells it apart.
- **Rule (done):** the drawing side does not draw a country border around land that the
  neighbour's file also covers (a point inside the ring is covered by both countries'
  rings, even-odd, so a hole that a real enclave fills is not contested). That outline
  stays a municipal line; the other side's copy is handled as before, so no line is
  drawn twice. Results: 21 → 7 border chains at the screenshot spot (569 of 634 km
  kept), 61 → 42 along the whole Bangladesh–India border; Czechia, Poland, Austria,
  Slovakia, Italy and Baarle-Nassau (real nested enclaves) unchanged. Dahagram shows
  as a municipal outline, not a country border.
- **Rejected:** a minimum line length (the good Central European borders have more
  short chains than this border: 47 under 10 km, 109 km in total, against 28 and 78 km)
  and "the far side must be the neighbour" (the two files leave thin gaps along the
  real border, and the rule cut it there).
- **Small islands:** Fais (9.76° N, 140.52° E) is in geoBoundaries and Natural Earth.
  Islands get no line because GlobeCraft never draws coastlines (only land borders),
  not because of this rule.

**13.15 Country borders, measured on every land border.** Natural Earth's line for each
pair is sampled every kilometre (`scripts/border-audit/README.md`, removed with the old pipeline on 2026-10-25). Totals over 283
pairs and 198,666 km, before and after (2026-10-23):

| Stretch | Before | After |
| --- | --- | --- |
| Replaced by the municipal border | 180,896 km (91.1 %) | 192,566 km (96.9 %) |
| Doubled (coarse kept beside the municipal border) | 9,367 km (4.7 %) | 1,425 km (0.7 %) |
| Coarse kept although municipal outlines are near | 7,706 km (3.9 %) | 4,370 km (2.2 %) |
| Coarse kept, no municipal data | 693 km | 300 km |
| Lost (removed, nothing in its place) | 4 km | 7 km |
| Breaks (a drawn line ending in nothing) | 2,901 | 2,107 |

Czechia, Poland, Austria, Slovakia and Italy: state lines identical; their country
lines lost nothing visible (13 km of Gulf of Trieste coast is no longer drawn as the
Italy–Slovenia border).

What each rule fixed, with an example from the audit:

- **Confirmed edges up to 16 km from Natural Earth** (`borders::CONFIRMED_BAND_DEG`):
  where both countries' outlines coincide but Natural Earth runs 5–10 km away (the Andes,
  Central Asia, the Sahara), the border was demoted to municipal lines and broke into
  pieces. Argentina–Chile at 27.5° S became continuous.
- **Per-border cut** (`topology::CountryCells`): within the old 3.3 km the coarse line
  stayed beside the accurate one wherever the sources differ more. Kazakhstan–Russia went
  from 423 km of doubled line to none; the Sokh enclave (Fergana valley) shows one outline
  instead of two.
- **Bridged detours only** (`handover::DETOUR_REACH_DEG`): the coarse line across the
  Southern Patagonian Ice Field, where neither file reaches, was dropped with nothing in
  its place.
- **Short holes bridged** (`borders::BRIDGE_KM`): where the neighbour's outline bends
  away for a few hundred metres, the border had holes (Iran–Pakistan near 26.3° N).
- **The other side fills gaps**: Mozambique's file has no line along part of the
  Eswatini border; Eswatini's outline draws it now.
- **Disputed land stays undrawn on both sides**, including through the fallback (Aksai
  Chin, Kashmir): there Natural Earth's line stays.
- **A border has land on both sides** (`borders::Land`, from Natural Earth's country
  outlines): an outline segment the neighbour's outline does not confirm is a border
  only with land about 1 km to both sides, so coasts near a border line are no longer
  drawn as border (the Beagle Channel: Argentina–Chile breaks 123 → 48).
- **The fallback needs the drawing side's own land across** (within 5 km, in its file):
  where that file has no land at all, the other side's shore is no shared border. The
  Ukrainian shore of the Syvash had been drawn along Natural Earth's de facto line
  (Russia–Ukraine breaks 17 → 80 → 13).

Remaining: stray municipal country lines (over 10 km from any Natural Earth line) are
mostly places where both countries' files agree and Natural Earth is off (Bolivia–Peru
along the Heath River, 10–13 km). Borders with a national source (Germany, France,
Spain, the US, Canada, Brazil) are not in the audit: their servers are not reachable
from the build container. A municipal line offset from the imagery (user, 71.98° N,
80.14° E, Yamalo-Nenets / Krasnoyarsk) is the source data: geoBoundaries has a vertex
every 0.5 km there and traces the river's meanders, but beside them. A projection error
would shift lines everywhere, not at one spot.

## 14. Size and efficiency review (2026-10-22)

Where GlobeCraft stands, and how much room there is.

**Code.** About 52,000 lines of Rust: app 21 k, data 14.7 k, render 7.6 k, core 5.2 k,
assistant 3.4 k. 535 crates in the dependency tree. The structure is sound (crate
boundaries, workers, bounded caches); the remaining weight is breadth, not waste.

**Binary.** The stripped release build is about 45 MB. Roughly 5.5 MB of it is embedded
assets (Milky Way 2.5 MB, Natural Earth 1.8 MB, Moon 0.4 MB, easter egg 0.4 MB); most of
the rest is the wgpu/naga stack (used for presentation and the experimental GPU
renderer), TLS, async I/O, SQLite and the gamepad, dialog and updater stacks.

**Frame cost.** Interface frame 0.44 ms. The overlay rebuild during a descent is now
under 2 ms of real work. The dominant visual cost is the CPU globe raster: 11–16 ms
for a 512×384 frame on the 2-core container, more for full-window frames and with 3D
terrain (34–42 ms).

| # | Opportunity | Expected gain | Size |
| --- | --- | --- | --- |
| 14.1 | **GPU globe by default where it is proven** (8.2): the raster moves off the CPU entirely | The largest single gain: frame cost from ~10–40 ms CPU to a few ms GPU, and lower power use | XL |
| 14.2 | **Release profile**: `lto = "fat"`, `codegen-units = 1` for published builds (release workflow and Flatpak); local `--release` keeps thin LTO for quick rebuilds. `panic = "abort"` is ruled out: the assistant and background tasks recover from panics. **Done 2026-10-22**, size gain to be measured on the next release | Typically 10–20 % smaller and a few % faster | S |
| 14.3 | **Lighter presentation**: build without the GPU renderer feature by default until 14.1 lands, or use eframe's glow backend for CPU builds | Likely 8–15 MB smaller and faster startup | S–M **Measured 2026-10-25, not pursued:** the release binary is 44.96 MB with the GPU renderer and 44.77 MB without it (thin LTO, stripped, Linux). wgpu is needed for presentation either way; only eframe's glow backend would drop it, and that blocks 14.1 |
| 14.4 | **Download big assets on first use** (Milky Way, Moon, easter egg) instead of embedding them, keeping a tiny fallback | ~3 MB smaller binary | S |
| 14.5 | **Fewer, smaller dependencies**: audit the 535-crate tree (`cargo tree -d` duplicates, unused features of `self_update`, `image`, `reqwest`, `tokio`) | Faster builds, a few MB | M |
| 14.6 | **Idle cost**: no repaint while nothing changes; measure idle CPU on the Mac and cap background refreshes | Battery life | S–M |
| 14.7 | **Documentation diet**: merge CONTRIBUTING, AGENTS and the per-feature docs into fewer, shorter pages; keep the performance log as data, not prose | A repository newcomers can read in an hour | M |
| 14.8 | **App state split** (10.2) and the vector rebuild (3.3): fewer special cases in `load.rs` and `App` | Easier changes, fewer bugs | L |
| 14.9 | **Municipal border classification** (review 2026-10-23): smaller search cells (0.02° instead of 0.05°), only the neighbours' still-unshared segments searched, rings classified in parallel on the 2-thread feature pool. The search also padded longitude by the latitude tolerance, missing a match now and then near cell edges at higher latitudes (one segment in Italy); fixed. **Done 2026-10-23**, output checked byte for byte on CZ, PL, AT, SK, IT, IN and BD | 0.33–1.51 s → 0.15–0.55 s per country load (table in PERFORMANCE_BASELINE) | S **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 14.10 | **Municipal publishing off the store lock** (review 2026-10-23): the publisher assembled the layer (10–45 ms) while holding the lock the UI thread takes every frame, so panning with municipal data could stall frames four times a second. Units are now shared (`Arc`), the publisher snapshots them and assembles on the feature pool. **Done 2026-10-23** | No frame waits on a municipal publish | S **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 14.11 | **Reuse the country-border decision between publishes**: `borders::resolve` reruns for every publish (15–32 ms on the feature pool) even when only the area moved. Cache each ring's result by the set of loaded countries and the border index | Less CPU while panning; no visible change since 14.10 | S **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |
| 14.12 | **Experimental GPU renderer ignores `lod_scale`**: `overview.wgsl` picks the level from its own raster height, so motion frames at 0.4 scale sample a coarser level than idle frames (the strobing fixed for the CPU path on 2026-10-23). Pass the level-of-detail height as a uniform before 14.1 | Required for 14.1 | S. **Done 2026-10-23**: the level-of-detail height travels in a spare uniform slot (`effects.z`), and the host zoom check uses it too |
| 14.13 | **Stable run order**: `assemble` walks hash maps, so runs come out in a different order after each publish. Sorting rings by id would make the output repeatable (easier diffs, steady dash phase) | Determinism | S. Tried 2026-10-23 (sorted snapshot, rings in first-seen order) and **reverted together with 13.18** while borders are rebuilt; redo after the border review. **Superseded 2026-10-25** by 13.19: borders and names come from OpenStreetMap tiles; this code was removed (branch `snapshot/2026-10-25-before-cleanup`) |

Overall: binary size can probably drop by a third to a half without losing features
(14.2–14.5), and the CPU-bound visual cost can drop by an order of magnitude only
through the GPU path (14.1). Everything else is incremental.

## Suggested order

1. Section 0: stabilise and verify the rewrite on the Mac (0.1, 0.2, 0.4 need the Mac; 0.6, 0.7 done). Borders are settled on OpenStreetMap tiles (13.19 phases 1–2 done 2026-10-25), so the municipal items in sections 3 and 13 are closed.
2. 1.1 replay benchmark, so there is a baseline.
3. Foundation, in this order: 1.2 generic `TileSource` (with night lights, 3.2, as the pilot source), 1.3 scheduling and cancellation, 1.5 tile processing, 1.4 predictive requests, 1.6 seamless arrival, 1.11 diagnostics. Re-measure after each.
4. 1.7 coordinate streaming, 1.8 smooth live objects, 1.9 precision and 1.10 elevation; the municipal redesign (3.1), PMTiles (3.4) and the vector rebuild (3.3) ride on 1.7.
5. Section 2, Add Data: the dialog, forms, service library with the master table's presets, user layers, then the Browser panel.
6. Adapters that are still missing (section 4: time, points, key manager), then the layers in section 5 as presets.
7. In parallel when it fits: 10.1 CI and 10.2 `App` split.
8. Afterwards: 8.2 GPU beta (8.1 terrain ray marching done on the CPU 2026-10-26); then wind, 3D buildings and the sky mode.

The previous v0.2.0 delivery plan is preserved in `docs/releases/v0.2.0.md`.
