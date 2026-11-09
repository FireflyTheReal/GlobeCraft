# Layout redesign (user, 2026-11-08)

> "Overall it's great but it feels unorganized, and the different windows from
> settings/airplanes/add data/(legend)... are cluttered and old. Also the great drop
> down feature from Fusion 360 is something to copy as well. I still like the current
> design, just not the layout."

The look stays: colours, icons, type, the globe, the navigator. What changes is where
things live and how they open. Every action stays a command, so the redesign moves
widgets, not behaviour.

## Today

- A ribbon with tabs (Home, Layers, Live, View, Places, Settings, Help); each group
  shows every one of its tools as a button, so groups are wide and the tabs crowded.
- A left sidebar (search results, navigation, export, shortcuts help) and a right pane
  (Measure, Assistant).
- Eleven floating `egui::Window`s, each built its own way: Settings, Add Data, About,
  Shortcuts, Explorer, Legend, the ruler's points, the altitude editor, and the cards
  for a selected flight, a selected satellite and "What's here". They differ in title
  bars, widths, paddings, how they close and where they appear, and several overlap
  the globe's controls.

## Target

1. **Ribbon groups with drop-downs, as in Fusion 360.** Each group shows a few pinned
   tools as large buttons; its title is a drop-down (`MODIFY ▾`) listing every tool of
   the group with its icon, name and shortcut, in sections. Each row has a `⋮` menu:
   *Pin to toolbar*, *Pin to shortcuts* (a user row of favourite tools) and *Change
   keyboard shortcut*. Pins and shortcuts are saved in the workspace settings; the
   defaults reproduce today's most used buttons. Command Search lists the same rows.
2. **One panel system instead of floating windows.** Three kinds of surface, one look:
   - *Docked panes* (left and right, resizable, tabbed): Layers, Places, Data, Measure,
     Assistant, Legend. One pane header style (title, pin, close), one width rule.
   - *Cards* anchored near what they describe (a flight, a satellite, a point): one
     card component with a header, key facts in a two-column grid, actions in a footer;
     at most one card open, placed away from the navigator and the scale bar.
   - *Dialogs* for tasks with a beginning and an end (Settings, Add Data, About,
     shortcuts, the altitude editor): centred, one size scale (small, medium, large),
     the same header, footer buttons right-aligned, Escape closes.
3. **Organised zones.** Top: the ribbon and search. Left: navigation panes. Right: tool
   panes. Bottom: the status bar, timeline and credits. The globe's own controls
   (navigator, zoom, scale bar) keep their corners and nothing else floats over them.
4. **Settings as a dialog with a sidebar** of categories, not a row of tabs that wraps;
   Add Data the same way (Library, Web map, Points and lines, Files).

## Steps

1. Inventory every surface with screenshots (light and dark) and agree the zones.
2. Shared components: pane, card and dialog chrome with tokens for size and spacing.
3. Ribbon drop-downs: group model (pinned and all tools, sections), the drop-down, the
   `⋮` menu with pin and shortcut editing, persistence, Command Search integration.
4. Move the floating windows into panes, cards and dialogs one by one, each with
   before/after screenshots in the interface screenshot test.
5. Keyboard: every drop-down and pane reachable by keyboard (feeds 6.6).

Status (2026-11-09): step 3 is done. Every group title opens its menu (`ui/ribbon_tools.rs`);
`ui.ribbon.pin`, `ui.quickTools.pin`, `ui.shortcut.edit`, `ui.shortcut.set` and
`ui.ribbon.resetTools` are the commands behind the ⋮ menu; pins and shortcuts live in
`workspace.ribbon_toggled`, `workspace.quick_tools` and `workspace.shortcuts`. Zoom In/Out,
Undo/Redo and Close Shape moved to their groups' menus (the navigator and the quick-access
row have them); a few groups gained menu-only tools (Top-Down View, Elevation Profile, Copy View
Link, Save, Clear Flight…). Screenshot test: `ribbon_menus_screenshot`.

Status (2026-11-09, evening): steps 2 and 4 are done. `ui/surface.rs` gives dialogs,
cards and popovers one frame, header (title, ✕) and footer. Dialogs (centred, three
widths, Escape closes): Settings, Add Data, About, Keyboard Shortcuts. Cards (top-left of
the globe, one at a time, `close_other_cards`): selected flight, selected satellite, What's
here. Popover: the altitude editor. Panes: the Legend and the Explorer are right panes;
the ruler's point editor is a section of the Measure pane. Screenshot test:
`surfaces_screenshots`. Left: step 5 (keyboard).

Each step is a commit with screenshots; the user checks the look on the Mac between
steps 3 and 4.
