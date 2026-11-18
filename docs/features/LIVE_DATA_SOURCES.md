# GlobeCraft — live imagery credits and use

**Satellite clouds:** Matt Eason, Live Cloud Maps (CC0-1.0). Latest source:
https://clouds.matteason.co.uk/images/8192x4096/clouds-alpha.png
Repository: https://github.com/matteason/live-cloud-maps

The clouds are drawn as a layer 10 km above the ground (`globecraft_render::clouds`):
over Everest and under airliners at cruise. They come from the full 8192 × 4096 map
(with 4096- and 2048-wide levels for views from far out) and are lit like the globe:
white by day, faint blue-grey at night. From above, the app draws them over the frame
and they fade out between 22 and 10.5 km; from below (under 10 km), the renderer draws
them into the sky, where relief hides them. Aircraft on the far side of the layer
fade behind thick cloud. The aurora is a layer just above the clouds (10.5 km): over
them from above, behind them (with the stars, behind the frame) from below.

With Sunlight on, the same service's water map
(https://clouds.matteason.co.uk/images/4096x2048/specular.jpg: the sea bright, land
and cloud dark) gives the sun's glint on the water, on the CPU and the GPU alike
(`globecraft_render::sun::glint`).

**Required source attribution:** Contains modified EUMETSAT data

The provider publishes eight updates per day (one every three hours), depending on upstream satellite data availability.

**Precipitation radar:** RainViewer public Weather Maps API, https://www.rainviewer.com/
API docs: https://www.rainviewer.com/api/weather-maps-api.html
The free API, as of 2026, offers radar tiles (up to zoom 7) in color scheme 2, Universal Blue. GlobeCraft remaps colors to a green/yellow/red rain-intensity visualization. It is **not** satellite infrared temperature. Radar coverage is not global and may be unavailable in some regions. Last published observations can be delayed.

API free use is limited to personal, educational, and small-community uses. For commercial distribution arrange licensing with the provider.

**Flight markers:** observed aircraft positions from OpenSky Network
(https://opensky-network.org/) and ADSB.lol (https://www.adsb.lol/).
OpenSky supplies global samples and ADSB.lol supplies nearby aircraft. Coverage
and timestamps depend on the providers; these are not simulated routes or
complete, instantaneous global tracking. Marker extrapolation is limited to
25 seconds. The layer displays source status and attribution.

Implementation notes: HTTP calls happen asynchronously; the world imagery remains rendered even if sources are offline; app displays status and image credits; clouds refresh every three hours and radar every ten minutes.
