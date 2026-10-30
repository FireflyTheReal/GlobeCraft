# Interface

GlobeCraft's window follows the layout of [RustGIS](https://github.com/opengeos/RustGIS)
(Apache-2.0; see [CREDITS.md](../../CREDITS.md)):

| Area | What it holds |
| --- | --- |
| Header | File, Quick Access Toolbar (Undo, Redo, Screenshot), the place search and **Command Search** (Ctrl/⌘+K); Open KML and Save stay in the File menu. On Windows and Linux the header also holds minimize, maximize/restore and close. macOS keeps its native traffic lights at the left. The window title stays **GlobeCraft** and is not drawn in the merged header |
| Ribbon | **File** menu, then the tabs Home, Layers, Live, View, Places, Settings and Help. Double-click a tab (or Ctrl+F1) to collapse the ribbon; a collapsed ribbon opens over the globe when a tab is clicked |
| Left pane | **Layers** (basemap and every layer as a table of contents with status and credits), **Places** (the KML tree; it can be pinned below the other tabs) and **Globe** (search results, navigation, export, controls) |
| Right pane | **AI Assistant** and **Measure** as closable tabs |
| Globe | The renderer, the navigator (rotation ring, move pad, zoom slider), scale bar and credits |
| Status bar | Cursor and centre coordinates (click to copy), camera altitude (click to type one), heading (click for north up), status messages, updates and tile loading |

The header is also the title bar. On macOS the native traffic lights stay at the left and
the header starts after them; dragging empty header space moves the window, and a
double-click zooms or restores it. On Windows and Linux the window has no native
decorations: minimize, maximize/restore and close are drawn at the right of the header,
the border resizes the window, and the same drag/double-click behaviour applies. The
window title stays **GlobeCraft** for Mission Control, the taskbar and window lists even
though the merged header does not draw it.

The theme follows the system, or Settings › Theme. Pane widths, the open panes, the ribbon
tab and whether it is collapsed are saved in `settings.json` (`sidebar` and `workspace`).

## Commands

Every action is a command with an id and JSON parameters, listed in
`crates/globecraft-core/src/commands.rs`. Ribbon buttons, the File menu, macOS menus, keyboard
shortcuts, the context menu, game controllers, Command Search and the AI assistant all run
commands through `App::run`, so each action has a single implementation and the same
enabled/checked state everywhere.

Command Search lists every command that needs no values, every layer, basemap and world.
When nothing matches, Enter hands the words to the AI assistant.

## Layers

Map layers have stable codes (`globecraft_core::layers::MapLayer`) shared by the ribbon,
the Layers pane, persistence and the assistant. Their visibility, the basemap and terrain
exaggeration are now remembered between launches (`settings.json` › `layers`); the stars,
cinematic sky and grid labels keep their earlier fields (`sky`, `grid_numbers`). Live feeds
(radar, clouds, aircraft, vessels, satellites, roads, traffic) start hidden on every launch,
as before, so opening GlobeCraft never contacts those services unasked.

## Terrain occlusion and hover details

When relief is drawn in a tilted view, ridges and islands hide overlays the same way they
hide the imagery: borders break into visible runs, and roads, railways, ferries, airports,
rivers, parks, place and peak markers, AIS ships, placemarks and satellite subpoints are
not drawn where the terrain is in front of them. Ships stay on the sea surface but can be
hidden by land. The exact top-down view skips this test.

Hovering a live object or place shows the fields its source publishes, in the selected
language:

| Object | Hover details |
| --- | --- |
| Aircraft | Callsign or hex, altitude, speed, heading, vertical rate (climb, descent or level) and report age |
| Ship | Name, MMSI, AIS vessel type, destination, length × beam, draught, position, course, speed and report age |
| Satellite | Name, NORAD id, orbital period, mean altitude and inclination |
| Airport | IATA and ICAO codes and the airport class (international, public, regional, military, private) |
| Place, water or park | Name and OpenMapTiles class |
| Peak | Name and elevation |

A field the source did not send shows as `—`.
