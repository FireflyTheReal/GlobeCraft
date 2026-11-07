# Network policy (roadmap 10.6)

**Status:** implemented 2026-11-05.

GlobeCraft has no analytics, telemetry or crash-reporting endpoint. It contacts a
service only for the map, data or action the user is using. This document is the
reference for which hosts those are, how often the app asks them and what identity
it sends.

The table is data: `globecraft_data::network::SERVICES` lists the static services,
and `network::tile_services()` derives the basemap rows from
`crates/globecraft-data/src/tiles/sources.json`. The Settings page renders that table
instead of keeping its own copy, so a new tile source appears there automatically.

## What the app may contact

| When | What is contacted |
| --- | --- |
| The globe is visible | The selected basemap's tile host; OpenStreetMap vectors for borders and names come from GlobeCraft's map-data release on GitHub |
| The user searches | Nominatim for results; Photon for suggestions while typing |
| A live layer is on | Its provider: OpenSky/ADSB.lol, OpenSeaFeed/Digitraffic, NOAA SWPC, CelesTrak, RainViewer, Matt Eason's clouds or TomTom |
| The user opens historical imagery | Esri Wayback's tile service and its release list |
| A release build checks for updates | GitHub's release API and asset hosts (one check at startup; disabled for development builds and installs the updater cannot replace) |
| The user adds a layer with an address | That address; local files never leave the computer |
| The user sends an assistant message | Only the selected assistant provider, with the user's key |

Links that open in the operating system's browser (Wikimedia Commons, a provider's
credit page, the Releases page) are not requests made by GlobeCraft.

## Built-in map and imagery sources

These rows are generated from `sources.json`. `max_parallel` is enforced per source by
the tile downloader. "Prefetch" means tiles just outside the view may be requested
ahead; "visible only" means the source's policy forbids it.

| Source | Host | Requests in flight | Prefetch | User agent |
| --- | --- | --- | --- | --- |
| Satellite (Esri World Imagery) | `server.arcgisonline.com` | up to 6 at once | prefetch | `GlobeCraft/0.2 (educational 3D globe viewer; +https://github.com/FireflyTheReal/GlobeCraft)` |
| Historical imagery (Esri Wayback) | `wayback.maptiles.arcgis.com` | up to 6 at once | prefetch | same as above |
| Streets (OpenStreetMap) | `tile.openstreetmap.org` | up to 2 at once | visible only | same as above |
| Terrain/Topography (OpenTopoMap) | `tile.opentopomap.org` | up to 2 at once | visible only | same as above |
| Mapterhorn DEM | `tiles.mapterhorn.com` | up to 6 at once | prefetch | same as above |
| Earth at Night (NASA Black Marble) | `gibs.earthdata.nasa.gov` | up to 4 at once | prefetch | same as above |
| Arctic imagery (NASA Blue Marble, polar) | `gibs.earthdata.nasa.gov` | up to 4 at once | prefetch | same as above |
| Antarctic imagery (NASA MODIS Terra, polar) | `gibs.earthdata.nasa.gov` | up to 4 at once | prefetch | same as above |

## Built-in services

"Cadence" is the app's normal pacing, not a guarantee from the provider. The app can
retry sooner after a failure and live layers pause while hidden. Provider terms remain
the authority; `CREDITS.md` links to them.

| Service | Hosts | Cadence | User agent |
| --- | --- | --- | --- |
| Esri Wayback release list | `s3-us-west-2.amazonaws.com` | once per session | `GlobeCraft/0.2 (wayback release list)` |
| GlobeCraft map data | `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com` | on demand; manifest reused for 10 min | `GlobeCraft/0.1 (map boundary overlay)` |
| OpenStreetMap Nominatim | `nominatim.openstreetmap.org` | on demand | `GlobeCraft/0.2 (educational 3D globe viewer; +https://github.com/FireflyTheReal/GlobeCraft)` |
| Photon (komoot) | `photon.komoot.io` | on demand while typing | `GlobeCraft/0.2 (search suggestions)` |
| OpenSky Network | `opensky-network.org` | normally one request every 17 min | `GlobeCraft/1.0 (live aircraft viewer; OpenSky / ADSB.lol)` |
| ADSB.lol | `api.adsb.lol`, `globe.adsb.lol` | normally one request every 10 s while the layer is visible | same as OpenSky |
| OpenSeaFeed AIS | `api.openseafeed.com` | normally one request every 2 min | `GlobeCraft/1.0 (live vessel viewer; OpenSeaFeed / Digitraffic)` |
| Digitraffic AIS (fallback) | `meri.digitraffic.fi` | normally one request every 2 min | same as OpenSeaFeed |
| NOAA SWPC aurora | `services.swpc.noaa.gov` | normally one request every 10 min | `GlobeCraft/1.0 (aurora forecast; NOAA SWPC)` |
| CelesTrak | `celestrak.org` | normally one request every 6 h | `GlobeCraft/1.0 (satellite orbits; CelesTrak)` |
| RainViewer | `api.rainviewer.com`, `tilecache.rainviewer.com` | normally one request every 10 min for the metadata, then visible radar tiles | `GlobeCraft/1.0 radar viewer` |
| Live Cloud Maps (Matt Eason, EUMETSAT) | `clouds.matteason.co.uk` | normally one request every 3 h | `GlobeCraft/1.0 (attribution: Matt Eason, EUMETSAT)` |
| TomTom Traffic | `api.tomtom.com` | normally one request every 2 min; camera movement may request a new tile grid sooner | `GlobeCraft/1.0 (traffic; TomTom)` |
| GlobeCraft updates | `api.github.com`, `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com` | once per session | `self-update/<crate version>` from the updater library |

Nominatim's usage policy asks for an identifying User-Agent and at most one request per
second; searches are user-triggered, not a background poll. OpenStreetMap's tile policy
asks for at most two downloads at once and visible tiles only. The table records those
policies where the app can apply them.

Checked 2026-11-06 with the agents above: NOAA SWPC, CelesTrak, OpenSeaFeed and
Digitraffic accepted them. Digitraffic requires gzip encoding, which the app's client
sends. On 2026-11-07 a user-supplied TomTom key returned an authenticated 256×256 PNG
tile with the app's exact URL, header and `GlobeCraft/1.0 (traffic; TomTom)` agent; the
key was not stored.

## User-added layers

A layer added through Add Data may name any HTTP(S) host. Its host is listed in
Settings → Network under "Your data", with the reload interval when it is a live URL.
A file on this computer has no host and is never uploaded. The request carries
`GlobeCraft/0.3 (3D globe; user-added layer; +https://github.com/FireflyTheReal/GlobeCraft)`.

WMS capabilities can name further endpoints, and a server may redirect. GlobeCraft
lists the address the user entered; it cannot know every host a third-party service
might redirect to. The same caveat applies to any provider above that uses a CDN.

## Assistant

The assistant contacts only the selected provider. Its prompts, the commands it
proposes and the API key go to that provider. A provider on `localhost` or the local
network stays on the user's network. Preset default hosts:

| Preset | Default host |
| --- | --- |
| Anthropic | `api.anthropic.com` |
| OpenAI | `api.openai.com` |
| Google Gemini | `generativelanguage.googleapis.com` |
| OpenRouter | `openrouter.ai` |
| Ollama (local) | `localhost:11434` |
| OpenAI-compatible | `localhost:1234` |

`OLLAMA_BASE_URL`, `OPENAI_COMPATIBLE_BASE_URL` or a saved base address can change the
host and port. The Settings page shows the effective host for the provider the
assistant would use, or says that no provider is configured.

## Identity and redirects

Every client built by `globecraft_data::http` sets the user agent from
`globecraft_data::network`; the assistant's `model_http` client uses the shared
assistant agent. The updater library sends its own identity. Requests follow HTTP
redirects, so a listed host can send the app to another host; the table names the
endpoint GlobeCraft chooses, not every possible redirect target.

No other headers identify the user. There is no account, device identifier or
telemetry. API keys are sent only to the provider they belong to; see
`docs/features/ASSISTANT.md` for key storage.

## Changing the policy

- A new static service needs a `Service` row in `network.rs`, its user agent constant,
  and an HTTP policy in `http.rs` when it needs different timeouts.
- A new tile source is a `sources.json` entry. It appears in Settings and in this
  document's map table automatically; keep the two in sync if the table is edited by
  hand.
- A new user-controlled endpoint must appear in the Settings page's dynamic section.
- Tests in `network.rs` check unique service ids, non-empty valid hosts, a host for
  every tile source, the parallel limits and the URL constants used by other modules.

The network table is descriptive. It does not intercept requests. Rate-limit
enforcement remains in each service's refresh loop, and a provider can tighten its
terms at any time. Review the provider terms linked from `CREDITS.md` before changing a
host, cadence or user agent.
