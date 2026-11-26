# Celestial background v1

Layers → Celestial background and Settings → Globe toggle a NASA-source Milky Way
backdrop behind Earth. It is disabled by default; enable it explicitly to use the backdrop. Loading or decoding failure
retains the previous procedural starfield with an English diagnostic/retry in
Settings. Turning it off restores the original rendering path and fixtures.
Earth imagery, shading, occlusion and tile requests stay unchanged.

The bundled 4096×2048 asset is the **celestial diffuse Milky Way** from
[NASA Deep Star Maps 2020, SVS 4851](https://svs.gsfc.nasa.gov/4851/), without its
bright Hipparcos/Tycho foreground. No additional procedural stars are painted over
it. This slice is directional imagery, not a selectable star catalog, deep-sky
search, HiPS survey or a physically navigable universe.

The Stars layer draws the 9,096 stars of the Yale Bright Star Catalogue (to about
magnitude 6.5, `crates/globecraft-render/assets/stars`) in the same celestial frame,
sized and coloured by magnitude and spectral class. Until 2026-11-18 the stars were
made up and fixed to the Earth. On screen the app's sky layer draws them behind the
frame at full resolution; screenshots paint them behind each strip
(`stars::paint_behind`).

`assets/sky/manifest.json` pins source and derivative SHA-256 values, release,
credit, map convention, usage assessment and offline preparation. The 2.5 MB JPEG
is a fixed-exposure Reinhard/sRGB **SDR derivative**, not HDR-equivalent fidelity.
The original EXR stays upstream; `scripts/prepare_sky.py` verifies it and prepares
the asset offline with FFmpeg/Python. No runtime download or EXR parser is needed.
The NASA/Gaia credit is visible on the globe and in Settings; a submitted screenshot
captures the displayed frame's credit in PNG Source metadata without copying the
shared framebuffer or drawing UI over the clean image.

Core converts Earth-fixed rays to ICRF/J2000 directions with sofars 0.6.1's IAU
2006/2000A bias/precession/nutation/rotation model. TT uses its UTC leap table;
UT1 is approximated by UTC and polar motion is zero. This is suitable for a
4K visual backdrop, not precision astrometry. Current wall-clock time is sampled
in minute buckets; a time simulator is not introduced. Opposite Earth longitudes
and changing time affect the frame transform rather than arbitrarily rotating a
camera-local texture. RA 0h is centered, RA increases leftward, north is up;
bilinear longitude wrapping and pole clamping prevent the usual mirror/seam bugs.
SOFA/sofars notices are included and readable in Settings → Globe.

Realistic mode fades faint sky near the daytime surface between solar altitudes
−18° and −6°, returning to full visibility by 100 km altitude. **Cinematic sky**
explicitly bypasses this attenuation. This is a simple visibility policy, not a
new atmospheric scattering model. The air (`atmosphere`, roadmap 8.3) composites in
front: it dims the sky behind it and puts the stars out where its own glow is bright.

Data decodes one optional image on its bounded CPU service and wakes the UI on
success/failure. Encoded input is limited to 8 MiB; decoded dimensions are fixed
and decoder allocation is limited to 64 MiB. One shared RGB image retains 24 MiB;
transient decoder allocations and compressed binary bytes are additional. Sky
assets do not enter imagery MBTiles or its 128 MiB disk target.

Render samples on CPU; GPU rendering uses the same exact CPU sky
repair after readback. No GPU sky texture/upload or device-specific compression
is required. Sky/cinematic/clock keys reject stale presentation results. CPU
fallback/device-loss behavior is retained. This is not a claim that every GPU or
packaged platform has been physically tested.

Tests cover the SOFA orientation reference, RA handedness, seams/poles, time
rotation, daylight/cinematic visibility, bounded async asset decode/wake,
preference persistence, screenshot credit snapshots, original fixtures and
unchanged Earth pixels/missing-tile priorities. The explicit native-GPU parity
check also includes the real sky when enabled. See CONTRIBUTING.md for matched
camera, toggle and platform visual checks. Higher-resolution assets, HDR/mip
quality profiles, catalogs, constellation guides and Sky/Space Explorer are later
slices from the planning pack.
