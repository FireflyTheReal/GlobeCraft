# How borders and place names are drawn

Since 2026-10-25 every border line and most place names come from OpenStreetMap,
through OpenMapTiles vector tiles served by [OpenFreeMap](https://openfreemap.org/).
The only other data is the Natural Earth 1:110m set compiled into the data crate,
which draws the world before the first tile arrives and answers "which country is
this point in". The older pipeline (Natural Earth 1:50m/1:10m downloads, national
municipal services, geoBoundaries, Overpass, and the matching between them) was
removed; it is kept on the branch `snapshot/2026-10-25-before-cleanup`.

## 1. Sources

| Data | Source | Code |
| --- | --- | --- |
| Countries, outlines for lookup, major cities | Natural Earth 1:110m, bundled | `boundaries/load.rs`, `boundaries/sources.rs` |
| Every border from country to municipality, disputed lines and claims | OpenMapTiles `boundary` layer | `boundaries/osm_tiles` |
| Cities, towns, villages, hamlets, islands, state names | OpenMapTiles `place` layer | `boundaries/osm_tiles/content.rs` |

OpenStreetMap stores each border once, shared by the areas on both sides, and
OpenMapTiles gives each line the lowest administrative level it belongs to. So no
border is drawn twice and nothing has to be matched between two surveys. Each zoom
level is generalised for its scale, so switching level is like switching imagery
levels: there is no coarse-to-detailed handover.

## 2. Which tiles (`osm_tiles/pick.rs`, `osm_tiles/mod.rs`)

- The level follows the altitude, about four tiles across the view, at most zoom 12
  (deeper tiles add only buildings and roads) and at most 96 tiles.
- A tile that is still missing is drawn from its nearest loaded ancestor, cut to its
  area, so the view never empties.
- Loaded ahead: every ancestor of the view, a ring of one tile around it, a block as
  wide as the view at the next three levels up (a quick zoom out of 8×), and the
  whole world to zoom 3.
- Tiles to zoom 6 stay in memory; the rest are bounded by 3 million vertices, least
  recently used dropped first. Tiles loaded ahead count as used.
- Up to 4 downloads (OpenFreeMap's limit for bulk clients) and 8 disk reads at once.

## 3. What is kept (`osm_tiles/content.rs`, `osm_tiles/compact.rs`)

Only land borders and names are decoded from a tile; maritime lines are dropped.
Each line keeps its raw `admin_level` and three flags: disputed, claim
(`claimed_by` present) and country edge (`adm0_l`/`adm0_r` present). Lines cut at
tile edges are joined again (`join.rs`), so dashes run on without restarting.

| `admin_level` | Drawn as | Button |
| --- | --- | --- |
| 2 | Country | Countries |
| 3–4 | State (thicker inside the largest countries) | States |
| 5–8 | Municipal, from zoom 11 (about 40 km) | Municipal borders |

**Disputed borders.** The neutral view (default) draws one line: the de facto
border, dashed where it is disputed. A claim line (claim flag, no country edge) is
drawn only with *Settings → Map style → Show both countries' claims on disputed borders*. The de facto
line also carries `claimed_by` because OpenMapTiles merges each OSM way, so the
country-edge flag is what tells the two apart.

**Names.** City ranks come from OpenMapTiles' `rank` (minus one, 0–8; unranked 8);
towns, villages and hamlets follow at ranks 11, 12 and 13, so small names appear
with the municipal lines. Tile names replace each other on every update; the bundled
names stay underneath as fallback.

## 4. Disk cache (`osm_tiles/store.rs`)

Tiles are stored in the compact format (format version 3) in `osm_borders.sqlite`
in the cache folder, bounded by *Settings → Cache → Border tile cache on disk* (64 MiB default,
8–1024 MiB, applies at once). The least recently used tiles are deleted first
(`auto_vacuum=FULL`, so the file shrinks). The disk is read first; the network is
used only for missing tiles or tiles of an older tile set, which stay shown until
replaced. The tile URL template is cached too, so an offline start still finds its
tiles.

## 5. Drawing (`globecraft-render/src/overlays/mod.rs::project_borders`)

With no tiles loaded, the bundled Natural Earth lines are drawn as country borders
(or as state lines when only the state button is on). Once tiles arrive, only tile
lines are drawn, by the table above. Lines are projected, simplified above
`SIMPLIFY_BORDER_ALTITUDE_KM` and dashed off the UI thread; new tiles fade in.
