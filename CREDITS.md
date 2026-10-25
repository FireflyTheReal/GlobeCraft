# Credits

GlobeCraft only shows the world because many people and organisations publish their
maps, data and code openly. Thank you. Please respect each service's terms of use;
the app shows the matching attribution on screen while a source is in use.

## Project

- **GlobeCraft** by Julien Foss ([@FireflyTheReal](https://github.com/FireflyTheReal)), MIT licence.
- Based on **[EarthCraft](https://github.com/therichronicles/EarthCraft)** by
  @therichronicles and @PixelSoftwareStudios (MIT). Its copyright notice is kept in
  [LICENSE](LICENSE).

## Imagery and terrain

| Source | Used for | Terms |
| --- | --- | --- |
| [Esri World Imagery](https://www.arcgis.com/home/item.html?id=10df2279f9684e4a9f6a7f08febac2a9) — Esri, Maxar, Earthstar Geographics and the GIS user community | Satellite basemap | Esri terms of use; attribution shown in the app |
| [Esri World Imagery Wayback](https://livingatlas.arcgis.com/wayback/) | Historical imagery by date | Esri terms of use |
| [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors | Streets basemap, place details, road geometry (Overpass) | ODbL; [tile usage policy](https://operations.osmfoundation.org/policies/tiles/) |
| [OpenTopoMap](https://opentopomap.org/about) | Topographic basemap | CC BY-SA; data © OpenStreetMap contributors, SRTM |
| [Mapterhorn](https://mapterhorn.com/) | 3D terrain elevation | See the Mapterhorn attribution page (Copernicus DEM and national elevation models) |
| [NASA SVS 4851](https://svs.gsfc.nasa.gov/4851/) — NASA/Goddard Space Flight Center Scientific Visualization Studio | Milky Way background (bundled, reduced) | NASA media guidelines; see [assets/sky/README.md](assets/sky/README.md) |
| NASA/GSFC/Arizona State University, LRO Camera (LROC) | Moon colour map (bundled, 2k) | NASA media guidelines |

## Borders and place names

| Source | Used for | Terms |
| --- | --- | --- |
| [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors | Every border from country to municipality, disputed lines, place names | ODbL |
| [OpenMapTiles](https://openmaptiles.org/) schema, served by [OpenFreeMap](https://openfreemap.org/) | Vector tiles carrying those borders and names | OpenMapTiles CC BY 4.0; OpenFreeMap free service, attribution shown in the app |
| [Natural Earth](https://www.naturalearthdata.com/) | Countries and major cities (1:110m, bundled), shown before the first tile arrives and used to find the country under the camera | Public domain |

## Search

| Source | Used for | Terms |
| --- | --- | --- |
| [Nominatim](https://nominatim.org/) (OpenStreetMap) | Place and address search | ODbL; [usage policy](https://operations.osmfoundation.org/policies/nominatim/) |
| [Photon](https://photon.komoot.io/) by komoot | Search suggestions while typing | ODbL data; fair use |
| [Wikimedia Commons](https://commons.wikimedia.org/) | Photo search (opens in the browser) | Per-file licences |

## Live layers

| Source | Used for | Terms |
| --- | --- | --- |
| [OpenSky Network](https://opensky-network.org/) | Aircraft positions | Non-commercial use, rate-limited; attribution |
| [ADSB.lol](https://www.adsb.lol/) | Aircraft positions near you | ODbL |
| [CelesTrak](https://celestrak.org/) | Satellite orbital elements | Free to use with attribution |
| [OpenSeaFeed](https://openseafeed.com/) | Ship (AIS) positions | Service terms |
| [Fintraffic Digitraffic](https://www.digitraffic.fi/) | Finnish ship positions (fallback) | CC BY 4.0 |
| [RainViewer](https://www.rainviewer.com/) | Precipitation radar | Free for personal, educational and small community use; attribution |
| [Matt Eason's cloud map](https://clouds.matteason.co.uk/) (from EUMETSAT and other satellite data) | Global clouds | Attribution |
| [TomTom](https://developer.tomtom.com/) | Traffic flow (your own API key) | TomTom terms |

## AI assistant providers

The assistant talks only to the provider you choose, with your own key: Anthropic,
OpenAI, Google Gemini, OpenRouter, Ollama or any OpenAI-compatible server.

## Code

GlobeCraft is built with Rust and many open-source crates (egui/eframe, glam, rayon,
reqwest, tokio, rstar, geo-index, fluent, kml, rusqlite and others); their licences are
listed by `cargo about` or in each crate. [sofars](https://crates.io/crates/sofars)
(IAU SOFA routines) is licensed as described in
[assets/sky/sofars-LICENSE.txt](assets/sky/sofars-LICENSE.txt).

### RustGIS

GlobeCraft's interface design and AI assistant are adapted from
[RustGIS](https://github.com/opengeos/RustGIS) (commit `446bc512`), © the RustGIS
contributors, licensed under the Apache License, Version 2.0. The full licence text is in
[licenses/RustGIS-Apache-2.0.txt](licenses/RustGIS-Apache-2.0.txt).

Files containing adapted RustGIS code say so in their module documentation:

| GlobeCraft file | Adapted from (RustGIS) | Changes |
| --- | --- | --- |
| `crates/globecraft-assistant/src/provider.rs` | `crates/assistant/src/provider.rs` | Removed the OpenAI Responses API, the ChatGPT sign-in and the Anthropic server-side fallback; GlobeCraft presets |
| `crates/globecraft-assistant/src/compact.rs` | `crates/assistant/src/compact.rs` | GlobeCraft summary prompt; Responses reasoning removed |
| `crates/globecraft-assistant/src/agent.rs` | `crates/assistant/src/agent.rs` | Rewritten around a `Host` trait; no subagents, skills, local agents or MCP |
| `crates/globecraft-assistant/src/tools.rs` | `crates/assistant/src/tools.rs` (output budgeting) | GlobeCraft tools over the command catalog |
| `apps/globecraft/src/app/ui/theme.rs` | `crates/ui-egui/src/theme.rs` | GlobeCraft brand colour, globe canvas, scalable text |
| `apps/globecraft/src/app/ui/icons.rs` | `crates/ui-egui/src/icons.rs` | RustGIS logo removed; globe, live-layer and GlobeCraft icons added |
| `apps/globecraft/src/app/ui/widgets.rs` | `crates/ui-egui/src/widgets.rs` | Commands resolved from GlobeCraft's catalog |
| `apps/globecraft/src/app/ui/ribbon.rs` | `crates/ui-egui/src/ribbon.rs` | GlobeCraft tabs and groups |
| `apps/globecraft/src/app/ui/chrome.rs` | `crates/ui-egui/src/chrome.rs` | Place search and GlobeCraft status bar |
| `apps/globecraft/src/app/ui/panes.rs` | `crates/ui-egui/src/panes.rs` | Layers/Places/Globe and Assistant/Measure panes |
| `apps/globecraft/src/app/ui/palette.rs` | `crates/ui-egui/src/chrome.rs` (Command Search) | Layers, basemaps and worlds as entries; asks the assistant |
| `apps/globecraft/src/app/ui/assistant_pane.rs` | `crates/ui-egui/src/assistantpane.rs` | No sign-in, skills or agent threads |

The RustGIS name and logo are not used.

```
Copyright the RustGIS contributors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use these files except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
