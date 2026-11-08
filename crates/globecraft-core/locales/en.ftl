# Embedded UI catalog. Keep keys and variables in sync across locales.

kml-open-dialog-title = Open KML or KMZ

kml-save-dialog-title = Save KML or KMZ as

screenshot-empty-error = Nothing to save yet.

kml-drop-type-error = Only KML, KMZ, GeoJSON, GPX or CSV files can be dropped here.

globe-placemark-action = Add placemark here

globe-fly-to-action = Fly here

globe-ruler-point-action = Add ruler point

globe-copy-coordinates-action = Copy coordinates

globe-context-menu-hint = Click on the globe.

app-ready-status = Ready. Drag to rotate, scroll to zoom.

search-empty-status = No results found.

search-pending-status = Searching…

search-input-placeholder = Search a place, address or “lat, lon”

search-submit-action = Search

settings-open-tooltip = Settings
settings-language-label = Language
settings-language-system = System locale ({ $language })
settings-language-system-fallback = System locale ({ $language } — fallback)

navigation-compass-tooltip = Click to align north up

navigation-north-tooltip = North up

navigation-reverse-tooltip = Flip view 180°

navigation-rotate-left-tooltip = Rotate left (hold)

navigation-rotate-right-tooltip = Rotate right (hold)

navigation-pan-north-tooltip = Move north (hold)

navigation-pan-south-tooltip = Move south (hold)

navigation-pan-west-tooltip = Move west (hold)

navigation-pan-east-tooltip = Move east (hold)

navigation-zoom-in-tooltip = Zoom in (hold)

navigation-zoom-out-tooltip = Zoom out (hold)

navigation-zoom-slider-tooltip = Drag to zoom
navigation-east-tooltip = East up
navigation-south-tooltip = South up
navigation-west-tooltip = West up
navigation-ring-tooltip = Drag the ring to rotate the view
navigation-tilt-tooltip = Tilt { $degrees }°: drag to tilt the view, double-click to look straight down
navigation-globe-tooltip = Drag to turn the globe

settings-title = Settings

settings-text-section-title = Text size

settings-ui-scale-label = Buttons and panels

settings-map-scale-label = Map text (names, ruler, scale)

settings-folders-section-title = Where to save files

settings-folder-browse-action = Browse…

settings-screenshot-folder-label = Screenshots (PNG):

settings-screenshot-folder-dialog-title = Screenshots folder

settings-kml-folder-label = KML/KMZ files:

settings-kml-folder-dialog-title = KML/KMZ folder

settings-folder-creation-help = Folders are created automatically when saving.

settings-reset-action = Restore defaults

settings-cache-section-title = Map cache
settings-cache-clear-action = Clear on-disk cache
settings-cache-clear-help = Removes downloaded map tiles. They download again when needed.
settings-cache-clearing-label = Clearing cache…
settings-cache-cleared-status = On-disk tile cache cleared.
settings-cache-clear-error = Could not clear on-disk cache: { $error }
settings-cache-disk-usage = { $size } on disk
settings-cache-measuring-label = Measuring…
settings-cache-usage-unavailable = Cache size unavailable

layers-section-title = Layers

layers-search-placeholder = Search layers…
layers-search-clear = Clear the layer search
layers-search-active-only = Active only
layers-search-active-help = Show only layers that are on.
layers-search-empty = No layers match “{ $query }”.
layers-search-empty-active = No layers are on.

wayback-loading-status = Loading available dates…

wayback-older-tooltip = Older

wayback-newer-tooltip = Newer

wayback-fallback-help = Not every place has a snapshot on every date; missing areas fall back to the nearest available imagery.

layers-grid-toggle = Latitude/longitude grid

layers-country-borders-toggle = Country borders (worldwide)

layers-state-borders-toggle = States / provinces (worldwide)
layers-state-altitude-label = States visible below

layers-place-labels-toggle = Country, state and city names

layers-municipal-borders-toggle = Municipal borders


layers-boundary-attribution = Borders and names: OpenStreetMap contributors (ODbL) via OpenFreeMap and OpenMapTiles; Natural Earth (public domain) offline.

boundaries-loading-status = Loading borders and names…
boundaries-osm-loading-status = Loading OpenStreetMap borders…

layers-atmosphere-toggle = Atmosphere and stars

layers-sunlight-toggle = Real-time sunlight (day/night)
layers-stars-toggle = Stars
layers-milky-way-toggle = Milky Way (NASA background)
layers-night-lights-toggle = City lights at night (NASA Black Marble)

layers-auto-rotation-toggle = Auto-rotate

layers-flights-toggle = Real aircraft (OpenSky + ADSB.lol)
aircraft-summary-status = Real tracked aircraft: { $count } | OpenSky: { $global } | ADSB.lol: { $local }
aircraft-loading-status = loading
aircraft-zoom-in-status = zoom in
aircraft-updated-seconds-status = { $seconds } s ago
aircraft-updated-status = { $minutes } min ago
aircraft-source-error = { $source }: { $error }
aircraft-hover-tooltip = { $code }  (ICAO { $hex })
    Altitude: { $altitude }  |  Speed: { $speed }
    Heading: { $heading }  |  Vertical rate: { $vertical_rate }
    Position { $age } s old
cmd-ui-legend = Legend
cmd-ui-legend-help = Shows what the colours on the globe mean: a selected flight's altitudes, rain radar, aurora and places of interest. Opens by itself when one of them appears.
polar-imagery-attribution = Polar imagery: NASA Blue Marble and MODIS (GIBS)
antarctic-claim-united-kingdom = British Antarctic Territory (United Kingdom)
antarctic-claim-argentina = Argentine Antarctica (Argentina)
antarctic-claim-chile = Chilean Antarctic Territory (Chile)
antarctic-claim-norway = Queen Maud Land (Norway)
antarctic-claim-australia = Australian Antarctic Territory (Australia)
antarctic-claim-france = Adélie Land (France)
antarctic-claim-new-zealand = Ross Dependency (New Zealand)
antarctic-claim-unclaimed = Marie Byrd Land (unclaimed)
legend-title = Legend
legend-empty = Nothing on the globe has a colour key right now.
legend-flight-altitude = Selected flight: altitude
legend-on-ground = on the ground
legend-radar = Rain radar
legend-radar-band = { $name } ({ $range })
legend-radar-light = light
legend-radar-moderate = moderate
legend-radar-heavy = heavy
legend-radar-very-heavy = very heavy
legend-radar-extreme = extreme, hail possible
legend-aurora = Aurora: chance
legend-poi = Places of interest
poi-group-food = Food and drink
poi-group-shopping = Shopping
poi-group-lodging = Lodging
poi-group-culture = Culture
poi-group-health = Health
poi-group-education = Education
poi-group-transport = Transport
poi-group-nature = Nature and parks
poi-group-terrain = Peaks
poi-group-water = Water
poi-group-worship = Places of worship
poi-group-services = Services
cmd-flight-select = Select a flight
cmd-flight-select-help = Follows one plane: its track so far, where it flies from and to, and its position every few seconds.
cmd-flight-clear = Close the flight
cmd-flight-clear-help = Stops following the selected plane.
cmd-satellite-select = Select a satellite
cmd-satellite-select-help = Shows one full revolution of a satellite's orbit at its height, predicted from its orbital elements, and a card with its height and speed.
cmd-satellite-clear = Close the satellite
cmd-satellite-clear-help = Hides the selected satellite's orbit and card.
satellite-card-catalogue = NORAD { $norad } • { $group }
satellite-card-height = Height { $height }
satellite-card-speed = Speed { $speed }
satellite-card-over = Over { $place }
satellite-card-orbit = One orbit in { $period } min • inclination { $inclination }°
satellite-card-source = { $model ->
        [sgp4] Predicted with SGP4 from CelesTrak elements { $days } days old
       *[simple] Estimated from CelesTrak elements { $days } days old
    }
flight-card-route = { $from } → { $to }
flight-card-route-loading = Looking up the route…
flight-card-route-unknown = Route unknown
flight-card-route-unsure = The published route may not be this flight's.
flight-card-flown = Flown { $time }
flight-card-to-go = { $distance } to go
flight-card-track-loading = Loading the track…
flight-card-error = No track: { $error }
flight-card-built = built { $year }
flight-card-altitude = { $feet } ft
flight-card-gps-altitude = GPS { $feet } ft
flight-card-level = level
flight-card-vertical-rate = { $rate } ft/min
flight-card-selected-altitude = autopilot { $feet } ft
flight-card-ground-speed = { $knots } kt over ground
flight-card-indicated-airspeed = IAS { $knots } kt
flight-card-true-airspeed = TAS { $knots } kt
flight-card-mach = Mach { $mach }
flight-card-track = track { $degrees }°
flight-card-selected-heading = autopilot heading { $degrees }°
flight-card-wind = wind from { $degrees }° at { $knots } kt
flight-card-temperature = { $celsius } °C outside
flight-card-squawk = squawk { $code }
flight-card-category = category { $category }
flight-card-position-source = position by { $source }
flight-card-emergency = Emergency: { $kind }
flight-card-source = Track and route: ADSB.lol (ODbL)
layers-weather-toggle = Live precipitation radar (RainViewer)
layers-clouds-toggle = Satellite cloud map (Matt Eason)
layers-live-overlays-help = Aircraft positions come from live tracking (coverage incomplete); global updates ~17 min and nearby updates ~90 s. Cloud imagery ~3 h; rain radar ~10 min. Internet required.

navigation-section-title = Navigation

navigation-reset-north-action = North up

navigation-global-view-action = Global view

search-results-label = Search results:

placemarks-section-title = Placemarks

placemarks-name-placeholder = Name

placemarks-add-center-action = At center

placemarks-fly-to-action = Go

kml-drop-zone-hint = Drag a .kml or .kmz file here

kml-save-action = Save

kml-save-tooltip = Saves into the KML/KMZ folder (see Settings)

kml-save-as-action = Save as…

kml-open-action = Open KML/KMZ…

ruler-section-title = Ruler

ruler-enable-toggle = Measure distances (click on the globe)

ruler-clear-action = Clear ruler

export-section-title = Export

screenshot-save-action = Save screenshot (PNG)

settings-folders-action = Change folders…

controls-help-toggle = Show controls help

controls-help-text = • Drag (left): rotate the globe
    • Right-drag / Shift+drag / two-finger turn: rotate the view
    • Wheel / pinch or +/-: zoom
    • Shift+scroll / Shift+drag up or down: tilt   • U: look straight down
    • Double-click: fly to the point
    • Arrow keys: move   • N: north up
    • Right-click: context menu

placemarks-fallback-name = Unnamed

provider-satellite-label = Satellite (Esri World Imagery)
provider-night-lights-label = Earth at Night (NASA Black Marble 2016)

provider-wayback-label = Historical imagery (Esri Wayback)

provider-streets-label = Streets (OpenStreetMap)

provider-topography-label = Terrain/Topography (OpenTopoMap)

provider-imagery-attribution = Imagery © Esri, Maxar, Earthstar Geographics

provider-streets-attribution = © OpenStreetMap contributors

# Whole messages: arguments retain the original numeric formatting.

kml-save-success = Saved to { $path }

kml-save-error = Error saving: { $error }

kml-import-success =
    { $count ->
        [1] { $count } placemark imported.
       *[other] { $count } placemarks imported.
    } — { $path }

kml-open-error = Error opening: { $error }

screenshot-save-success = Screenshot saved: { $path }

screenshot-save-error = Error: { $error }

search-navigation-status = Going to { $destination }

search-results-status =
    { $count ->
        [1] { $count } result found.
       *[other] { $count } results found.
    }

search-request-error = Search error: { $error }

wayback-snapshot-attribution = { $attribution } — snapshot { $date }

placemarks-generated-name = Place { $number }

wayback-availability-status = { $count } snapshots available

export-folder-label = Folder: { $path }

ruler-measurement-summary = Points: { $count }   Total: { $distance }

statusbar-tiles-loading = loading { $count } tiles

statusbar-camera-center = Center: { $latitude }°, { $longitude }°

statusbar-camera-altitude = Camera altitude: { $distance }
statusbar-cursor-elevation = Elevation: { $distance }


settings-uncap-fps-toggle = Uncap FPS (restart required)
settings-gpu-globe-toggle = Draw the globe on the GPU (beta, restart required)
settings-gpu-globe-help = Faster on Apple Silicon. Close views, the grid and any GPU problem still use the CPU renderer. Turn this off if the globe looks wrong.
settings-frame-sync-help = By default, presentation synchronizes with the display refresh rate. G-SYNC/FreeSync is configured in your display and graphics driver.

# Explorer / Visualização e novas camadas — conteúdo bilíngue
top-explore-action = Explore +
top-earth = Earth
top-moon = Moon
top-custom-planet = Custom planet
explorer-title = Explore · places, sky and planets
explorer-selected-location = Selected location
explorer-search-place = Search for a place to view information
explorer-latlon = Latitude { $latitude } · Longitude { $longitude }
explorer-center = Center: { $latitude }, { $longitude }
explorer-place-photos = Place photos
explorer-photos-opened = Photo gallery opened in browser
explorer-view-osm = View on OpenStreetMap
explorer-map-opened = Map opened in browser
explorer-photo-copyright = Photos are searched on Wikimedia Commons and copyright belongs to their creators. Place details open in your browser; these are not Google Maps reviews.
explorer-narrated-tour = Narrated tour (TTS)
explorer-tour-default = We are exploring { $place }. Explore the location on the globe and choose points of interest to continue your tour.
explorer-read-aloud = ▶ Read text aloud
explorer-speech-sent = Text sent to speech synthesizer
explorer-globe-layers = Globe layers
explorer-layer-info = These georeferenced layers are drawn on the globe using online data (internet required).
explorer-cloud-credit = Cloud imagery: Matt Eason / Live Cloud Maps
explorer-cloud-source = Live Cloud Maps — author and source code
explorer-cloud-license = Contains modified EUMETSAT data · CC0 1.0
explorer-radar-credit = Radar source: RainViewer
explorer-radar-source = RainViewer weather radar
explorer-radar-explanation = Green/yellow/red indicates approximate rain intensity (radar reflectivity), not infrared satellite temperature. Coverage and freshness vary.
explorer-bodies = Celestial bodies
explorer-earth = Earth
explorer-moon = Moon
explorer-custom-planet = Custom planet
explorer-moon-fixed = The Moon is fixed and cannot be edited. The settings below apply only to the custom planet.
explorer-name = Name:
explorer-image = Image:
explorer-browse = Browse
explorer-display-planet = Display planet image
explorer-planet-displayed = Planet displayed on the globe
explorer-save-planet = Save planet…
explorer-load-planet = Load planet…
explorer-planet-loaded = Planet and sky loaded on the globe
explorer-planet-saved = Planet and sky embedded in { $path }
explorer-file-working = Processing file…
explorer-embedded-image = Embedded image
explorer-sky-galaxy = Custom sky and galaxy
explorer-image-path = Image file path
explorer-display-galaxy = Display galaxy
explorer-sky-applied = Sky applied to the globe background
explorer-sky-preview = The custom sky is previewed here; it is not used as the Earth renderer skybox.
live-clouds-failed = Cloud update failed
live-cached-image =  (cached image shown)
live-clouds-ok = Clouds: Matt Eason (3-hour updates)
live-clouds-loading = Clouds: loading satellite imagery…
live-radar-failed = Radar update failed
live-radar-age = Rain radar: observation approximately { $minutes } min old
live-radar-loading = Rain radar: loading RainViewer data…
sidebar-cloud-source = Cloud data: Matt Eason / EUMETSAT
sidebar-radar-source = Radar data: RainViewer
live-cloud-credit = Matt Eason · Live Cloud Maps · Contains modified EUMETSAT data
live-radar-credit = Precipitation radar: RainViewer (rainviewer.com)

# Native desktop menus
menu-file-title = File
menu-kml-open = Open KML/KMZ…
menu-add-data = Add Data…
menu-kml-save = Save KML/KMZ
menu-kml-save-as = Save KML/KMZ As…
menu-screenshot-save = Save Screenshot
menu-quit-action = Quit GlobeCraft


settings-distance-units-label = Distance units
settings-units-system = System locale ({ $unit })
settings-units-metric = Kilometers / meters
settings-units-imperial = Miles / feet

ruler-unit-automatic = Automatic
ruler-unit-centimeters = Centimeters
ruler-unit-meters = Meters
ruler-unit-kilometers = Kilometers
ruler-unit-inches = Inches
ruler-unit-feet = Feet
ruler-unit-yards = Yards
ruler-unit-miles = Miles
ruler-unit-nautical-miles = Nautical miles
ruler-unit-smoots = Smoots
ruler-unit-degrees = Degrees
ruler-unit-arcseconds = Arcseconds
ruler-unit-football-fields = Football fields
ruler-unit-american-football-fields = American football fields
ruler-units-label = Units
ruler-distance-smoots = { $value } smoots
ruler-distance-football-fields = { $value } football fields
ruler-unit-automatic-help = Use the distance units selected in Settings.
ruler-unit-centimeters-help = 1 centimeter = 0.01 meters.
ruler-unit-meters-help = 1 meter = 100 centimeters.
ruler-unit-kilometers-help = 1 kilometer = 1,000 meters.
ruler-unit-inches-help = 1 inch = 2.54 centimeters.
ruler-unit-feet-help = 1 foot = 12 inches.
ruler-unit-yards-help = 1 yard = 3 feet.
ruler-unit-miles-help = 1 mile = 1,760 yards.
ruler-unit-nautical-miles-help = 1 nautical mile = 1,852 meters.
ruler-unit-smoots-help = 1 smoot = 67 inches.
ruler-unit-degrees-help = Central angle along the measured path on the globe.
ruler-unit-arcseconds-help = 1 arcsecond = 1/3,600 of a degree of central angle.
ruler-unit-football-fields-help = One American football field is 100 yards, excluding end zones.

altitude-edit-tooltip = Set an exact camera altitude
altitude-edit-title = Camera altitude
altitude-apply-action = Apply
altitude-cancel-action = Cancel
altitude-input-error = Enter a number between { $minimum } and { $maximum } { $unit }.

settings-hide-navigation-controls-toggle = Hide navigation controls
settings-navigator-style-label = Navigator:
settings-navigator-globe = Globe
settings-navigator-classic = Classic

settings-close-action = Close

ruler-point-edit-help = Alt-click: new path · Drag: move · Hover + Delete: remove
ruler-point-edit-help-macos = Option-click: new path · Drag: move · Hover + Delete: remove
compass-north = N
compass-northeast = NE
compass-east = E
compass-southeast = SE
compass-south = S
compass-southwest = SW
compass-west = W
compass-northwest = NW
compass-heading = { $degrees }° { $direction }
compass-heading-tooltip = Direction at the top of the map, clockwise from true north. Click to reset north.
layers-grid-numbers-toggle = Show numbers
ruler-bearing-label = Last segment bearing: { $bearing }
ruler-bearing-tooltip = Initial geodesic bearing from the previous point to the last point, clockwise from true north.
grid-coordinate-label = { $degrees }° { $direction }

menu-edit-title = Edit
edit-undo-action = Undo
edit-redo-action = Redo

ruler-points-title = Ruler coordinates
ruler-path-heading = Path { $number }
ruler-point-row = { $number }: { $latitude }°, { $longitude }°
ruler-point-edit-mode = Edit selected point
ruler-point-add-mode = Add a point
ruler-point-latitude = Latitude (°)
ruler-point-longitude = Longitude (°)
ruler-point-new-path-toggle = Start a separate path
ruler-point-input-help = Decimal degrees. Negative latitude is south; negative longitude is west.
ruler-point-apply-action = Apply
ruler-point-add-action = Add point
ruler-point-new-action = New point
ruler-point-close-action = Close

settings-earth-model-label = Earth model
earth-model-sphere = Perfect Sphere
earth-model-wgs84 = Realistic — WGS 84 (G2296)
earth-model-sphere-help = Original mean-radius sphere. Optional alternative to the realistic Earth model.
earth-model-wgs84-help = Default Earth model, with WGS 84 ellipsoid geometry and geodesics. G2296 reference frame: EPSG:10606 (2D), EPSG:10605 (3D); no epoch transformations, tectonic motion, terrain or geoid heights. Uses the CPU globe renderer.

places-root-name = My Places
places-document-name = Document
places-folder-name = New folder
places-add-folder-action = + Folder
places-clear-all-action = Clear All
places-delete-action = Delete
places-visible-help = Show this place and its children on the globe
places-empty-help = Open a KML or KMZ file or add a place. Drag places into folders to organize them.
places-description-placeholder = Description

# Worldwide layers and the 3D Easter egg (merged)
flat-earth-title = ✦ SECRET FLAT EARTH
flat-earth-unlocked = Flat-Earth Easter egg unlocked! Press Esc to return.
flat-earth-disclaimer = A visual joke, not a scientific Earth model.
flat-earth-hint = Drag: rotate · Shift: pan · Scroll: zoom · Double-click: reset · Esc: exit
flat-earth-return = Back to globe
flat-earth-image-error = Could not load the hidden illustration.
layers-vessels-toggle = Worldwide AIS ships
layers-aurora-toggle = Aurora (NOAA forecast)
ribbon-layer-aurora = Aurora
aurora-loading-status = Loading the aurora forecast…
aurora-loaded-status = Forecast for { $time } (in { $minutes } min) • highest chance { $peak } % • shown near the present only
aurora-credit = Aurora: NOAA SWPC
aircraft-credit = Aircraft: OpenSky Network · ADSB.lol (ODbL)
vessels-credit = Ships: OpenSeaFeed
vessels-fallback-credit = Ships: Fintraffic Digitraffic (CC BY 4.0)
satellites-credit = Satellites: CelesTrak
traffic-credit = Traffic: © TomTom
aurora-source = NOAA Space Weather Prediction Center (OVATION)
layers-timezones-toggle = Time zones
ribbon-layer-timezones = Time zones
timezones-loading-status = Loading the time zones…
timezones-loaded-status = { $count } time zones • the local time under the cursor shows in the status bar
timezones-source = timezone-boundary-builder (OpenStreetMap, ODbL)
timezones-credit = Time zones: timezone-boundary-builder, © OpenStreetMap contributors
statusbar-cursor-local-time = Local time: { $time }
layers-satellites-toggle = Satellites (predicted CelesTrak orbits)
layers-traffic-toggle = Live congestion (TomTom; optional key)
layers-traffic-key = TomTom API key (not stored)
layers-roads-toggle = Roads (OSM)
layers-railways-toggle = Railways, subways and trams (OSM)
layers-ferries-toggle = Ferry routes (OSM)
layers-airports-toggle = Airports (OSM)
layers-poi-toggle = Places of interest (OSM)
layers-peaks-toggle = Mountain peaks (OSM)
layers-waters-toggle = Rivers and water names (OSM)
layers-parks-toggle = Parks and nature reserves (OSM)
transport-zoom-help = Zoom in to see them (below { $altitude } altitude)
transport-loading-status = Loading…
transport-error-status = Map data not available: { $error }

roads-zoom-help = Zoom in for roads (below { $altitude } altitude)
roads-legend = Gray = traffic unknown; red = under construction (NOT a traffic jam)

vessels-loading-status = Loading worldwide AIS ships…
vessels-world-status = { $count } vessels • worldwide OpenSeaFeed • partial reception, reports up to 90 min old
vessels-custom-status = { $count } vessels • custom AIS GeoJSON • provider coverage
vessels-fallback-status = { $count } vessels • Fintraffic fallback (Finland only); global feed unavailable
vessels-fallback-source = Fintraffic (fallback)
satellites-loading-status = Loading orbits…
satellites-loaded-status = { $count } satellites • SGP4 predictions from CelesTrak elements (not telemetry)
satellites-together = Together with: { $names }
satellite-group-station = Space stations
satellite-group-navigation = Navigation (GPS, Galileo, GLONASS, BeiDou)
satellite-group-weather = Weather
satellite-group-bright = Brightest (visible to the eye)
aircraft-class-unknown = Aircraft (kind not reported)
aircraft-class-airliner = Airliner or business jet
aircraft-class-heavy = Heavy jet
aircraft-class-light = Light aircraft
aircraft-class-fast = High-performance jet
aircraft-class-helicopter = Helicopter
aircraft-class-glider = Glider
aircraft-class-balloon = Balloon or airship
aircraft-class-ultralight = Ultralight or hang glider
aircraft-class-drone = Drone
vessel-class-cargo = Cargo
vessel-class-tanker = Tanker
vessel-class-passenger = Passenger
vessel-class-fishing = Fishing
vessel-class-high-speed = High-speed craft
vessel-class-tug = Tug or pilot boat
vessel-class-service = Service vessel
vessel-class-military = Military or law enforcement
vessel-class-pleasure = Pleasure craft or sailing
vessel-class-other = Other
vessel-class-unknown = Not reported
legend-aircraft = Aircraft
legend-vessels = Ships (larger = longer)
legend-satellites = Satellites
traffic-key-help = Enter a TomTom key for real traffic
traffic-zoom-help = Zoom in to see traffic
traffic-loading-status = Loading live traffic…
traffic-loaded-status = Traffic updated { $minutes } min ago (green=free, yellow=slow, red=jammed)

vessels-unnamed = Unidentified vessel
vessels-hover-tooltip =
    { $name } • MMSI { $mmsi }
    { $ship_type ->
        [fishing] Fishing vessel
        [tug] Tug or pilot boat
        [military] Military or law-enforcement vessel
        [pleasure] Pleasure craft or sailing vessel
        [passenger] Passenger ship
        [cargo] Cargo ship
        [tanker] Tanker
        [high_speed] High-speed craft
        [service] Service vessel
        [other] Other vessel
       *[unknown] Vessel type not reported
    } • Destination: { $destination }
    Size: { $dimensions }  |  Draught: { $draught }
    { $latitude }°, { $longitude }° • { $course } • { $speed } kt
    Last AIS report { $minutes } min ago • coverage varies
satellites-hover-tooltip =
    { $name } (NORAD { $norad }) • { $group }
    Period { $period } • Altitude { $altitude } • Inclination { $inclination }
    { $model ->
        [sgp4] Predicted with SGP4 from CelesTrak elements
       *[simple] Estimated orbital position • CelesTrak
    }
transport-airport-hover-tooltip =
    { $name }
    IATA { $iata } • ICAO { $icao }
    { $class ->
        [international] International airport
        [public] Public airport
        [regional] Regional airport
        [military] Military airfield
        [private] Private airfield
       *[other] Airfield
    }

update-available-label = Update to v{ $version }
update-available-tooltip = Update available: v{ $version } — click to download and install
update-downloading-label = Downloading update
update-progress-tooltip = Downloading update: { $percent }%
update-installing-label = Installing update
update-installing-tooltip = Verifying and installing update
update-restart-label = Restart to update
update-restart-tooltip = Update installed: v{ $version } — click to save your work and restart
update-retry-label = Retry update
update-releases-action = Open releases
update-saving-label = Saving before restart

sidebar-explore-title = Globe
sidebar-data-title = Data
data-pane-favorites-title = Favorites
data-pane-favorites-empty = Star a source to keep it here.
data-pane-recent-title = Recent
data-pane-recent-empty = Sources you add appear here.
data-pane-services-title = Library
data-pane-mine-title = My data
data-pane-star-help = Add to favorites
data-pane-unstar-help = Remove from favorites
data-pane-open-layers-action = Order, opacity and removal in Layers
data-pane-export-help = Export favorites as a file
data-pane-import-help = Import favorites from a file
data-pane-export-empty = Star a source first: only favorites are exported.
data-pane-export-success = Exported { $count } { $count ->
        [one] source
       *[other] sources
    } to { $path }
data-pane-import-success = Added { $count } { $count ->
        [one] source
       *[other] sources
    } to your favorites from { $path }
sidebar-places-title = Places
sidebar-pin-places-action = Pin
sidebar-pin-places-tooltip = Keep Places visible when switching panels
sidebar-unpin-places-tooltip = Unpin Places
sidebar-places-files-title = KML / KMZ files
sidebar-imagery-title = Base imagery

sidebar-geography-title = Borders, labels & grid
sidebar-transport-title = Transport
sidebar-weather-title = Weather
sidebar-live-objects-title = Aircraft, ships & satellites
sidebar-sky-title = Sky & lighting

menu-application-title = GlobeCraft
menu-view-title = View
menu-add-title = Add
menu-tools-title = Tools
menu-window-title = Window
menu-help-title = Help
menu-about-action = About GlobeCraft
menu-settings-action = Settings…
menu-sidebar-toggle = Sidebar
menu-navigation-toggle = Navigation controls
menu-add-placemark = Placemark at camera center
menu-fullscreen-toggle = Fullscreen
menu-minimize-action = Minimize
menu-controls-help = Controls and keyboard shortcuts
menu-documentation-action = Documentation
menu-report-issue-action = Report an issue…
about-version-label = Version { $version }
about-description = Explore the globe, live layers and your geographic places.
about-project-link = Project and source code

coordinates-format-label = Coordinates
coordinates-format-dd = Decimal degrees (DD)
coordinates-format-dms = Degrees, minutes, seconds (DMS)
coordinates-format-ddm = Degrees, decimal minutes (DDM)
coordinates-format-utm = UTM / UPS
coordinates-format-mgrs = MGRS
coordinates-precision-degrees = Decimal places in degrees
coordinates-precision-seconds = Decimal places in seconds
coordinates-precision-minutes = Decimal places in minutes
coordinates-precision-meters = Decimal places in meters
coordinates-precision-grid = MGRS digits per axis
coordinates-cursor-readout = Cursor: { $coordinates }
coordinates-camera-readout = Center: { $coordinates }
coordinates-copy-tooltip = { $coordinates }
    Click to copy coordinates.
coordinates-input-help = Search accepts DD, DMS, DDM, UTM/UPS and MGRS. UTM input: zone N/S easting northing. Grid precision describes display resolution, not source accuracy.

settings-category-general = General
settings-category-appearance = Appearance
settings-category-navigation = Navigation
settings-category-globe = Globe
settings-category-graphics = Graphics
settings-category-cache = Data & Cache
settings-category-network = Network
settings-category-files = Files & Export
settings-search-placeholder = Search settings…
settings-search-empty = No settings match “{ $query }”.
settings-theme-label = Theme
settings-theme-system = System
settings-theme-dark = Dark
settings-theme-light = Light
settings-ui-font-label = Interface font
settings-map-font-label = Map label font
settings-font-system = System default
settings-fonts-loading = Finding installed fonts…
settings-cache-budgets-label = Imagery cache budgets
settings-cache-decoded-label = Decoded memory target
settings-cache-disk-label = Encoded disk target
settings-cache-restart-help = Budget changes apply after restarting. Memory eviction is best effort; frame references and other app allocations are additional. Disk counts managed imagery files, including SQLite overhead.
settings-cache-borders-label = Map data cache on disk (each set)
settings-cache-borders-usage = Borders and names: { $size }; transport, roads, places and points of interest: { $vector_size }. Applies at once; the tiles used longest ago are dropped first.
settings-cache-downloads = Downloaded this session: imagery for the view { $imagery } ({ $imagery_tiles } tiles), imagery loaded ahead { $ahead } ({ $ahead_tiles } tiles), borders and names { $borders } ({ $border_tiles } tiles). Read from the disk cache instead: { $disk } ({ $disk_tiles } tiles).
settings-cache-resident-label = Current decoded cache: { $size }
settings-cache-confirm = Clear downloaded map imagery? Settings and geographic documents are kept.
settings-confirm-action = Confirm
settings-cancel-action = Cancel
settings-reset-confirm = Restore all settings to defaults? Open geographic documents and the language override are kept. Cache budgets and frame synchronization take effect after restart.
settings-pan-sensitivity = Drag pan sensitivity
settings-rotation-sensitivity = Drag rotation sensitivity
settings-zoom-sensitivity = Wheel zoom sensitivity
settings-invert-scroll = Reverse wheel zoom
settings-startup-view-label = Startup view
settings-startup-current-action = Use current view
settings-startup-default-action = Use default view
settings-startup-help = The saved position, heading and camera altitude are used at the next launch.

settings-network-intro = Hosts GlobeCraft may contact. Built-in services show their request limit and user agent; layers you add and assistant providers come from your settings. Limits describe how GlobeCraft paces requests, not a guarantee from the provider.
settings-network-group-basemaps = Maps and imagery
settings-network-group-map-data = Map data
settings-network-group-search = Search
settings-network-group-live = Live layers
settings-network-group-updates = Updates
settings-network-group-user = Your data
settings-network-group-assistant = Assistant
settings-network-service-wayback = Esri Wayback release list
settings-network-service-clouds = Live Cloud Maps (Matt Eason, EUMETSAT)
settings-network-service-map-data = GlobeCraft map data
settings-network-service-updates = GlobeCraft updates
settings-network-limit-parallel = Up to { $count } requests at once
settings-network-limit-seconds = Normally one request every { $seconds } s
settings-network-limit-minutes = Normally one request every { $minutes } min
settings-network-limit-hours = Normally one request every { $hours } h
settings-network-limit-on-demand = Only when needed
settings-network-limit-once = Once per session
settings-network-limit-user-controlled = At the interval you set
settings-network-host-shared = Together with the other sources on { $host }: up to { $count } at once
settings-network-tiles-on-demand = Visible tiles only
settings-network-tiles-prefetch = Visible tiles first; nearby tiles may be prefetched
settings-network-tile-agent = Tile requests send the user agent: { $agent }
settings-network-agent = User agent: { $agent }
settings-network-agent-updater = The updater library sends its own user agent.
settings-network-user-layer-none = No user-added layer contacts a network host.
settings-network-user-layer-host = { $name }: { $host }
settings-network-user-layer-reload = { $name }: { $host } — reloads every { $minutes } min
settings-network-user-layer-help = These addresses come from the layers you added. Files on this computer are not sent anywhere.
settings-network-assistant-none = No assistant provider is configured. The assistant contacts nothing until you choose one.
settings-network-assistant-selected = { $provider }: { $host }
settings-network-assistant-invalid = { $provider }: the configured address is not a usable host
settings-network-assistant-help = The assistant uses only this provider. Prompts, the commands it proposes and its API key go there; a provider on localhost or your local network stays on your network.

settings-category-controllers = Controllers
controllers-enabled = Enable controllers
controllers-help = Explore navigation: left stick pans, right stick X rotates, triggers zoom. Return controls to neutral after closing dialogs or reconnecting.
controllers-disconnected = No selected device connected
controllers-action-none = Unbound
controllers-action-cancel = Cancel camera movement
controllers-left-x = Left stick X
controllers-left-y = Left stick Y
controllers-right-x = Right stick X
controllers-right-y = Right stick Y
controllers-left-trigger = Left trigger
controllers-right-trigger = Right trigger
controllers-pan-x = Horizontal pan
controllers-pan-y = Vertical pan
controllers-heading = Heading
controllers-zoom-out = Zoom out
controllers-zoom-in = Zoom in
controllers-reset = Reset selected device profile
controllers-listening = Listening for 10 seconds… Release, then move an axis or press the desired button.
controllers-conflicts-help = Axis assignments may be shared intentionally. Binding a button replaces its previous action only after confirmation. Button names follow the normalized device layout.
controllers-conflict = { $button } already runs “{ $action }”. Replace this binding?
controllers-replace = Replace binding
controllers-bind = Press to bind
controllers-invert = Invert
controllers-deadzone = Deadzone
controllers-sensitivity = Sensitivity
controllers-curve = Response curve
controllers-buttons = Button actions
controllers-monitor = Live normalized inputs

sky-cinematic = Cinematic sky (visible during daylight)
sky-help = The Milky Way is NASA’s diffuse map, prepared as a 4K SDR background; Stars draws point stars over it. No selectable star catalog or astronomical navigation.
sky-loading = Preparing celestial background…
sky-retry = Retry background loading
sky-short-credit = Sky: NASA SVS · Gaia DR2 (ESA/Gaia/DPAC)
    { $sky }
sky-software-credits = Celestial transform software licenses

# Native Rust renderer terrain (no browser)
layers-terrain-toggle = Real 3D terrain (Mapterhorn)
layers-terrain-exaggeration = Vertical exaggeration
layers-terrain-help = Terrain on the globe. Zoom below 150 km altitude; elevation tiles need internet.

# GlobeCraft 0.3 interface
ribbon-tab-home = Home
ribbon-tab-layers = Layers
ribbon-tab-live = Live
ribbon-tab-view = View
ribbon-tab-places = Places
ribbon-tab-settings = Settings
ribbon-tab-help = Help
layers-globe-title = Terrain & grid
cmd-camera-fly-to = Fly To
cmd-camera-fly-to-help = Fly the camera to a latitude and longitude, optionally at an altitude and heading.
cmd-camera-set = Go To XY
cmd-camera-set-help = Move the camera instantly; parameters left out keep their current value.
cmd-search-place = Find Place
cmd-search-place-help = Search for a place, an address or coordinates (OpenStreetMap Nominatim) and fly to the best result.
cmd-camera-zoom-in = Zoom In
cmd-camera-zoom-in-help = Move the camera closer to the surface.
cmd-camera-zoom-out = Zoom Out
cmd-camera-zoom-out-help = Move the camera away from the surface.
cmd-camera-reset-north = North Up
cmd-camera-reset-north-help = Turn the view so north points up (N).
cmd-camera-tilt = Tilt
cmd-camera-tilt-help = Lean the view toward the horizon, or back (Shift+scroll, Shift+drag or right-drag up and down, Shift+Up/Down). Close to the ground the camera backs off as far as the tilt needs.
cmd-camera-top-down = Top-Down View
cmd-camera-top-down-help = Look straight down again (U).
cmd-camera-global-view = Global View
cmd-camera-global-view-help = Fly out until the whole globe is in view.
cmd-camera-auto-rotate = Auto-Rotate
cmd-camera-auto-rotate-help = Slowly spin the globe.
cmd-places-add-center = Placemark
cmd-places-add-center-help = Add a placemark at the centre of the view and edit its name.
cmd-places-add = Add Placemark At
cmd-places-add-help = Add a placemark at a latitude and longitude.
cmd-places-add-folder = New Folder
cmd-places-add-folder-help = Add a folder to My Places.
cmd-places-fly-to = Fly To Place
cmd-places-fly-to-help = Fly to a placemark by its name.
cmd-places-remove = Remove Place
cmd-places-remove-help = Remove a placemark or folder by its name.
cmd-places-open = Open KML
cmd-places-open-help = Open a KML or KMZ file into My Places.
cmd-places-save = Save
cmd-places-save-help = Save My Places to the current KML file.
cmd-places-save-as = Save As
cmd-places-save-as-help = Save My Places to a new KML or KMZ file.
cmd-places-clear = Clear All
cmd-places-clear-help = Remove every place and forget the open KML files (the files are kept).
cmd-ruler-enable = Ruler
cmd-ruler-enable-help = Measure geodesic distances: click the globe to add points, Alt-click to start a new path.
cmd-ruler-add-point = Add Ruler Point
cmd-ruler-add-point-help = Add a measuring point at a latitude and longitude.
cmd-ruler-clear = Clear Ruler
cmd-ruler-clear-help = Remove every measuring point.
cmd-ruler-close-shape = Close Shape
cmd-ruler-close-shape-help = Join each ruler path of three points or more back to its first point and measure the area it encloses.
cmd-ruler-profile = Elevation Profile
cmd-ruler-profile-help = The ground's height along the ruler: lowest and highest point, climbing, descending and the heights along the way.
cmd-ruler-unit = Ruler Unit
cmd-ruler-unit-help = Choose the unit measurements are shown in.
cmd-ruler-edit-points = Edit Points
cmd-ruler-edit-points-help = Type exact coordinates for ruler points.
cmd-edit-undo = Undo
cmd-edit-undo-help = Undo the last ruler edit.
cmd-edit-redo = Redo
cmd-edit-redo-help = Redo the last undone ruler edit.
cmd-file-screenshot = Screenshot
cmd-file-screenshot-help = Save the globe without the interface as a PNG in the screenshot folder.
cmd-view-copy-link = Copy View Link
cmd-view-copy-link-help = Copy a globecraft:// link to this view (place, height, angle, basemap, layers and web data). Paste it into the search box, or send it to someone, to open the same view.
cmd-view-open-link = Open View Link
cmd-view-open-link-help = Open a globecraft:// view link: its place, height and angle, basemap, layers and web data.
view-link-copied-status = View link copied.
view-link-opened-status = View link opened.
cmd-file-screenshot-quality = Screenshot Quality
cmd-file-screenshot-quality-help = Choose the screenshot quality: Normal (screen size, once every tile has loaded), Plus (twice the size, one zoom level deeper), Pro (four times the size, two levels deeper) or Pro Max (eight times the size, three levels deeper).
screenshot-quality-normal = Normal
screenshot-quality-plus = Plus
screenshot-quality-pro = Pro
screenshot-quality-pro-max = Pro Max
cmd-file-cancel-screenshot = Cancel Screenshot
cmd-file-cancel-screenshot-help = Stop the screenshot being made; nothing is saved.
screenshot-cancelled-status = Screenshot cancelled.
screenshot-none-running-error = No screenshot is being made.
screenshot-rendering-status = Making the { $quality } screenshot: { $percent } %, { $tiles } tiles to load…
cmd-ui-add-data = Add Data
cmd-ui-add-data-help = Add a web map, points and lines from a file or URL, or a source from the library.
cmd-data-add = Add Data Layer
cmd-data-add-help = Add a library source, a web map or a GeoJSON, GPX or CSV feed as a layer.
cmd-data-search = Search Data Library
cmd-data-search-help = Find sources in the Add Data library by name, description or category.
cmd-data-show = Show User Layer
cmd-data-show-help = Show or hide a layer you added.
cmd-data-opacity = User Layer Opacity
cmd-data-opacity-help = Set how opaque a layer you added is.
cmd-data-move = Move User Layer
cmd-data-move-help = Draw a layer you added over or under its neighbour.
cmd-data-zoom-to = Zoom To User Layer
cmd-data-zoom-to-help = Fly to the area a feature layer covers.
cmd-data-remove = Remove User Layer
cmd-data-remove-help = Remove a layer you added.
cmd-info-here = What's Here
cmd-info-here-help = Coordinates in every format, ground height, country, state, municipality, nearest place, time and sun at a point.
cmd-ui-here = Show What's Here
cmd-ui-here-help = Opens a card with everything known about a point. Right-click the globe, or press and hold on it.
cmd-layer-basemap = Basemap
cmd-layer-basemap-help = Choose the imagery draped on the globe.
cmd-layer-wayback-release = Imagery Date
cmd-layer-wayback-release-help = Pick an Esri World Imagery Wayback release by date.
cmd-layer-wayback-compare = Compare Dates
cmd-layer-wayback-compare-help = Swipe between two Wayback imagery dates on either side of a draggable split; set either date, or swap them.
wayback-compare-label = Compare with
wayback-compare-tooltip = Swipe between two imagery dates
wayback-compare-swap-tooltip = Swap the two dates
cmd-layer-show = Show Layer
cmd-layer-show-help = Show, hide or toggle a map layer.
cmd-layer-opacity = Layer Opacity
cmd-layer-opacity-help = Set how opaque the clouds, radar, aurora, time zones or traffic layer is (0 to 1).
cmd-layer-solo = Show Layer Alone
cmd-layer-solo-help = Show only one layer of the map's content (Alt-click it in the Layers pane); the same again, or without a layer, shows what was visible before.
layers-opacity-label = Opacity
layers-solo-note = Only { $layer } is shown.
layers-solo-show-all = Show all again
cmd-layer-terrain-exaggeration = Exaggeration
cmd-layer-terrain-exaggeration-help = Set the vertical exaggeration of 3D terrain.
cmd-layer-flat-live-objects = Aircraft and satellites on the ground
cmd-layer-flat-live-objects-help = Draw aircraft and satellites flat on the ground below them instead of at their real height.
cmd-layer-state-altitude = States visible below
cmd-layer-state-altitude-help = Set the camera altitude below which state and province borders are drawn.
cmd-world-earth-model = Earth Model
cmd-world-earth-model-help = Choose the WGS 84 ellipsoid or a perfect sphere for geometry and distances.
cmd-world-body = World
cmd-world-body-help = Show the Earth, the Moon or a custom planet.
cmd-ui-explorer = Explorer
cmd-ui-explorer-help = Open the Explorer: place details, photos, narrated tours and planets.
cmd-ui-pane-layers = Layers Pane
cmd-ui-pane-layers-help = Show the Layers pane.
cmd-ui-pane-places = Places Pane
cmd-ui-pane-places-help = Show My Places.
cmd-ui-pane-explore = Globe Pane
cmd-ui-pane-explore-help = Show search results, navigation and export options.
cmd-ui-pane-data = Data Pane
cmd-ui-pane-data-help = Show your favorite and recent sources, the data library and your layers.
cmd-ui-pane-assistant = AI Assistant
cmd-ui-pane-assistant-help = Ask questions about the globe or tell GlobeCraft what to do, with a model of your choice.
cmd-assistant-stop = Stop the AI Assistant
cmd-assistant-stop-help = End the assistant's current request; actions it hasn't run yet are skipped and a late reply is ignored.
cmd-ui-pane-measure = Measure Pane
cmd-ui-pane-measure-help = Show the ruler's measurements, units and history.
cmd-ui-left-pane = Side Pane
cmd-ui-left-pane-help = Show or hide the pane on the left of the globe.
cmd-ui-navigation-controls = Navigator
cmd-ui-navigation-controls-help = Show or hide the on-globe compass and zoom controls.
ribbon-group-time = Time
cmd-ui-timeline = Timeline
cmd-ui-timeline-help = Show the timeline: the app's date and time, steps, play, and the window of past events.
cmd-time-set = Set time
cmd-time-set-help = Show the globe at a date and time, or back at the real time (now): sunlight, stars, daily imagery, Wayback and timed points follow.
cmd-time-step = Step time
cmd-time-step-help = Move the app's time by some hours, forward or back.
cmd-time-play = Play time
cmd-time-play-help = Run the app's time at a speed, such as an hour per second.
cmd-time-pause = Pause time
cmd-time-pause-help = Hold the app's time where it is.
cmd-time-speed = Time speed
cmd-time-speed-help = How fast the time runs when it plays, such as an hour per second.
cmd-time-window = Event window
cmd-time-window-help = Show timed points (earthquakes…) only from the hours before the app's time.
timeline-live = Live
timeline-now-action = Now
timeline-now-help = Back to the real time
timeline-day-back-help = One day back
timeline-hour-back-help = One hour back
timeline-hour-forward-help = One hour forward
timeline-day-forward-help = One day forward
timeline-time-of-day-help = Time of day (UTC)
timeline-speed-label = Speed
timeline-speed-minute = 1 min/s
timeline-speed-hour = 1 h/s
timeline-speed-six-hours = 6 h/s
timeline-speed-day = 1 day/s
timeline-window-label = Events
timeline-window-all = All
timeline-window-hour = Last hour
timeline-window-day = Last day
timeline-window-week = Last 7 days
timeline-window-month = Last 30 days
cmd-ui-tile-diagnostics = Tile diagnostics
cmd-ui-tile-diagnostics-help = Show the imagery tiles in view by state, the download queues per source and the download rates.
diagnostics-tiles-line = Tiles in view: { $total } (ready { $ready }, loading { $loading }, missing { $missing }, failed { $failed })
diagnostics-downloads-line = Downloads running: { $count }
diagnostics-source-line = { $source }: { $queued } queued, { $downloading } downloading
diagnostics-source-paused-line = { $source }: { $queued } queued, { $downloading } downloading, paused by the server
diagnostics-rates-line = View { $view } · ahead { $ahead } · borders { $borders }
cmd-ui-navigator-style = Navigator style
cmd-ui-navigator-style-help = Choose the navigator with the mini globe or the classic ring, move pad and zoom slider.
cmd-ui-ribbon = Collapse Ribbon
cmd-ui-ribbon-help = Collapse or expand the ribbon (Ctrl+F1).
cmd-ui-theme = Theme
cmd-ui-theme-help = Follow the system, or use the dark or light theme.
cmd-ui-fullscreen = Full Screen
cmd-ui-fullscreen-help = Enter or leave full screen.
cmd-ui-minimize = Minimize
cmd-ui-minimize-help = Minimize the window.
cmd-ui-command-search = Command Search
cmd-ui-command-search-help = Find and run any command by name.
cmd-ui-settings = Settings
cmd-ui-settings-help = Open the settings window.
cmd-settings-units = Units
cmd-settings-units-help = Show distances and altitudes in metric or imperial units, or follow the system.
cmd-settings-experience = Experience level
cmd-settings-experience-help = Basic shows the everyday tools; Advanced adds the Earth models, other worlds and the forms for web services and feature URLs.
settings-experience-label = Experience level
settings-experience-basic = Basic
settings-experience-advanced = Advanced
add-data-advanced-hint = Web services and feature URLs: switch to Advanced in Settings.
cmd-settings-coordinate-format = Coordinates
cmd-settings-coordinate-format-help = Choose how coordinates are shown.
cmd-settings-language = Language
cmd-settings-language-help = Choose the interface language.
cmd-ui-shortcuts = Shortcuts
cmd-ui-shortcuts-help = Show the mouse and keyboard controls.
cmd-help-documentation = Documentation
cmd-help-documentation-help = Open the GlobeCraft documentation.
cmd-help-report-issue = Report Issue
cmd-help-report-issue-help = Report a problem or suggest an idea.
cmd-ui-about = About
cmd-ui-about-help = About GlobeCraft and its data sources.
cmd-app-quit = Quit
cmd-app-quit-help = Close GlobeCraft.
cmd-info-view = Describe View
cmd-info-view-help = The camera position, visible layers, basemap, world and units.
cmd-info-places = List Places
cmd-info-places-help = Every placemark and folder in My Places, with coordinates.
cmd-info-ruler = Measurement
cmd-info-ruler-help = The ruler's points, total distance and last bearing.
assistant-suggestion-fly = Fly to Mount Everest
assistant-suggestion-layers = Show live aircraft and the rain radar
assistant-suggestion-measure = How far is it from Lisbon to Rio de Janeiro?
assistant-suggestion-placemark = Add placemarks for the Seven Wonders of the World
assistant-suggestion-moon = Take me to the Moon
assistant-note-length = The reply reached its length limit.
assistant-note-filter = The provider's content filter stopped the reply.
assistant-note-summarising = Summarising the earlier conversation to keep it within the model's context…
assistant-note-compacted = Compacted the earlier conversation.
assistant-note-empty = The model sent an empty reply; ask again, or try another model.
assistant-note-stopped = Stopped.
assistant-note-stopped-early = The reply stopped early ({ $reason }).
assistant-note-declined = The model declined this request.
assistant-note-declined-reason = The model declined this request: { $why }
assistant-note-incomplete = The reply stopped before completion; no actions were run.
assistant-note-gemini-stopped = Gemini stopped the reply ({ $reason }).
assistant-note-http-retry = The provider returned HTTP { $status }; trying again in { $seconds } s.
assistant-note-connection-retry = The connection failed; trying again in { $seconds } s.
assistant-note-step-limit = Stopped after { $steps } steps; say "continue" to go on.
assistant-note-call-limit = Stopped after { $calls } actions for one message; say "continue" to go on.
assistant-note-repeat-limit = Stopped: the model asked for the same action { $count } times in a row.
assistant-usage = { $input } tokens in, { $output } out
assistant-usage-cost = { $input } tokens in, { $output } out · about ${ $cost } (estimate)
assistant-usage-tooltip = Tokens this message used across all its requests. The cost is a rough estimate from approximate list prices per model family; your provider's bill is what counts.
assistant-model-tooltip = The model answering; click to change it
assistant-new-chat = New conversation
assistant-compact-tooltip = Summarise the conversation so far, so a long chat stays within the model's context
assistant-provider-section = AI provider
assistant-provider-label = Provider
assistant-model-label = Model
assistant-list-models = List
assistant-address-label = Address
assistant-key-label = API key
assistant-key-optional = optional
assistant-get-key = Get an API key
assistant-remember-key = Remember API keys on this computer
assistant-privacy-note = Your messages, the view (camera, layers, places) and command results are sent to the provider you choose. Keys are stored only if you ask, readable by your user account alone.
assistant-approve-title = Allow this action?
assistant-approve-reason = The assistant wants to run this, and { $why }:
assistant-approve-always = Allow such actions for this conversation
assistant-allow = Allow
assistant-decline = Decline
assistant-input-hint = Ask about the globe, or tell it what to do…
assistant-input-help = Enter to send, Shift+Enter for a new line
assistant-input-help-busy = Escape or Stop ends the request
assistant-stop = Stop
assistant-send = Send
assistant-intro = Ask a question about the places in front of you, or describe what to do. The assistant runs GlobeCraft commands for you: flying, layers, placemarks, measurements and more.
assistant-thinking = Thinking…
palette-hint = Search commands — e.g. terrain, fly, placemark
palette-empty = No command matches. Press Enter to ask the AI assistant.
palette-ask-assistant = Ask the AI assistant: { $question }
palette-layer-show = Show layer
palette-layer-hide = Hide layer
ribbon-file = File
ribbon-file-help = Open and save places, take screenshots, settings and quit
ribbon-expand-tooltip = Expand the ribbon (Ctrl+F1)
ribbon-collapse-tooltip = Collapse the ribbon (Ctrl+F1)
ribbon-scroll-left = Scroll left
ribbon-scroll-right = Scroll right
ribbon-group-navigate = Navigate
ribbon-group-places = Places
ribbon-group-measure = Measure
ribbon-group-capture = Capture
ribbon-group-assistant = AI
ribbon-group-basemap = Basemap
ribbon-group-pane = Pane
ribbon-group-worlds = Worlds
ribbon-group-panes = Panes
ribbon-group-window = Window
ribbon-group-kml = KML / KMZ
ribbon-group-add = Add
ribbon-group-manage = Manage
ribbon-group-preferences = Preferences
ribbon-group-display = Display
ribbon-group-commands = Commands
ribbon-imagery-date = Imagery date
ribbon-sources-status = Sources & Status
ribbon-basemap-satellite = Satellite
ribbon-basemap-wayback = Wayback
ribbon-basemap-streets = Streets
ribbon-basemap-topo = Topographic
ribbon-units-system = System
ribbon-units-metric = Metric
ribbon-units-imperial = Imperial
ribbon-assistant-provider = AI Provider
ribbon-assistant-provider-help = Choose the model the AI assistant uses and enter its key.
ribbon-layer-countries = Countries
ribbon-layer-states = States
ribbon-layer-labels = Place Names
ribbon-layer-municipal = Municipalities
ribbon-layer-terrain = 3D Terrain
ribbon-layer-grid = Grid
ribbon-layer-grid-labels = Grid Labels
ribbon-layer-atmosphere = Atmosphere
ribbon-layer-sunlight = Sunlight
ribbon-layer-night-lights = Night Lights
ribbon-layer-stars = Stars
ribbon-layer-milky-way = Milky Way
ribbon-layer-cinematic = Cinematic Sky
ribbon-layer-radar = Rain Radar
ribbon-layer-clouds = Clouds
ribbon-layer-flights = Aircraft
ribbon-layer-vessels = Ships
ribbon-layer-satellites = Satellites
ribbon-layer-roads = Roads
ribbon-layer-railways = Railways
ribbon-layer-ferries = Ferries
ribbon-layer-airports = Airports
ribbon-layer-poi = Places of interest
ribbon-layer-peaks = Peaks
ribbon-layer-waters = Rivers & waters
ribbon-layer-parks = Parks
ribbon-layer-traffic = Traffic
statusbar-tiles-ready = Tiles ready
assistant-settings-title = Assistant settings
pane-measure-title = Measure
pane-close-tooltip = Close the pane
pane-close-named-tooltip = Close { $name }
layers-earth-only = Layers apply to the Earth. Switch back to Earth in View › Worlds.
explore-results-empty = Search for a place in the box at the top of the window; its results appear here.
measure-total-label = Total distance
measure-points-label = { $count } points
measure-area-label = Area: { $area }
measure-profile-title = Elevation profile
measure-profile-loading = Loading the ground's heights…
measure-profile-stats = Lowest { $min } · highest { $max } · up { $ascent } · down { $descent }
measure-profile-hover = { $distance } · { $height }
measure-how-title = How to measure

settings-category-map-style = Map style

settings-map-style-line-country = Country borders

settings-map-style-line-state = State and province borders

settings-map-style-line-municipal = Municipal borders
settings-map-style-line-disputed = Disputed borders
settings-map-style-group-borders = Borders
settings-map-style-group-roads = Roads
settings-map-style-group-transport = Railways, ferries and airports
settings-map-style-group-water-parks = Rivers and parks
settings-map-style-group-time-zones = Time zones
settings-map-style-line-major-road = Motorways and main roads
settings-map-style-line-minor-road = Smaller roads and streets
settings-map-style-line-roadworks = Roadworks
settings-map-style-line-railway = Railways
settings-map-style-line-service-rail = Yards and sidings
settings-map-style-line-subway = Subways
settings-map-style-line-tram = Trams and light rail
settings-map-style-line-ferry = Ferries
settings-map-style-line-runway = Runways
settings-map-style-line-taxiway = Taxiways
settings-map-style-line-river = Rivers
settings-map-style-line-canal = Canals
settings-map-style-line-park = Parks and protected areas
settings-map-style-line-time-zone = Time zone lines

settings-map-style-line-help = Municipal borders on a state line use the state style, so the switch to them up close keeps one look.

settings-map-style-color-label = Color

settings-map-style-width-label = Width

settings-map-style-dash-label = Line

settings-map-style-dash-solid = Solid

settings-map-style-dash-dashed = Dashed

settings-map-style-dash-dotted = Dotted

settings-map-style-labels-title = Name size

settings-map-style-label-country = Countries

settings-map-style-label-state = States and provinces

settings-map-style-label-city = Cities and towns
settings-map-style-label-road = Street names
settings-map-style-label-time-zone = Time zone offsets

settings-map-style-reset-action = Reset map style

cmd-style-line = Map line style
cmd-style-line-help = Set the color, width and dash of a kind of map line: borders, roads, railways and other transport, rivers, park outlines or time zone lines.
cmd-style-label-size = Name size
cmd-style-label-size-help = Set the font size of country, state, city or street names, or of the time zones' offsets.
cmd-style-reset = Reset map style
cmd-style-reset-help = Restore the default border colors, widths, dashes and name sizes.

add-data-title = Add Data
add-data-tab-library = Library
add-data-tab-web = Web map
add-data-tab-features = Points and lines
add-data-category-ocean = Ocean
add-data-category-earth = Earth
add-data-category-hazards = Hazards
add-data-commercial-yes = commercial use allowed
add-data-commercial-no = non-commercial only
add-data-longevity-institutional = run by a public institution
add-data-longevity-goodwill = run on goodwill, no guarantee
add-data-add-action = Add
add-data-added-label = Added
add-data-test-action = Test
add-data-testing-status = Fetching a sample…
add-data-test-failed = It did not work: { $error }
add-data-web-xyz = Tiles (XYZ, WMTS)
add-data-web-wms = WMS
add-data-web-template-label = Tile address
add-data-web-service-label = Service address
add-data-web-tile-size-label = Tile size
add-data-web-max-zoom-label = Finest level
add-data-web-layer-label = Layer
add-data-web-no-layers = Load the service's layers first
add-data-web-load-layers = Load layers
add-data-web-default-name = Web map
add-data-web-test-ok = A tile arrived ({ $width } × { $height } pixels).
add-data-web-help = A tile address takes {"{"}z{"}"}, {"{"}x{"}"} and {"{"}y{"}"} ({"{"}-y{"}"} for TMS); {"{"}bbox{"}"} and {"{"}date{"}"} (yesterday) are filled in too. The layer is drawn over the imagery, under the borders.
add-data-size-by-label = Size by
add-data-size-by-hint = A property, such as mag or population (found if empty)
add-data-name-label = Name
add-data-attribution-label = Credit
add-data-attribution-hint = What the data's licence asks you to show
add-data-location-label = File or URL
add-data-location-hint = A path on this computer or https://…
add-data-browse-action = Browse…
add-data-file-filter = GeoJSON, GPX or CSV
add-data-format-label = Format
add-data-format-auto = From the file name
add-data-format-geojson = GeoJSON
add-data-format-gpx = GPX
add-data-format-csv = CSV with coordinates
add-data-format-unknown = Choose the format: the file name does not say it.
add-data-refresh-label = Reload every
add-data-refresh-help = For a live feed: how often it is read again. 0 reads it once.
add-data-color-label = Colour
add-data-features-test-ok = Read { $points } points and { $lines } lines or outlines.
add-data-kml-help = KML and KMZ open into My Places.
add-data-kml-action = Open KML…
add-data-added-status = Added “{ $name }” to your layers.
user-point-time = Time: { $time }
user-cluster-hover = { $count } points here
layers-user-legend-size = Sized by { $field }: { $low } to { $high }
layers-user-legend = { $points ->
        [one] 1 point
       *[other] { $points } points
    } · { $lines ->
        [one] 1 line or outline
       *[other] { $lines } lines or outlines
    }
layers-user-title = My data
layers-user-empty = Nothing added yet.
layers-user-add-action = Add Data…
layers-user-loading = Loading…
layers-user-failed = Not loading: { $error }
layers-user-opacity-label = Opacity
layers-user-up-help = Draw over the layer above
layers-user-down-help = Draw under the layer below
layers-user-zoom-help = Fly to the area it covers
layers-user-remove-help = Remove this layer
preset-gebco = Seafloor (GEBCO)
preset-gebco-help = Shaded relief of the ocean floor and the land, worldwide.
preset-emodnet = European seafloor depth (EMODnet)
preset-emodnet-help = Water depth of the European seas in colour, finer than GEBCO there.
preset-gibs = Yesterday from space (NASA GIBS)
preset-gibs-help = The whole Earth as the MODIS Terra satellite saw it yesterday, clouds included.
preset-forest-loss = Tree cover loss (Global Forest Watch)
preset-forest-loss-help = Forest lost since 2001, from Landsat (Hansen, University of Maryland).
preset-earthquakes = Earthquakes, past week (USGS)
preset-earthquakes-help = Earthquakes of magnitude 2.5 and up in the last seven days, read again every five minutes.
globe-whats-here-action = What's here?
here-title = What's here
here-copy-help = Copy
here-elevation-label = Ground height
here-loading = Loading…
here-country-label = Country
here-no-country = None (sea or unclaimed land)
here-state-label = State
here-municipality-label = Municipality
here-municipality-help = Named after the most important town inside the same municipal borders, usually the municipality's seat.
here-state-not-loaded-help = The state borders around this point are not loaded. They load around the view: look at the point from closer.
here-municipality-not-loaded-help = Municipal borders load only while the view is zoomed in close to the point.
here-admin-no-name-help = No name lies inside the same borders as the point in the borders loaded so far.
here-nearest-label = Nearest place
here-nearest-value = { $distance } { $direction } of { $name }
here-time-label = Time
here-time-value = { $solar } solar · { $utc } UTC · nautical zone { $zone }
here-time-help = Mean solar time follows the longitude; the legal time zone of the place can differ by an hour or more.
here-sun-label = Sun
here-sun-up = { $degrees }° above the horizon
here-sun-down = { $degrees }° below the horizon
