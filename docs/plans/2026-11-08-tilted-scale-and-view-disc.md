# Scale bar and navigator view disc for tilted views (roadmap 6.11)

**Status:** implemented and verified 2026-11-08; reviewed, checked again (full gate: 611 passed; `interface_screenshots`) and committed the same day.

## Why this item

The scale bar and the navigator's mini globe were built for a straight-down
camera. The bar converted altitude to metres per point at the centre of the
view, and the mini globe drew the view as a circle around the camera's ground
point. Both are right only at zero tilt. In a tilted view the ground under the
bar is closer or farther than the centre, and the view reaches forward to the
horizon; the bottom edge can even be sky. Roadmap 6.11 listed this as the
remaining interface part of the tilt work.

## What shipped

### Scale bar

`crates/globecraft-render/src/scale_bar.rs`:

- `ScaleBar` gained `bottom_inset_points`. `globe.rs` draws the bar and its
  label at that inset instead of the fixed `SCALE_BOTTOM_INSET_POINTS`.
- A tilted sphere view now measures the actual ground under the bar. The
  `visible_scale_ground` helper scans upward from the usual bottom inset in
  4-point steps until both ends of a 120-point line hit the Earth; that inset
  is where the bar is drawn. A view that looks past the horizon at the bottom
  therefore lifts the bar onto the ground, and a view with no ground under the
  left edge draws no bar.
- The maximum ground distance over that line selects a round distance in the
  current unit system. A 24-step bisection then finds the width whose two ends
  span exactly that distance.
- A straight-down perfect sphere keeps the original altitude-based bar bit for
  bit. The ellipsoid path is unchanged: it already measured the surface under
  its endpoints.

### Navigator mini globe

`apps/globecraft/src/app/ui/navigation/mini_globe.rs`:

- `MiniGlobe::view_outline` samples the four edges of the globe area (12 points
  per edge). Each sample is the ground point under that screen ray
  (`Basis::pick`), or, when the ray misses the Earth, the tangent point on the
  camera's horizon circle at the ray's azimuth (`horizon_point`). The points
  are projected onto the mini globe and drawn as one closed polygon.
- The horizon helper uses the ray's horizontal component, so it also works for
  rays above the camera's horizon whose closest approach lies behind the
  camera. This keeps the boundary on the horizon circle instead of jumping to
  the mini globe's limb.
- `navigation.rs` switches at a named 1° threshold (`shows_view_footprint`).
  Straight-down and near-straight-down views keep the original cap circle;
  tilted views draw the footprint with the same accent fill and stroke.

## Verification

- `crates/globecraft-render/src/scale_bar.rs`:
  `tilted_sphere_bar_measures_the_ground_under_its_ends` checks the drawn
  width against the actual surface distance for 900 km at 55° and 60 km at
  75°, and that the result differs from the old altitude-based policy.
  `a_top_down_sphere_keeps_the_altitude_based_bar` checks the unchanged
  straight-down result.
- `apps/globecraft/src/app/ui/navigation/tests.rs`:
  `a_straight_down_view_keeps_the_cap_disc` covers the 1° policy;
  `a_tilted_view_shows_a_forward_footprint` checks the far edge is more than
  twice the top-down cap and the near edge stays close behind the centre;
  `a_tilted_footprint_follows_the_horizon_not_the_sky` checks that a top edge
  looking at the sky still lands on the camera's horizon, well inside the mini
  globe.
- `interface_screenshots` (inspected by eye): `06-tilted-dark` (900 km, 55°)
  and `07-tilted-close-dark` (60 km, 75°) show the forward wedge and the
  ground-measured bar; `01-home-light` keeps the top-down circle;
  `01-tilt-arc-light` shows the footprint at a global tilted view. At that
  extreme view the left edge is sky, so the bar is hidden rather than drawn
  over space.
- Full gate: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked` (612 passed, 62 ignored) and
  `cargo build --workspace --locked`.

## Limits and critique

- The bar only scans upward along its left edge. If that whole edge is sky, as
  in a very high altitude with a strong tilt, there is no bar; it does not move
  sideways to find ground. This is deliberate: a scale bar over space would be
  wrong.
- The scan tests a 120-point line even when the final bar is shorter, so it can
  lift the bar farther than strictly necessary. The result is conservative and
  keeps the bar on the ground.
- The footprint is clipped by the mini globe's visible hemisphere. At extreme
  high-altitude tilts only the near part of a very large footprint is drawn;
  the polygon closes along the limb rather than showing the far side.
- The 1° threshold keeps the original circle for tiny tilts. The sampled
  polygon is convex in practice, but f32 pixel rounding makes a strict
  convexity assertion brittle, so the tests check its extent and horizon
  behaviour instead.
- GPU parity remains open in the roadmap row; this change is the CPU/interface
  path.
