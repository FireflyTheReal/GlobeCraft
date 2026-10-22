# GlobeCraft roadmap

Last revised 2026-10-22: GlobeCraft rename, second batch of user requests (section 13),
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
| 0.5 | Move the root `DATA_SOURCES.md` and `data_sources_triage.csv` into `docs/` (the new one replaces the old `docs/DATA_SOURCES.md`), with the user's OK | S | Plan housekeeping; the two copies have drifted |
| 0.6 | Replace the emoji glyphs still used as icons with the vector icon set, or drop them where an icon is already drawn | S–M | Affects "💾 Save", "📂 Open KML/KMZ…", the globe context menu, the Explorer worlds and "⏳" statuses. The localisation fixture pins the old strings, so update it deliberately |
| 0.7 | Add a command for the state-border altitude (`layer.stateAltitude`) so the assistant and Command Search can change it | S | The setting exists only as a pane control |
| 0.8 | First GlobeCraft release (0.3.0) notes with the "what changed" screenshots, from the new repository `FireflyTheReal/GlobeCraft` | S | `docs/releases/`. English only: the repository has no translated documents |
| 0.9 | Re-shoot the README screenshot (`assets/repository/globecraft-app.png`) with the GlobeCraft name on screen | S | The current image still shows the old title |

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
| 1.1 | **Streaming replay benchmark.** Recorded camera paths (fly across the globe, zoom into a city, drag-pan, orbit) against a local fixture tile server that simulates latency and bandwidth per host. It reports time to first complete view, time to sharp (every visible tile at its target level), hole pixels per frame, wasted downloads (fetched but never shown), bytes, and frame times | M | "Perfect" needs a number. This is the yardstick for every item below; run it before and after each one and keep the results in `PERFORMANCE_BASELINE.md` |
| 1.2 | **Generic `TileSource`** in data: id, kind (raster imagery, elevation, vector), access (XYZ/TMS template, WMTS REST, WMS GetMap by tile bbox, PMTiles, local MBTiles), format, zoom range, 256 or 512 px tiles, projection, attribution and licence, prefetch policy (OSM stays on demand), key reference, optional time dimension. `TileKey` gets a 16-bit source id and a time index; the disk cache keys by source identity | L | Replaces the closed `Provider` enum. Night lights (3.2) is the pilot: the first source added as data, not code |
| 1.3 | **Scheduling and cancellation**: priority by screen distance and level (coarse first), cancel downloads that leave the view, per-host concurrency (HTTP/2 multiplexing vs HTTP/1.1 hosts), per-host backoff honouring 429 and `Retry-After`, conditional revalidation (`ETag`, `Cache-Control`) for disk-cached tiles | M | The bandwidth goes to what the user will see next |
| 1.4 | **Predictive requests**, always below visible work: coarse tiles along a fly-to path and around its destination (the path is known in advance), extrapolation of drag and inertia velocity, and a ring one level coarser around the viewport | M | Arrive with the picture ready instead of watching it fill in |
| 1.5 | **Tile processing**: build a half-resolution level at decode time so minified sampling is one bilinear tap on it instead of the area filter; native 512 px tiles; WebP/AVIF; decode cost and bytes accounted per source | M | Faster and the same quality; the filter stays for stride above two |
| 1.6 | **Seamless arrival**: a short time-based cross-fade when a finer tile lands (today it pops in), the existing parent fallback kept, consistent colour across levels | S–M | Removes the last visible "loading" artefacts |
| 1.7 | **Coordinate streaming** for every geographic coordinate: borders, labels, roads, places, live objects. Request vector data by viewport tile and zoom (MVT/PMTiles or bbox APIs); clip and simplify per zoom off the UI thread; keep stable feature ids so updates are diffs, not replacements; project incrementally (only changed tiles) and keep reusing cached meshes, as the borders now do | L–XL | One streaming model instead of per-dataset loaders. It carries the municipal redesign (3.1) and the vector rebuild (3.3) |
| 1.8 | **Smooth live objects**: time-stamped samples with dead reckoning between updates (aircraft heading and speed, vessel course and speed, satellites by SGP4) so positions move at frame rate; fade by sample age | M | Today aircraft and ships jump every refresh |
| 1.9 | **Precision at close range**: camera-relative coordinates for overlays and picking, so lines and markers do not jitter when zoomed into a street | S–M | Needed before 3D buildings and street-level work |
| 1.10 | **Elevation as a first-class tile source**: its own level of detail, shared by terrain, hill shading, cursor elevation, the ruler's elevation profile and picking | M | One DEM pipeline instead of terrain-only lookups |
| 1.11 | **Streaming diagnostics overlay** (toggle): tile grid, level per tile, pending, failed and cached states, bytes per second and queue length per host | S–M | Makes tuning visible on the Mac, not only in benchmarks |

**Exit criteria.** On the replay benchmark:

- no grey holes after a fly-to lands;
- time to sharp and wasted downloads clearly below the first baseline;
- CPU frame and interface frame times no worse than today.

Set the exact targets from the first baseline run (1.1) rather than guessing them now.

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
| 3.0 | **State/province streaming and fade** (reported 2026-10-22): state and province lines arrive too late, so they pop instead of fading, especially with a high "States visible below" setting; drawing them feels laggy | M | **Implemented 2026-10-22, needs a check on the Mac.** See 3.0 below. First slice of coordinate streaming (1.7) |
| 3.1 | **Municipal redesign**: fade from state lines to municipal lines, fetching only the municipalities in view at a reasonable altitude, with no stutter while panning (see 3.1 below) | L | **First slice implemented 2026-10-22** (streaming units, see below); needs a check on the Mac, then per-source tuning |
| 3.2 | **Night lights layer** from NASA GIBS "Earth at Night" (`IMG-03` [16.3]). Black Marble tiles in WMTS `epsg3857` with a fixed date; blended by the sun terminator when Sunlight is on, plain otherwise | M | Wanted by the user. Pilot source for the generic `TileSource` (1.2), plus a blend term in `pixel.rs`. Check the layer identifier and date with GetCapabilities first. `IMG-24` (raw VIIRS) is a pre-bake, so skip it |
| 3.3 | **Vector rebuild, steps 2–5** of `docs/features/VECTOR_MAP_DATA.md` (see 3.3 below) | XL | Unlocks Overture divisions (`REF-18`) and every future vector layer without one-off wiring |
| 3.4 | **PMTiles v3 reader** (`pmtiles` crate, HTTP range reads) with its own byte-bounded vector cache, separate from the 256 MiB imagery budget | M | Shared adapter C. Unlocks Mapterhorn as PMTiles (`TER-09`), Protomaps (`IMG-14`), building footprints (`BLD-01`), field parcels (`LND-05`) and EEZ borders (`MAR-03`) |
| 3.5 | **Polar view**: a dedicated polar projection (GIBS epsg3413/3031) in place of the `PolarCap` interim | L | The user prefers a real pole map. Needs per-provider projection in the tile path (`proj4rs`) |
| 3.6 | Disputed borders: a per-country "worldview" choice (Natural Earth map units and geoBoundaries) | M | "As the country represents itself" is a common request and the data is already there |
| 3.7 | Trim the boundary log to the last N lines in the data crate, not only in the pane | S | The pane now shows the last 12 lines; the log itself still grows |

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

Implemented first slice (2026-10-22): municipal data now streams in *units* around the view, nearest first, from about 640 km down (before the 402 km fade): national grid cells with stable requests (FRA 0.25°, others 1°, each within a per-source radius), one Brazilian state at a time from IBGE instead of the whole country, whole-country geoBoundaries files clipped to the view, and OSM cells as the fallback. A bounded store keeps loaded units, so panning back is instant, and a throttled publisher adds cells to the layer instead of swapping it after every request. State lines stay under the municipal outlines. Still open: (b) size-ranked labels for every source, per-country visible altitudes for dense countries (French communes cover only about 0.75° around the centre), and a bounded on-disk cache for cells.

**3.3 Vector rebuild.** The rewrite already did half of step 4: `MapLayer` and `LayerSet` replace the per-layer booleans in the UI. Next:

- the data-side seams (`LayerId`, `LayerSpec`, `TileKey`, `LayerTile`, `MapSnapshot`, `CountryIndex`);
- per-source adapters instead of the 693-line `load.rs`;
- per-tile refresh, then cross-fade.

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
| 6.11 | Touch and trackpad: pinch to zoom, two-finger rotate and tilt, inertia tuned for Mac trackpads | S–M | Mac users expect these gestures |
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
| 8.1 | **Terrain occlusion**: true heightfield ray marching (today it is a 2.5D displacement with no occlusion), at least for close views | L | The next visible quality jump. Do it on the GPU path first |
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
- **Poster export (M):** high-resolution offscreen renders with a title, scale and attribution block.
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
| 13.1 | **Municipal takeover and staging** (3.1 d): municipal outlines replace the coarse state and country lines first, where municipalities sit on those borders; interior municipalities and their names appear only at a narrower zoom, biggest first | M | **First slice implemented 2026-10-22.** Segments are classified per loaded unit (state border, interior, shared); state-border segments draw from 402 km, interior lines and small names only below an altitude set by the municipalities' mean size (about 75 km for Germany), and shared borders are drawn once instead of twice. Still open: country borders across two national sources (the units never see each other), fading the Natural Earth state lines out where municipal state borders cover them, and checking on the Mac |
| 13.2 | **One header row**: merge the logo and Quick Access Toolbar row with the File / Home / Layers… tab row; drop the "placemarks.kml — Earth" title | S | **Done 2026-10-22.** Logo, File, tabs, Quick Access Toolbar, both searches and the collapse button share one 38-pt row; the file and body names moved to the window title |
| 13.3 | **All planets and moons, streamed**: NASA Solar System Treks WMTS (Moon, Mars, Mercury, Vesta, Ceres, Titan…), USGS Astrogeology and GIBS where they serve tiles; one `TileSource` per body (1.2), with elevation where available (1.10) | M–L | Today the Moon and custom planets are single images. Credits per body in CREDITS.md |
| 13.4 | **Borders in the dark**: when Night lights / sunlight are on, draw land borders at full strength all around the night side (today they follow the lit-side styling) | S | Pairs with 3.2 and 8.4 |
| 13.5 | **Navigator in the Fusion 360 style**: a small view cube or mini globe that shows orientation, click a face or pole to snap, with compact move and zoom controls beside it | M | Replaces the large ring, pad and slider stack |
| 13.6 | **Small, simple live symbols**: one minimal glyph per category for aircraft (by size class), ships (by AIS ship type: cargo, tanker, passenger, fishing, sailing…) and satellites (by group); satellites drawn at their real altitude above the globe | M | Uses the category fields the feeds already send. Pairs with 1.8 smooth motion |
| 13.7 | **"Map data not yet available" at maximum zoom**: detect Esri's placeholder tiles (they come back as normal images) and treat them as missing, so the deepest real level is upsampled instead (overzoom) | S–M | **Done 2026-10-22.** Requests carry `blankTile=false`, so Esri answers 404 where it has no tile; 404s and placeholders already in the disk cache mark the tile unavailable for the session, and renderers show the deepest real level without waiting or re-requesting |
| 13.8 | **Style settings**: colour, thickness and dash pattern per line type (country, state, municipal), label sizes per kind, and a reset button | M | All values are named constants already; they become settings with commands |
| 13.9 | **Wayback**: a drop-down calendar of the available release dates, and a **swipe compare** of two dates with a slider across the globe | M | The release list is already loaded; compare needs two imagery requests per frame |
| 13.10 | **Remove the metropolitan regions layer**: municipal borders cover it. Optionally, a country picker for municipal borders (only the selected countries load) | S | **Removal done 2026-10-22** (layer, IBGE metro pass, Overture metro slices and their scripts). The country picker is still open |
| 13.11 | **GlobeCraft repository**: fresh history, English only, straightforward README for non-technical users, every source credited in CREDITS.md | S | Done 2026-10-22 |

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
| 14.3 | **Lighter presentation**: build without the GPU renderer feature by default until 14.1 lands, or use eframe's glow backend for CPU builds | Likely 8–15 MB smaller and faster startup | S–M |
| 14.4 | **Download big assets on first use** (Milky Way, Moon, easter egg) instead of embedding them, keeping a tiny fallback | ~3 MB smaller binary | S |
| 14.5 | **Fewer, smaller dependencies**: audit the 535-crate tree (`cargo tree -d` duplicates, unused features of `self_update`, `image`, `reqwest`, `tokio`) | Faster builds, a few MB | M |
| 14.6 | **Idle cost**: no repaint while nothing changes; measure idle CPU on the Mac and cap background refreshes | Battery life | S–M |
| 14.7 | **Documentation diet**: merge CONTRIBUTING, AGENTS and the per-feature docs into fewer, shorter pages; keep the performance log as data, not prose | A repository newcomers can read in an hour | M |
| 14.8 | **App state split** (10.2) and the vector rebuild (3.3): fewer special cases in `load.rs` and `App` | Easier changes, fewer bugs | L |

Overall: binary size can probably drop by a third to a half without losing features
(14.2–14.5), and the CPU-bound visual cost can drop by an order of magnitude only
through the GPU path (14.1). Everything else is incremental.

## Suggested order

1. Section 0: stabilise and verify the rewrite on the Mac. Now: 13.1 municipal takeover, then the quick wins 13.2, 13.7, 13.10 and 14.2–14.4.
2. 1.1 replay benchmark, so there is a baseline.
3. Foundation, in this order: 1.2 generic `TileSource` (with night lights, 3.2, as the pilot source), 1.3 scheduling and cancellation, 1.5 tile processing, 1.4 predictive requests, 1.6 seamless arrival, 1.11 diagnostics. Re-measure after each.
4. 1.7 coordinate streaming, 1.8 smooth live objects, 1.9 precision and 1.10 elevation; the municipal redesign (3.1), PMTiles (3.4) and the vector rebuild (3.3) ride on 1.7.
5. Section 2, Add Data: the dialog, forms, service library with the master table's presets, user layers, then the Browser panel.
6. Adapters that are still missing (section 4: time, points, key manager), then the layers in section 5 as presets.
7. In parallel when it fits: 10.1 CI and 10.2 `App` split.
8. Afterwards: 8.1 terrain occlusion and 8.2 GPU beta; then wind, 3D buildings and the sky mode.

The previous v0.2.0 delivery plan is preserved in `docs/releases/v0.2.0.md`.
