# Interface

GlobeCraft's window follows the layout of [RustGIS](https://github.com/opengeos/RustGIS)
(Apache-2.0; see [CREDITS.md](../../CREDITS.md)):

| Area | What it holds |
| --- | --- |
| Header | Logo, Quick Access Toolbar (Open KML, Save, Undo, Redo, Screenshot), the open file and world, the place search and **Command Search** (Ctrl/⌘+K) |
| Ribbon | **File** menu, then the tabs Home, Layers, Live, View, Places, Settings and Help. Double-click a tab (or Ctrl+F1) to collapse the ribbon; a collapsed ribbon opens over the globe when a tab is clicked |
| Left pane | **Layers** (basemap and every layer as a table of contents with status and credits), **Places** (the KML tree; it can be pinned below the other tabs) and **Globe** (search results, navigation, export, controls) |
| Right pane | **AI Assistant** and **Measure** as closable tabs |
| Globe | The renderer, the navigator (rotation ring, move pad, zoom slider), scale bar and credits |
| Status bar | Cursor and centre coordinates (click to copy), camera altitude (click to type one), heading (click for north up), status messages, updates and tile loading |

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
