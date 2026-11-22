# Original raster fixtures

These four 64 × 48 RGBA frames were captured by calling `render_row` in the
unmodified `src/main.rs` at upstream commit `ae3b8f19e1942835403b4ff5bae435ea3795737b`.
They were generated on macOS ARM64 with Rust 1.99.0 before replacing the renderer.
Each `.rgba` file is raw row-major RGBA8, 12,288 bytes, with no image header.

The camera is latitude -15°, longitude -50°, heading 25°, altitude 14,000 km.
The provider is Satellite, release 0, with a 10° grid step where enabled.

| File | Grid | Atmosphere | Sun | Cache |
| --- | --- | --- | --- | --- |
| `plain.rgba` | off | off | none | empty |
| `atmosphere.rgba` | off | on | none | empty |
| `grid-sun.rgba` | on | on | latitude 20°, longitude 45° | empty |
| `loaded.rgba` | on | on | latitude 20°, longitude 45° | one level-0 tile |

`tile.png` is the loaded 2 × 2 tile: red/green on its first row and blue/white on
its second, all fully opaque. The original renderer sampled those same pixels.
No tile downloads or user caches were involved.

The renderer test compares every output channel, permitting one quantization
step for floating-point differences between platforms. Missing tile requests
must still be reported. These fixtures are regression evidence, not generated
build output. Do not overwrite them to accommodate an unexplained failure.
For an intentional rendering change, review the image difference and update
fixtures separately with the source commit, camera/configuration and rationale.

## The air (2026-11-15)

`atmosphere.rgba`, `grid-sun.rgba`, `loaded.rgba` and every `batches/*.rgba` were
recorded again when the physically based air (`src/atmosphere.rs`, roadmap 8.3)
replaced the blue rim and the halo: every pixel with the atmosphere on changed, as
intended (a thin limb that follows the sun, haze toward the limb, orange sunlight at
the terminator). Recorded on Linux x86-64 with `GLOBECRAFT_RECORD_FIXTURES=1 cargo
test -p globecraft-render raster_matches_regression_fixtures` and `… ray_batches`,
cameras and configurations unchanged; the pictures were compared before and after by
eye. `plain.rgba` (atmosphere off) and the `.misses` files are the originals.

## Ray-batching boundary fixtures

The additional `batches/` snapshots were captured from GlobeCraft commit
`c31bfa8` in an isolated temporary source checkout before enabling wide ray
batching. The capture called the unchanged production `render_pixels`; it did
not copy the pixel algorithm or generate expectations from the new SIMD code.
Original upstream fixtures above remain unchanged. Captured on macOS ARM64,
Rust 1.99.0, optimized dev profile without debug information/incremental output.

All cases use grid/atmosphere/sunlight, a 10° grid, sun at (20°, 45°), release 7,
and the existing 2×2 `tile.png` available only at level 0 for the chosen provider.
Everything else is absent. No user caches, services or requests are involved.

| Name | Size | Provider | Latitude | Longitude | Heading | Altitude km |
| --- | --- | --- | --- | --- | --- | --- |
| odd-satellite | 65×49 | Satellite | -15 | -50 | 25 | 14000 |
| polar-streets | 67×51 | Streets | 89.9 | 179.9 | -130 | 1000 |
| dateline-topo | 63×47 | Topo | -40 | 179.9 | 70 | 500 |
| narrow-wayback | 3×9 | Wayback | 0 | -180 | 180 | 30000 |
| one-pixel-column | 1×7 | Satellite | -15 | -50 | 25 | 14000 |
| low-altitude | 7×5 | Satellite | 89.9 | -179.9 | -20 | 0.15 |

Each `.rgba` is row-major RGBA8 (`width × height × 4` bytes), with the same
one-channel quantization tolerance as the original fixtures. Each `.misses` is
a list sorted by packed tile key: little-endian `u64` key followed by `f32`
nearest-pixel squared distance, 12 bytes per entry. Missing keys/priorities are
compared exactly, including short row and eight-row batch tails. Do not overwrite
these snapshots to accommodate a batching regression.

`polar-streets.misses` and `dateline-topo.misses` were regenerated when text-map
providers moved from a hard `ceil(lod)` level switch to `floor(lod)` plus a short
crossfade into the finer level. The RGBA snapshots are unchanged; only which
level is sampled during the last part of a zoom band, and therefore the request
priorities, changed. Satellite cases were untouched.

`odd-satellite.misses` was regenerated (same keys, new nearest-pixel squared
distances) when the imagery LOD moved from rounding to the nearest level with a
whole-band imagery crossfade to a one-level-oversampling base with an
end-of-band crossfade (`LOD_SHARPNESS_BIAS` 0.5 → 1.0, `IMAGERY_ZOOM_FADE_START`
0.7): all pixels now base-sample the z1 chain instead of only blend-gated ones,
so each missing key's nearest sampled pixel changed. At the other extreme
cameras the base floor was unchanged (`floor(zf)` fractional part ≥ 0.5), so
`low-altitude.misses` and `one-pixel-column.misses` (which now has no blend
sample and therefore empty misses) and both RGBA snapshots are unchanged.

## Minification area filter (2026-10-21, uncommitted working tree)

`Tile::sample` gained an optional box area-average (`Tile::sample(u, v, stride)`,
stride 1 keeps the exact legacy bilinear path) so tiles finer than the raster —
the one-level-oversampling base band, and 0.4-scale motion rasters using the
same display-referenced band — stop aliasing into a low-bitrate look. The
renderer requests the stride from the per-pixel level of detail only past
`TILE_MINIFICATION_FILTER_START` = 1.25 texels/pixel; coarser fallback levels
magnify and keep bilinear. Generated for the four original frames and all six
batch snapshots from the tree containing this change (same cameras and fake
level-0 tile as documented above). Key sets are unchanged everywhere because
the filter cannot affect which levels a pixel walks; only the sampled color
changed, and only where a base-or-better level actually sampled a loaded tile:
`loaded.rgba`, `odd-satellite.rgba`, `polar-streets.rgba` (~29% of pixels, the
pole camera whose base is the single level-0 tile), and the small
`one-pixel-column.rgba` / `narrow-wayback.rgba` edges (`dateline-topo.rgba`,
`low-altitude.*`, and the empty-tile frames are byte-identical). The tracking
fixture `.misses` files on disk here remain the state after the LOD-policy regen
above; regenerating under the area filter produced byte-identical misses for
every case, verifying request behavior is unchanged.

## Stars leave the frame (2026-11-18)

The made-up stars (one per 1/900-radian cell, fixed to the Earth) were removed from
the renderer: the catalogue stars are drawn behind the frame instead
(`stars::paint_behind`, the app's sky layer), and `RenderCfg::stars` became the
sky's moment. The batch references were rendered with the old stars on; only
`odd-satellite.rgba` showed any, and exactly its 2 star pixels changed to space
when regenerated (`GLOBECRAFT_RECORD_FIXTURES=1 cargo test -p globecraft-render
ray_batches`). Every other fixture is byte-identical.

## Sun and sky light apart, multiple scattering, twilight adaptation (2026-11-22)

Daylight now comes in two parts (`pixel::Daylight`): the sun's direct light, coloured
by the air it crossed, and the sky's, cool; relief and shadows hide only the first,
and the scene's exposure follows the eye's adaptation into the blue hour. The air's
tables gained Hillaire's multiple-scattering term, and inside the air its light is
adapted to twilight (`atmosphere::adaptation`). Every fixture with the sun or the
atmosphere changed (`GLOBECRAFT_RECORD_FIXTURES=1 cargo test -p globecraft-render --lib
-- raster_matches ray_batches`); checked against Bing Maps' sunrise recordings with
the `bing_sun_comparison` probe.

## Daylight as two rising curves (2026-11-22, later)

The fixed split of daylight (65 % sun, 35 % sky) made shaded ground darker just
before sunset than just after, and lit ground dimmer at sunrise than in the blue hour
(a dark band, then a light one, past the terminator; Julien's screenshot and
recording). Lit and shaded ground now each follow a brightness curve that only rises
with the sun (`pixel::Daylight`, test `lit_and_shaded_ground_brighten_with_the_sun`),
shade at 65 % of light by day, the sun's colour no longer squared, a slope's gain at
most 1.5, and the blue-hour exposure only with the camera in the air. Every fixture
with the sun changed.
