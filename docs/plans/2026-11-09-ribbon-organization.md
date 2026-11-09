# Ribbon, panes and settings: one organization (2026-11-09)

Part of the layout redesign (`2026-11-08-layout-redesign.md`). The user: "it's just not as
simple as I think it could be … a solid organization of tabs, groups and singular settings,
the standard view and what belongs in the drop-down."

## What is wrong today (inventory screenshots, Advanced mode)

- **Seven tabs, three of them thin.** Places repeats Home's Places group; Settings repeats
  the Settings window (theme, units, language, experience); Help is four buttons that are a
  menu everywhere else.
- **Things live in two or three places.** Navigation buttons are on Home, in the Explore
  pane and on the navigator; Screenshot is on Home, in the Explore pane, in the File menu and
  the quick-access row; panes are toggled from a "Pane" group on four tabs; theme is on View
  and on Settings; the TomTom key is on the Live ribbon and in Settings › Keys.
- **Layers are split by where they were added, not by what they are.** Roads, railways,
  ferries and airports (static OpenStreetMap data) sit on *Live*; sky layers on *View*;
  borders and names on *Layers*.
- **Choices take one button per option.** Basemap (4), theme (3), units (3), Earth model (2),
  world (3), experience (2). They fill the ribbon and say less than one button showing the
  current choice.
- **Tabs overflow.** Layers scrolls at 1480 px; groups have one, three or nine tools.
- **Settings has eleven categories in two wrapped rows of tabs**; the left pane mixes a
  search-result list, navigation buttons, export and a shortcuts cheat sheet.

## Rules

1. **One home per tool.** A tool has one place on the ribbon. Panes show lists and state
   (places, layers, data), never buttons repeating the ribbon. Preferences live in Settings.
2. **Tabs answer the user's questions**: *Explore* (go somewhere, mark it, measure it,
   capture it), *Map* (what is drawn), *Live* (what is happening now, and when), *View* (how
   the window and the sky look). File and Help are menus, not tabs.
3. **Groups are nouns and stay small**: at most one large button and two columns of three
   small ones visible. Everything else of the group is in its ▾ menu. A group with one tool
   has no menu.
4. **A set of choices is one split button** showing the current choice: click applies it,
   ▾ lists the others (Basemap, Theme, World, Units, Earth model, Screenshot quality).
5. **Every tab fits a 1280-point window** without scrolling, in English and Portuguese.
6. **Advanced adds, never moves.** Advanced tools appear in group menus (and a few extra
   buttons at a group's end); Basic and Advanced share the same tabs and groups, so switching
   level never reshuffles the ribbon.

## Header

`[File]  Explore  Map  Live  View   …   [quick access]  [place search]  [Command Search]  [?]  [⚙]`

- **File** menu: New, Open…, Open Recent ▸, Save, Save As…, Add Data…, Export ▸ (Screenshot,
  Copy View Link), Settings…, Quit.
- **?** menu: Keyboard Shortcuts, Documentation, Report an Issue, About GlobeCraft.
- **⚙** opens Settings (⌘,).

## Tabs and groups

Visible on the ribbon / in the group's ▾ menu only. *(A)* = Advanced only.

### Explore (was Home)

| Group | On the ribbon | In the menu |
|---|---|---|
| Go | **Find Place** · Global View, North Up, Top-Down View | Zoom In, Zoom Out, Auto-Rotate, Open View Link…, Explorer *(A)* |
| Places | **Placemark** · New Folder, Open… | Save, Save As…, Clear All, Places pane |
| Measure | **Ruler** · Edit Points, Clear | Close Shape, Elevation Profile, Ruler Unit ▸, Undo, Redo, Measure pane |
| Capture | **Screenshot ▾** (split: quality) | Copy View Link, Screenshots Folder… |
| Assistant | **AI Assistant** | — |

### Map (was Layers)

| Group | On the ribbon | In the menu |
|---|---|---|
| Imagery | **Satellite ▾** (split: Streets, Topographic, Historical) · date stepper when Historical | Compare dates |
| Borders & Names | Countries, States, Municipalities · Place Names, Sea Names, Places of Interest | Line style…, States visible below… |
| Nature | **3D Terrain** · Rivers & Waters, Parks, Peaks | Exaggeration |
| Transport | Roads, Railways, Airports, Ferries | — (moved from Live: static OpenStreetMap data) |
| Reference | Grid, Grid Labels, Time Zones | Earth Model ▸ *(A)* |
| My Data | **Add Data** · Data pane | Library, Favourites, Recent |

### Live

| Group | On the ribbon | In the menu |
|---|---|---|
| Weather | Rain Radar, Clouds, Aurora | — |
| Moving | Aircraft, Ships, Satellites · Road Traffic | Clear selection; the TomTom key moves to Settings › Keys |
| Hazards | Active Fires, Disaster Alerts | (presets today; become layers) |
| Time | **Timeline** · Now, Play / Pause | Speed, Window |

### View

| Group | On the ribbon | In the menu |
|---|---|---|
| World | **Earth ▾** (split: Moon, Custom Planet) *(A)* | Explorer *(A)* |
| Sky & Light | Atmosphere, Sunlight, Night Lights · Stars, Milky Way | — |
| Panels | Sidebar, Legend, Measure, Assistant | Layers, Places, Data, Tile Diagnostics |
| Window | **Navigator** · Full Screen, Theme ▾ | Collapse Ribbon, Minimize, Interface Size… |

Gone: the **Places** tab (into Explore › Places and File), the **Settings** tab (into the
Settings dialog; Theme into View › Window; units and coordinate format into the status bar,
where the values are shown), the **Help** tab (into ?), the four "Pane" groups (into
View › Panels).

## Panes

- **Left sidebar**, tabbed: *Places* · *Layers* · *Data*. The Explore pane goes: search
  results appear under the search box and as a temporary "Search results" folder at the top of
  Places (as in Google Earth); its navigation and export buttons repeated the ribbon; the
  shortcut cheat sheet is ? › Keyboard Shortcuts.
- **Right**, tabbed: *Measure* · *Assistant* · *Legend* (the Legend window becomes a pane).
- **Cards** for one selected thing (flight, satellite, "What's here"), one card at a time,
  anchored away from the navigator (layout redesign step 4).

## Status bar

`Cursor  Elevation  Centre  Altitude  Heading … status … tiles`. Clicking a coordinate
opens the coordinate format; clicking a distance opens the units. That is where the values
are read, so that is where they are changed.

## Settings dialog

A sidebar with seven pages instead of eleven wrapped tabs, and the search box above it:

1. **General**: language, experience level, units, coordinate format, startup view.
2. **Appearance**: theme, interface size, fonts, map text size.
3. **Map style**: line colours, widths and dashes; name sizes.
4. **Navigation**: mouse and trackpad, navigator style, game controllers.
5. **Globe & graphics**: Earth model, sky, GPU globe, frame rate.
6. **Data & network**: cache sizes, download limits, service keys.
7. **Files & shortcuts**: folders; every tool with its shortcut, editable; Reset ribbon.

## Group menus (fixes to step 3)

- No explanation line at the top; the ⋮ appears only on the hovered row and is part of it.
- No blue bars: the menu has two sections, the tools on the ribbon (in ribbon order), a
  separator, then the tools kept in the menu. Pinning moves a tool between them.
- Group titles read as buttons: the whole title row, as wide as the group, is the target;
  single-tool groups have no menu and no caret.
- Keyboard: ↑/↓ move, Enter runs, → opens ⋮, Esc closes.
- Choice sets show as radio rows.

## Status

- 2026-11-09: steps 1–3 done. Four tabs in `ui/ribbon_tabs.rs`; choice buttons
  (`widgets::choice_big`, `choice_small`); ? and ⚙ in the header; File menu with Add Data.
  All four tabs fit 1280 points in English and Portuguese (`layout_inventory_screenshots`
  with `GLOBECRAFT_SHOT_SIZE=1280x800`, `GLOBECRAFT_SHOT_LANG=pt-BR`). Differences from the
  tables: Hazards waits until fires and alerts are layers; the exaggeration is a small menu
  (`1.0× ▾`) in Nature; the ruler unit stays visible in Measure.
- 2026-11-09: step 4 done. The Explore pane and `ui.pane.explore` are gone (a saved
  "explore" panel opens Places); search results are listed at the top of Places with a
  `search.clear` ✕, and a finished search shows Places. The Places pane lost its Save /
  Save As / Open row and its file footer (drop zone, path field, Clear All): those are in
  File and Explore › Places, and a KML dropped anywhere opens.

## Steps (each a commit with before/after screenshots of every tab)

1. Group menus fixed (above).
2. Split buttons for choice sets; the Places, Settings and Help tabs folded in; File and ?
   menus; tabs renamed (saved tab codes map: home→explore, layers→map, places→explore,
   settings→view, help→explore).
3. Groups as in the tables, with tab-width checks at 1280 points in both languages.
4. Left sidebar without the Explore pane; search results in Places.
5. Settings dialog with the sidebar and seven pages.
6. Status-bar menus for coordinate format and units.

Commands keep their ids, so the assistant, Command Search and saved shortcuts keep working.
Pins saved against the old group ids are dropped once (they are a day old).
