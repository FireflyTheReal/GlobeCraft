# Add Data

Layers the user adds: web maps and points and lines, next to GlobeCraft's own layers.
Roadmap 2.1–2.4.

## Opening it

- Ribbon → Layers → My data → **Add Data**, File → **Add Data…**, Command Search, or
  Layers pane → My data → **Add Data…** (`ui.addData`).
- The assistant uses `data.add` with a library `preset`, or a `url` and its `kind`
  (`tiles`, `wms`, `features`). Commands take web addresses only; files on this computer
  are added through the window.

## The window

| Tab | What it adds | Notes |
| --- | --- | --- |
| Library | Checked keyless sources (`user_data/presets.rs`) | Licence, commercial use and longevity under each. `data-sources.yml` fetches a sample from each |
| Web map | XYZ or WMTS REST tiles; a WMS | Template placeholders: `{z}`, `{x}`, `{y}`, `{-y}` (TMS), `{bbox}` (EPSG:3857 metres, for WMS), `{date}` (yesterday, UTC). WMS: Load layers reads GetCapabilities. Test fetches the tile under 0°, 0° at level 2 |
| Points and lines | GeoJSON, GPX, CSV from a file or URL | CSV finds `lat`/`latitude`/`y` and `lon`/`lng`/`longitude`/`x` columns and `,` `;` tab or `|`. A URL can reload every N minutes. Test reads it and counts points and lines |

KML and KMZ open into My Places (Open KML…).

## Layers pane

My data lists the layers top of the drawing order first: visibility, opacity, up and
down, zoom to the area (feature layers), remove, and the credit. Loading and errors show
under the layer. The list is saved as `user_layers` in `settings.json`.

## Drawing

- Web maps (`user_data/raster.rs`): tiles for the view, at most 8 downloads at a time
  and 384 tiles kept per layer; coarser loaded tiles stand in. All visible web maps are
  projected into one image (`weather::project_user_rasters`, 1,280 pixels wide, 640
  while dragging) drawn over the imagery and under the borders.
- Features (`user_data/vector.rs`): up to 2 million vertices; lines clipped to the view,
  points with names kept 70 points apart, at most 150 names per layer.

Limits: Web Mercator only (no polar views), no MBTiles, PMTiles, OGC API or ArcGIS REST
yet, no legends.
