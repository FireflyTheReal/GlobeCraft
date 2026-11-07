# Layers pane search and filter (roadmap 6.9)

**Status:** implemented and verified 2026-11-07; no commit made.

## Why this item

The Layers pane is the table of contents for every basemap, built-in layer and
user-added layer. It grew past a screen as map data arrived, and the user has to
scroll and read every heading to find one row. Roadmap 6.9 asks for a search by
name, category and active state; the catalogue itself remains Add Data.

## What shipped

### Sticky search and active-only filter

`apps/globecraft/src/app/ui/panes.rs` draws `App::layers_search` above the
`layers-scroll` scroll area, so the field stays reachable while a long list
scrolls. It holds one `LayersUi` state on `App`:

- `query` is the search text. It is not saved: the filter only narrows the pane
  for the session, and no settings key was added.
- `active_only` is the **Active only** check box. It uses `App::layer` for
  built-in rows (so refinements follow their parent), `layer.visible` for user
  layers and the selected provider for the basemap radio.

`matches_filter` in `layers_pane.rs` folds the query and every candidate name
with `globecraft_core::language::fold_name`: lower case, accents removed, only
alphanumeric characters kept. Searching `paises` therefore finds
`Países` in Portuguese. Rows match on their own label or their group's label, so
searching `borders` or `limites` keeps a whole category.

### Filtered list

`App::layers_pane` builds the basemap, user-layer and built-in-group lists from
the filter before drawing them:

- Only matching rows are drawn; a group with no matches disappears.
- A matching group heading is forced open while a filter is active without
  changing the stored open/closed state. Clearing the filter restores the state
  the user had before.
- The Geography loader log and boundary attribution, and the live-overlays help
  footer, are hidden while filtering so the result list stays readable.
- Matching user layers keep their opacity, order, zoom and legend controls.
- If nothing matches, the pane shows `layers-search-empty` with the raw query.
  `layers-search-empty-active` is a localized fallback for an empty active-only
  result; in practice a basemap is always selected, so the basemap group remains.
- The ✕ button appears when the query is not empty, and Esc clears the field
  while it has focus. Both clear the query without touching `active_only`.

The six Fluent IDs (`layers-search-placeholder`, `layers-search-clear`,
`layers-search-active-only`, `layers-search-active-help`,
`layers-search-empty`, `layers-search-empty-active`) exist in both
`en.ftl` and `pt-BR.ftl` with the same variables.

## Verification

- `search_matches_names_groups_and_active_state` covers label and group matches
  in English and Portuguese, accent folding, the empty query and the active-only
  interaction.
- `interface_screenshots` renders `02-layers-search-light` (query `parks` keeps
  only the Parks row under its open group), `02-layers-active-light` (only active
  rows and their groups) and `02-layers-search-empty-light` (the localized
  no-results note). All three were inspected by eye.
- Full gate: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked` (573 passed, 61 ignored) and
  `cargo build --workspace --locked`.

## Limits and critique

- Search matches the current UI language only. An English query does not find a
  Portuguese label; this is the same rule as Settings search and keeps the
  result tied to what the user can read on screen.
- While filtering, group headings cannot be collapsed because the matches must
  stay visible. Clearing the search restores the previous state.
- The filter is plain substring matching after folding, not fuzzy matching or
  keyword aliases; layer codes and English names are not searched in Portuguese.
- The pane does not scroll to the first match; the filtered list is short enough
  that the first row is already at the top.
- The empty active-only note is defensive: a basemap is always selected, so the
  basemap group normally prevents an entirely empty active-only result.
