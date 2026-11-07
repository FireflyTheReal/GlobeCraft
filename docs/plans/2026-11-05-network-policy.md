# Network policy and hosts-contacted page (roadmap 10.6)

**Status:** implemented 2026-11-05; macOS GUI and live-agent checks completed
2026-11-06; TomTom authenticated tile checked 2026-11-07.

## Why this item

GlobeCraft talks to more than twenty services: basemap tiles, map data, search,
live layers, updates and whichever assistant provider the user chooses. Before this
change that knowledge was spread across request builders, constants and settings, so
neither a user nor a reviewer could answer "which host will this app contact?" without
reading the whole data crate. The roadmap asked for one table of hosts, cadence and
user agents, plus a Settings page that lists every host the app may contact.

This is a trust feature, not a rendering change. It leaves tile selection, refresh
timing, projection and persisted settings untouched. The only intentional behavior
change is that four live services now send their own identity instead of borrowing the
aircraft or radar agent.

## What shipped

### One table in data

`crates/globecraft-data/src/network.rs` now owns:

- `SERVICES`: the static services (Wayback release list, map data, search, aircraft,
  vessels, aurora, satellites, radar, clouds, traffic and updates), each with a stable
  id, optional translation key, Settings group, hosts, user agent and request cadence;
- `tile_services()`: every built-in tile source read from `tiles/sources.json`, with
  its host extracted from the XYZ template (or the Wayback tile host), its
  `max_parallel` and its prefetch policy;
- `host_of_url()`: display-only extraction of the authority from an HTTP(S) address,
  including ports, IPv6 brackets and credentials removal;
- the shared user-agent constants used by `http.rs`, `model_http.rs` and the app.

Static hosts live in the table. Tile hosts are derived, so adding a source to
`sources.json` cannot leave it off the page. User-added layers and the assistant are
dynamic and are listed from the user's settings instead of being invented as built-ins.

### HTTP identity

`http::Policy` gained `Traffic`, `Vessels`, `Aurora` and `Satellites`. They reuse the
previous timeouts (12 s for traffic, 25 s for the other three) and send dedicated user
agents:

| Policy | Agent |
| --- | --- |
| Traffic | `GlobeCraft/1.0 (traffic; TomTom)` |
| Vessels | `GlobeCraft/1.0 (live vessel viewer; OpenSeaFeed / Digitraffic)` |
| Aurora | `GlobeCraft/1.0 (aurora forecast; NOAA SWPC)` |
| Satellites | `GlobeCraft/1.0 (satellite orbits; CelesTrak)` |

`model_http` now uses `network::ASSISTANT_USER_AGENT`; the string is identical to the
previous inline value. Existing policies and agents (tile, search, boundary, Wayback,
clouds, radar, aircraft, user data) are unchanged.

### Settings → Network

A new searchable Settings category renders groups in this order: maps and imagery, map
data, search, live layers, updates, your data and assistant. Each service shows its
hosts, cadence and user agent. Tile rows also say whether the source is visible-only or
may prefetch.

The page adds two dynamic sections:

- **Your data**: each saved layer's host, with its reload interval for live feature
  URLs; local files are omitted because they contact nothing;
- **Assistant**: the provider the assistant would use and its effective address,
  including environment base-URL overrides and a saved custom address. If no provider is
  configured, the page says so.

The Settings search now matches service ids, product names, English service names and
hosts, including the dynamic layer/assistant hosts. Searching for `github`, `OpenSky`
or `tile.openstreetmap.org` finds the Network page.

### Documentation

- `docs/NETWORK_POLICY.md` is the reference: current tables, provider-policy caveats,
  redirects, assistant presets, identity and how to change the policy.
- `README.md` links the policy and mentions the Settings page.
- `docs/ARCHITECTURE.md` records the data-owned table and its descriptive role.
- `CONTRIBUTING.md` adds the page to the categorized-preferences smoke checklist.
- `docs/ROADMAP.md` marks 10.6 done and removes it from the working queue.

## Deliberate behavior changes

1. Traffic, vessels, aurora and satellites no longer send the radar or aircraft user
   agent. Requests, URLs, timeouts and refresh intervals are unchanged.
2. Settings has one more category. Defaults and the persisted settings format are
   unchanged.
3. `configured_assistant_provider` was extracted from `initial_provider` so the page and
   the assistant share the same choice (saved provider, else the first provider with an
   environment key). `initial_provider` still falls back to Anthropic exactly as before.

## Verification

Ran from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
python3 -m unittest discover -s scripts -p 'test_profile_summary.py' -v
```

Results: formatting, clippy and the workspace build passed. The workspace test run
passed 568 tests with 61 ignored manual/network tests; the Python summary tests passed 2
of 2. Network-specific tests cover unique service ids, valid non-empty hosts, user-agent
prefixes, a service for every static Settings group, a host and parallel limit for every
tile source, host extraction, and the Wayback/map-data/update URL constants. A user-layer
test checks that a tile or feature URL yields a host while a local path does not. An app
test checks cadence formatting in English; the existing localization tests check that
both catalogs have matching keys and variables.

The Settings page was checked in the real macOS application (debug build, isolated
`XDG_CACHE_HOME`) on 2026-11-06: the window was dragged to its 520×360 point minimum,
the Network page was viewed in English and Portuguese, and the body was scrolled from
the basemap rows to the Assistant group. Nothing was clipped or overlapped; hosts and
the longer Portuguese help text wrapped inside the window. An ignored kittest test,
`network_settings_screenshots`, renders the same page at that size in both languages
with a remote user layer and an Ollama provider, and asserts the basemap, user-layer
and assistant hosts appear. It writes its screenshots to
`target/screenshots-network/`; it is not part of the default test run.

Live requests with the exact agents from `network.rs` on 2026-11-06:

| Service | Result |
| --- | --- |
| NOAA SWPC aurora | 200, 920 KB JSON |
| CelesTrak (stations) | 200, 8 KB JSON |
| OpenSeaFeed snapshot | 200, 4.4 MB JSON |
| Digitraffic AIS | 200 with `Accept-Encoding: gzip` (reqwest sends it; a client without it gets 406) |
| TomTom Orbis traffic | 200, 256×256 RGBA PNG tile with a user-supplied test key |

The TomTom key was supplied for this check and was not stored in the repository or in
a settings file. With it, the app's exact URL, `TomTom-Api-Key` header and
`GlobeCraft/1.0 (traffic; TomTom)` agent returned a 65 KB tile for Berlin at zoom 12.

## Review notes and critique

These are observations from this item, not all fixed here.

1. **The table describes policy; it does not enforce it.** `http::client(Policy)` sets
   identity and timeouts, but no request passes through a central limiter that reads
   `RequestLimit`. Tile `max_parallel` is enforced in the tile cache, and live refresh
   timers are in their workers; a future call site can ignore the table entirely. The
   feature therefore improves transparency, not privacy enforcement. A real gate would
   need a service handle that owns URL, identity, timeout and admission together, and
   that is a larger change than 10.6. Roadmap 1.3 (per-host limits) is the right place
   for it; adding a global limiter here without workload evidence could add latency and
   block background work.

2. **`Policy` and `SERVICES` are parallel lists and can drift.** The tests guard the
   Wayback, map-data and updater URL constants, but search and live-service URLs remain
   inline in their modules. A URL change there would leave the page listing a stale
   host, which is exactly the privacy claim this feature is supposed to make. The next
   step should expose each service's URL constant and test it against the table, or let
   the service descriptor own the URL builder. Do not duplicate URL construction in the
   UI table.

3. **Every agent has now been checked against a successful response.** Live requests
   on 2026-11-06 showed that NOAA SWPC, CelesTrak, OpenSeaFeed and Digitraffic accept the
   new agents (Digitraffic requires `Accept-Encoding: gzip`, which reqwest sends). On
   2026-11-07 a user-supplied TomTom key returned an authenticated 256×256 PNG tile with
   the app's exact URL, header and agent. The key was not stored. Recheck the rows if a
   provider changes its policy; if one objects, keep its old agent and record the
   constraint here and in `NETWORK_POLICY.md`.

4. **The page cannot be exhaustive for third-party redirects.** It lists the host the
   app chooses. HTTP redirects, WMS capabilities, DNS/CDN routing and a system proxy can
   contact another host that is not in the table. The page and policy document say this
   explicitly. A runtime request log would be more complete but would risk recording
   sensitive URLs and keys; that trade-off deserves a separate design, not a hidden
   logging path.

5. **The page is dense, though it now has layout evidence.** Twenty-two built-in rows
   with a host line, cadence and user agent produce a long scroll, and some Portuguese
   strings are longer than the English ones. The 520×360 point minimum and the dynamic
   groups were checked in the real macOS app and by `network_settings_screenshots` in
   both languages; wrapping, scrolling and the footer held up. The screenshot test is
   manual/ignored rather than a CI gate (roadmap 10.1), so a future layout change can
   regress it unnoticed. Collapsing user agents behind a disclosure would shorten the
   page if it proves noisy.

6. **Environment-derived assistant state appears before the assistant is used.** The
   page shows the provider selected by an environment key even if no message has been
   sent, and the helper text says the assistant uses that provider. That is the correct
   "would use" state, but it is not the same as "has contacted". The wording avoids
   claiming a request happened; a future status line could distinguish configured from
   active.

7. **`Config::from_env` is called while building the page.** It reads environment
   variables and normalizes local base URLs, but performs no I/O. It runs only while
   Settings is open and the user is interacting, so this is not a frame-path problem.
   If the assistant configuration becomes hot-reloadable, the page should subscribe to
   the same state instead of re-deriving it per frame.

8. **Service grouping is partly a UI concern.** `network::Category` is used both to
   group services and to order the Settings page. The Assistant and Your Data groups
   have no static `Service` and are special-cased in the app. This is acceptable while
   there are two dynamic groups; if more appear, split "service category" from "page
   section" so data does not grow UI-only variants.

9. **The documentation table is manually maintained for static services.** Tile rows
   are generated in code but not in the Markdown. A future service row or host change
   must update both the table and the document; a test cannot see the Markdown. A small
   generator would keep them aligned, but it should read the production table rather
   than a second description.

10. **The table names Esri Wayback's release list as S3 and its tiles as
    `wayback.maptiles.arcgis.com`.** Those are two different hosts for one product.
    The page shows both in separate groups, which is accurate but may surprise users
    looking for a single "Esri" line. Grouping by product rather than by request type
    would be a presentation improvement, not a policy change.

## Files changed

- `crates/globecraft-data/src/network.rs` (new): table, tile derivation, host parser,
  tests.
- `crates/globecraft-data/src/lib.rs`: export the module.
- `crates/globecraft-data/src/http.rs`: shared agents; Traffic/Vessels/Aurora/Satellites
  policies.
- `crates/globecraft-data/src/model_http.rs`: shared assistant agent.
- `crates/globecraft-data/src/traffic.rs`, `vessels.rs`, `aurora.rs`, `satellites.rs`:
  dedicated policies.
- `crates/globecraft-data/src/wayback.rs`, `map_data.rs`: crate-visible URL constants
  for the table tests.
- `crates/globecraft-data/src/user_data/mod.rs`: `UserLayer::network_host` and its test.
- `apps/globecraft/src/app/ui/settings.rs`: Network category, dispatch, search.
- `apps/globecraft/src/app/ui/settings/network.rs` (new): table rendering, dynamic
  hosts, cadence formatting and tests.
- `apps/globecraft/src/app/assistant.rs`: shared configured-provider selection.
- `apps/globecraft/src/app/screenshots.rs`: ignored minimum-size layout screenshots in
  both languages, with dynamic hosts.
- `crates/globecraft-core/locales/en.ftl`, `pt-BR.ftl`: category, groups, cadence,
  dynamic rows and help.
- `README.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`,
  `docs/NETWORK_POLICY.md` (new), this document.
