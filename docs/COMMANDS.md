# Commands

Generated from `crates/globecraft-core/src/commands.rs` and the English labels by
`crates/globecraft-core/tests/command_reference.rs`; do not edit by hand. Every
command can be run from Command Search, and the assistant can run those marked so.
Parameters are a JSON object; none is required unless marked.

## Explore

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `camera.flyTo` | Fly To | Fly the camera to a latitude and longitude, optionally at an altitude and heading. | `altitude_km` (number): Camera altitude above the surface in km (0.15..40000); default keeps the current altitude<br>`heading_deg` (number): View heading clockwise from north<br>`lat` (number), required: Latitude in degrees, -90..90<br>`lon` (number), required: Longitude in degrees, -180..180<br>`tilt_deg` (number): View tilt from straight down, 0..80; default keeps the current tilt | yes |
| `camera.set` | Go To XY | Move the camera instantly; parameters left out keep their current value. | `altitude_km` (number)<br>`heading_deg` (number)<br>`lat` (number)<br>`lon` (number)<br>`tilt_deg` (number): 0 (straight down) .. 80 | yes |
| `search.place` | Find Place | Search for a place, an address or coordinates (OpenStreetMap Nominatim) and fly to the best result. | `fly` (boolean): Fly to the best result when it arrives (default true)<br>`query` (string), required: A place name, an address, or coordinates such as 48.85, 2.35 | yes |
| `search.clear` | Clear Search Results | Remove the last search's results from Places and empty the search box. | — | yes |
| `camera.zoomIn` | Zoom In | Move the camera closer to the surface. | — | yes |
| `camera.zoomOut` | Zoom Out | Move the camera away from the surface. | — | yes |
| `camera.resetNorth` | North Up | Turn the view so north points up (N). | — | yes |
| `camera.tilt` | Tilt | Lean the view toward the horizon, or back (Shift+scroll, Shift+drag or right-drag up and down, Shift+Up/Down). Close to the ground the camera backs off as far as the tilt needs. | `step` (number): Degrees to add to the current tilt<br>`tilt_deg` (number): Tilt from straight down, 0..80 (less close to the ground) | yes |
| `camera.topDown` | Top-Down View | Look straight down again (U). | — | yes |
| `camera.globalView` | Global View | Fly out until the whole globe is in view. | — | yes |
| `camera.autoRotate` | Auto-Rotate | Slowly spin the globe. | `on` (boolean): Omit to toggle | yes |
| `places.addAtCenter` | Placemark | Add a placemark at the centre of the view and edit its name. | `description` (string)<br>`name` (string) | yes |
| `places.add` | Add Placemark At | Add a placemark at a latitude and longitude. | `description` (string)<br>`folder` (string): Name of an existing folder to put it in<br>`lat` (number), required<br>`lon` (number), required<br>`name` (string) | yes |
| `places.addFolder` | New Folder | Add a folder to My Places. | `name` (string) | yes |
| `places.flyTo` | Fly To Place | Fly to a placemark by its name. | `name` (string), required: Placemark name (case-insensitive) | yes |
| `places.remove` | Remove Place | Remove a placemark or folder by its name. | `name` (string), required: Placemark or folder name | asks first: it discards work or closes GlobeCraft |
| `places.clear` | Clear All | Remove every place and forget the open KML files (the files are kept). | — | asks first: it discards work or closes GlobeCraft |
| `ruler.enable` | Ruler | Measure geodesic distances: click the globe to add points, Alt-click to start a new path. | `on` (boolean): Omit to toggle | yes |
| `ruler.addPoint` | Add Ruler Point | Add a measuring point at a latitude and longitude. | `lat` (number), required<br>`lon` (number), required<br>`new_path` (boolean): Start a separate path | yes |
| `ruler.clear` | Clear Ruler | Remove every measuring point. | — | yes |
| `ruler.closeShape` | Close Shape | Join each ruler path of three points or more back to its first point and measure the area it encloses. | `on` (boolean): Omit to toggle | yes |
| `ruler.profile` | Elevation Profile | The ground's height along the ruler: lowest and highest point, climbing, descending and the heights along the way. | — | yes |
| `ruler.unit` | Ruler Unit | Choose the unit measurements are shown in. | `unit` (string), required: one of `automatic`, `centimeters`, `meters`, `kilometers`, `inches`, `feet`, `yards`, `miles`, `nautical_miles`, `smoots`, `degrees`, `arcseconds`, `football_fields` | yes |
| `ruler.editPoints` | Edit Points | Type exact coordinates for ruler points. | — | no |
| `edit.undo` | Undo | Undo the last ruler edit. | — | yes |
| `edit.redo` | Redo | Redo the last undone ruler edit. | — | yes |
| `view.copyLink` | Copy View Link | Copy a globecraft:// link to this view (place, height, angle, basemap, layers and web data). Paste it into the search box, or send it to someone, to open the same view. | — | yes |
| `view.openLink` | Open View Link | Open a globecraft:// view link: its place, height and angle, basemap, layers and web data. | `link` (string), required: A globecraft://view?lat=…&lon=… link (optional alt, heading, tilt, basemap, layers, data) | asks first when it names a web address |
| `file.screenshot` | Screenshot | Save the globe without the interface as a PNG in the screenshot folder. | `quality` (string): one of `normal`, `plus`, `pro`, `pro_max`: normal: screen size once every tile has loaded; plus: twice the size, one zoom level deeper; pro: four times the size, two levels deeper; pro_max: eight times the size, three levels deeper. Default: the chosen quality | asks first: it reads or writes files on this computer |
| `file.screenshotQuality` | Screenshot Quality | Choose the screenshot quality: Normal (screen size, once every tile has loaded), Plus (twice the size, one zoom level deeper), Pro (four times the size, two levels deeper) or Pro Max (eight times the size, three levels deeper). | `quality` (string), required: one of `normal`, `plus`, `pro`, `pro_max` | asks first: it reads or writes files on this computer |
| `file.cancelScreenshot` | Cancel Screenshot | Stop the screenshot being made; nothing is saved. | — | asks first: it reads or writes files on this computer |
| `ui.here` | Show What's Here | Opens a card with everything known about a point. Right-click the globe, or press and hold on it. | `lat` (number), required<br>`lon` (number), required | no |
| `info.view` | Describe View | The camera position, visible layers, basemap, world and units. | — | yes |
| `info.places` | List Places | Every placemark and folder in My Places, with coordinates. | — | yes |
| `info.ruler` | Measurement | The ruler's points, total distance and last bearing. | — | yes |
| `info.here` | What's Here | Coordinates in every format, ground height, country, state, municipality, nearest place, time and sun at a point. | `lat` (number), required<br>`lon` (number), required | yes |

## Map

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `data.add` | Add Data Layer | Add a library source, a web map or a GeoJSON, GPX or CSV feed as a layer. | `attribution` (string)<br>`format` (string): one of `geojson`, `gpx`, `csv`<br>`kind` (string): one of `tiles`, `wms`, `features`<br>`layers` (string): WMS layer names, comma-separated<br>`max_zoom` (integer)<br>`name` (string)<br>`preset` (string): one of `gebco`, `emodnet_bathymetry`, `gibs_true_colour`, `forest_loss`, `earthquakes`: A library source: GEBCO seafloor, EMODnet European bathymetry, NASA GIBS yesterday's true colour, Global Forest Watch tree cover loss, USGS earthquakes of the past week<br>`refresh_minutes` (integer): Reload a feature URL this often; 0 loads it once<br>`size_by` (string): For features: the property points are sized and ranked by (for example mag or population); left out, the usual ones are found<br>`url` (string): Instead of a preset: a web address. For tiles a template with {z}, {x} and {y}; for a WMS its service address; for features a GeoJSON, GPX or CSV file | asks first when it names a web address |
| `data.search` | Search Data Library | Find sources in the Add Data library by name, description or category. | `query` (string): Words to look for in the library's names, descriptions and categories (ocean, earth, hazards); empty lists the whole library | yes |
| `data.show` | Show User Layer | Show or hide a layer you added. | `id` (integer), required: A user layer's id (info.view lists them)<br>`visible` (boolean): Omit to toggle | yes |
| `data.opacity` | User Layer Opacity | Set how opaque a layer you added is. | `id` (integer), required<br>`opacity` (number), required: 0 (invisible) to 1 (opaque) | yes |
| `data.move` | Move User Layer | Draw a layer you added over or under its neighbour. | `direction` (string): one of `up`, `down`: Up draws it over the next layer<br>`id` (integer), required | yes |
| `data.zoomTo` | Zoom To User Layer | Fly to the area a feature layer covers. | `id` (integer), required: A feature layer's id | yes |
| `data.remove` | Remove User Layer | Remove a layer you added. | `id` (integer), required | asks first: it discards work or closes GlobeCraft |
| `layer.basemap` | Basemap | Choose the imagery draped on the globe. | `basemap` (string), required: one of `satellite`, `wayback`, `streets`, `topo` | yes |
| `layer.waybackRelease` | Imagery Date | Pick an Esri World Imagery Wayback release by date. | `date` (string): Release date as listed, e.g. 2021-06-30<br>`step` (integer): +1 for an older, -1 for a newer release | yes |
| `layer.waybackCompare` | Compare Dates | Swipe between two Wayback imagery dates on either side of a draggable split; set either date, or swap them. | `date` (string): Release shown right of the split, as listed<br>`enabled` (boolean): Omit to toggle<br>`left_date` (string): Release shown left of the split, as listed<br>`split` (number): Where the split runs, 0 (left edge) .. 1 (right edge)<br>`step` (integer): +1 for an older, -1 for a newer comparison release<br>`swap` (boolean): Exchange the two sides' releases | yes |
| `layer.show` | Show Layer | Show, hide or toggle a map layer. | `layer` (string), required: one of `country_borders`, `state_borders`, `place_labels`, `municipal_borders`, `terrain`, `grid`, `grid_labels`, `time_zones`, `atmosphere`, `sunlight`, `stars`, `milky_way`, `radar`, `clouds`, `aurora`, `flights`, `vessels`, `satellites`, `roads`, `buildings`, `traffic`, `night_lights`, `railways`, `ferries`, `airports`, `points_of_interest`, `peaks`, `waters`, `sea_names`, `parks`<br>`visible` (boolean): Omit to toggle | yes |
| `layer.opacity` | Layer Opacity | Set how opaque the clouds, radar, aurora, time zones or traffic layer is (0 to 1). | `layer` (string), required: one of `clouds`, `radar`, `aurora`, `time_zones`, `traffic`<br>`opacity` (number), required | yes |
| `layer.solo` | Show Layer Alone | Show only one layer of the map's content (Alt-click it in the Layers pane); the same again, or without a layer, shows what was visible before. | `layer` (string): A layer code as in layer.show; omit to show again what was visible before | yes |
| `layer.terrainExaggeration` | Exaggeration | Set the vertical exaggeration of 3D terrain. | `value` (number), required: Vertical exaggeration, 0.5..3 | yes |
| `layer.flatLiveObjects` | Aircraft and satellites on the ground | Draw aircraft and satellites flat on the ground below them instead of at their real height. | `on` (boolean): true: aircraft and satellites drawn on the ground; false: at their height. Omit to toggle | yes |
| `layer.stateAltitude` | States visible below | Set the camera altitude below which state and province borders are drawn. | `km` (number), required: States and provinces are drawn below this camera altitude, 0..8000 km (default 1609) | yes |
| `world.earthModel` | Earth Model | Choose the WGS 84 ellipsoid or a perfect sphere for geometry and distances. | `model` (string), required: one of `wgs84_g2296`, `perfect_sphere` | yes |

## Live

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `flight.select` | Select a flight | Follows one plane: its track so far, where it flies from and to, and its position every few seconds. | `callsign` (string): Its callsign, for its route; left out, the one it reports<br>`hex` (string), required: The plane's transponder (ICAO 24-bit) address, 6 hex digits | yes |
| `satellite.select` | Select a satellite | Shows one full revolution of a satellite's orbit at its height, predicted from its orbital elements, and a card with its height and speed. | `norad` (string), required: The satellite's NORAD catalogue number, e.g. 25544 for the ISS | yes |
| `satellite.clear` | Close the satellite | Hides the selected satellite's orbit and card. | — | yes |
| `flight.clear` | Close the flight | Stops following the selected plane. | — | yes |

## View

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `world.body` | World | Show the Earth, the Moon or a custom planet. | `body` (string), required: one of `earth`, `moon`, `custom` | yes |
| `ui.explorer` | Explorer | Open the Explorer: place details, photos, narrated tours and planets. | — | no |
| `ui.pane.layers` | Layers Pane | Show the Layers pane. | — | no |
| `ui.pane.places` | Places Pane | Show My Places. | — | no |
| `ui.pane.assistant` | AI Assistant | Ask questions about the globe or tell GlobeCraft what to do, with a model of your choice. | `ask` (string) | no |
| `assistant.stop` | Stop the AI Assistant | End the assistant's current request; actions it hasn't run yet are skipped and a late reply is ignored. | — | no |
| `ui.pane.measure` | Measure Pane | Show the ruler's measurements, units and history. | — | no |
| `ui.leftPane` | Side Pane | Show or hide the pane on the left of the globe. | — | no |
| `ui.navigationControls` | Navigator | Show or hide the on-globe compass and zoom controls. | `on` (boolean): Omit to toggle | yes |
| `ui.legend` | Legend | Shows what the colours on the globe mean: a selected flight's altitudes, rain radar, aurora and places of interest. Opens by itself when one of them appears. | `on` (boolean): Omit to toggle | no |
| `ui.timeline` | Timeline | Show the timeline: the app's date and time, steps, play, and the window of past events. | `on` (boolean): Omit to toggle | no |
| `time.set` | Set time | Show the globe at a date and time, or back at the real time (now): sunlight, stars, daily imagery, Wayback and timed points follow. | `time`, required: now (back to the real time), an ISO 8601 date or date and time (UTC unless a zone is given), or seconds since 1970 | yes |
| `time.step` | Step time | Move the app's time by some hours, forward or back. | `hours` (number), required: Hours to move the app's time, negative to go back | yes |
| `time.play` | Play time | Run the app's time at a speed, such as an hour per second. | `hours_per_second` (number): How fast the time runs; left out, the timeline's speed | yes |
| `time.pause` | Pause time | Hold the app's time where it is. | — | yes |
| `time.speed` | Time speed | How fast the time runs when it plays, such as an hour per second. | `hours_per_second` (number), required: How fast the time runs when it plays; a playing time takes it at once | yes |
| `time.window` | Event window | Show timed points (earthquakes…) only from the hours before the app's time. | `hours` (number), required: Timed points (earthquakes…) show when they happened within this many hours before the app's time; 0 shows all | yes |
| `ui.tileDiagnostics` | Tile diagnostics | Show the imagery tiles in view by state, the download queues per source and the download rates. | `on` (boolean): Omit to toggle | yes |
| `ui.navigatorStyle` | Navigator style | Choose the navigator with the mini globe or the classic ring, move pad and zoom slider. | `style` (string), required: one of `globe`, `classic`: globe: mini globe in the rotation ring; classic: ring, move pad and zoom slider | yes |
| `ui.ribbon` | Collapse Ribbon | Collapse or expand the ribbon (Ctrl+F1). | — | no |
| `ui.focusNext` | Next Region | Move the keyboard focus to the next part of the window: the ribbon, the left pane, the right pane, the globe. Tab and the arrow keys then move between its controls; Enter or Space presses one. | — | no |
| `ui.focusPrevious` | Previous Region | Move the keyboard focus to the previous part of the window. | — | no |
| `ui.ribbon.pin` | Pin to Toolbar | Show a tool of a ribbon group on the ribbon, or keep it in the group's menu only. | `group` (string), required: Ribbon group id, such as home.navigate<br>`on` (boolean): Omit to toggle<br>`tool` (string), required: Command id, then a space and its parameters as JSON when it has any | no |
| `ui.quickTools.pin` | Pin to Quick Access | Add a tool to the quick-access buttons beside the place search, or remove it. | `on` (boolean): Omit to toggle<br>`tool` (string), required: Command id, then a space and its parameters as JSON when it has any | no |
| `ui.shortcut.edit` | Change Keyboard Shortcut… | Choose the keys that run a tool. | `tool` (string), required: Command id, then a space and its parameters as JSON when it has any | no |
| `ui.shortcut.set` | Set Keyboard Shortcut | Set the keys that run a tool, remove its shortcut or restore the default. | `keys` (string): Keys such as command+shift+E; empty for no shortcut; omit for the default<br>`tool` (string), required: Command id, then a space and its parameters as JSON when it has any | no |
| `ui.ribbon.resetTools` | Reset Ribbon Tools | Put the ribbon's tools, the quick-access buttons and the keyboard shortcuts back as they were at installation. | — | no |
| `ui.theme` | Theme | Follow the system, or use the dark or light theme. | `theme` (string), required: one of `system`, `dark`, `light` | yes |
| `ui.fullscreen` | Full Screen | Enter or leave full screen. | — | no |
| `ui.minimize` | Minimize | Minimize the window. | — | no |
| `ui.commandSearch` | Command Search | Find and run any command by name. | — | no |
| `settings.units` | Units | Show distances and altitudes in metric or imperial units, or follow the system. | `units` (string), required: one of `system`, `metric`, `imperial` | yes |
| `settings.experience` | Experience level | Basic shows the everyday tools; Advanced adds the Earth models, other worlds and the forms for web services and feature URLs. | `level` (string), required: one of `basic`, `advanced` | yes |
| `settings.coordinateFormat` | Coordinates | Choose how coordinates are shown. | `format` (string), required: one of `dd`, `dms`, `ddm`, `utm_ups`, `mgrs` | yes |
| `settings.language` | Language | Choose the interface language. | `language` (string), required: one of `system`, `en`, `pt-BR` | yes |
| `style.line` | Map line style | Set the color, width and dash of a kind of map line: borders, roads, railways and other transport, rivers, park outlines or time zone lines. | `color` (string): Hex colour #rrggbb or #rrggbbaa (aa is opacity)<br>`dash` (string): one of `solid`, `dashed`, `dotted`<br>`line` (string), required: one of `country`, `state`, `municipal`, `disputed`, `major_road`, `minor_road`, `roadworks`, `railway`, `service_rail`, `subway`, `tram`, `ferry`, `runway`, `taxiway`, `river`, `canal`, `park`, `time_zone`: Borders (country, state/province, municipal, disputed), roads (motorway to secondary, smaller roads, roadworks), transport (railways, yards and sidings, subways, trams, ferries, runways, taxiways), rivers, canals, park outlines or time zone lines<br>`width` (number): Line width in points, 0.5..6 | yes |
| `style.labelSize` | Name size | Set the font size of country, state, city or street names, or of the time zones' offsets. | `label` (string), required: one of `country`, `state`, `city`, `road`, `time_zone`<br>`size` (number), required: Font size in points before the map text scale, 6..24 | yes |
| `style.reset` | Reset map style | Restore the default border colors, widths, dashes and name sizes. | — | asks first: it discards work or closes GlobeCraft |

## File

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `places.open` | Open KML | Open a KML or KMZ file into My Places. | — | asks first: it reads or writes files on this computer |
| `places.save` | Save | Save My Places to the current KML file. | — | asks first: it reads or writes files on this computer |
| `places.saveAs` | Save As | Save My Places to a new KML or KMZ file. | — | asks first: it reads or writes files on this computer |
| `places.export` | Export Places | Write the places shown (ticked) and the ruler's paths to a GeoJSON or GPX file, for hiking apps, GPS devices and other map programs. My Places stays in its KML file. | `format` (string): one of `geojson`, `gpx`: When the path has no such ending<br>`path` (string): File to write, ending in .geojson or .gpx; a bare name goes into the places folder. Omit to let the user choose in a dialog | asks first: it reads or writes files on this computer |
| `ui.addData` | Add Data | Add a web map, points and lines from a file or URL, or a source from the library. | — | no |
| `ui.settings` | Settings | Open the settings window. | — | no |
| `file.exportSettings` | Export Settings… | Save every setting, added layers and saved sources included, to a file you can keep or bring to another computer. Service and assistant keys are not included. | — | no |
| `file.importSettings` | Import Settings… | Replace the current settings with ones exported earlier. Keys and open places are kept. | — | no |
| `app.workOffline` | Work Offline | Stop every download: maps, imagery and data come only from what is stored on this computer, and live layers, search, the assistant and update checks pause. | `on` (boolean): Omit to toggle | no |
| `app.quit` | Quit | Close GlobeCraft. | — | asks first: it discards work or closes GlobeCraft |

## Help

| Command | Name | What it does | Parameters | Assistant |
| --- | --- | --- | --- | --- |
| `ui.shortcuts` | Shortcuts | Show the mouse and keyboard controls. | — | no |
| `help.documentation` | Documentation | Open the GlobeCraft documentation. | — | no |
| `help.reportIssue` | Report Issue | Report a problem or suggest an idea. | — | no |
| `help.copyDiagnostics` | Copy Diagnostics | Copy the version, system, settings in use and the end of the run log, to paste into an issue. Your location and data addresses are left out. | — | no |
| `ui.about` | About | About GlobeCraft and its data sources. | — | no |
