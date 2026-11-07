//! The Time zones layer (roadmap 5, `REF-05`): zone borders over the globe and the
//! local time under the cursor, at the app's time, in the status bar.
use super::App;
use eframe::egui::{self, Color32, Pos2, Rect, Shape};
use globecraft_core::camera::Basis;
use globecraft_core::layers::MapLayer;
use globecraft_data::settings::map_style::{LabelType, LineStyle, LineType};
use globecraft_data::timezones::TimeZones;
use std::sync::Arc;

const LABEL_HALO: Color32 = Color32::from_rgba_premultiplied(10, 12, 16, 220);
/// The row of whole-hour labels sits this far above the bottom of the view…
const ROW_INSET_POINTS: f32 = 70.0;
/// …or, when the globe does not reach the bottom, this share of the view's height
/// above its rim.
const ROW_ABOVE_RIM: f32 = 0.1;
const ROW_SEARCH_STEP_POINTS: f32 = 10.0;
/// The row stays north of 58° S: south of 60° S Antarctica's sectors keep the time
/// of their stations, not the meridians' whole hours, and carry their own labels.
const ROW_MAX_LAT_DEG: f64 = 58.0;

/// How the zones are drawn: the line in the user's map style (Settings → Map style),
/// the labels at its size in the line's colour, opaque, all at the map scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    line: LineStyle,
    label_points: f32,
    scale: f32,
}

impl Look {
    fn label_colour(&self) -> Color32 {
        let [r, g, b, _] = self.line.color.0;
        Color32::from_rgb(r, g, b)
    }
}

/// What the borders were drawn for: the view, the zones and their look.
type BordersKey = (Basis, Rect, usize, Look);

/// The borders drawn for one view.
#[derive(Default)]
pub(super) struct TimeZoneShapes {
    built: Option<(BordersKey, Vec<Shape>)>,
}

impl TimeZoneShapes {
    fn paint(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        zones: &Arc<TimeZones>,
        (look, view): (Look, (f64, f64, f64)),
    ) {
        let key = (*basis, rect, Arc::as_ptr(zones) as usize, look);
        if self.built.as_ref().is_none_or(|(built, _)| *built != key) {
            self.built = Some((key, borders(painter, basis, rect, zones, &look, view)));
        }
        if let Some((_, shapes)) = &self.built {
            // Labels are laid out again: kept galleys go stale when the font atlas
            // is rebuilt (garbled offsets).
            // (Collected first: laying out text inside `extend` would wait on the
            // painter's own lock.)
            let fresh: Vec<Shape> = shapes
                .iter()
                .map(|shape| super::live_layers::fresh_text(painter, shape))
                .collect();
            painter.extend(fresh);
        }
    }
}

/// Whether a zone's bounds reach within the horizon of a camera over `(lat, lon)` at
/// `altitude_km`: zones wholly past it are not projected at all.
fn within_horizon(bounds: [f64; 4], (lat, lon, altitude_km): (f64, f64, f64)) -> bool {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    // The horizon's angle from the point below the camera, with a margin for tilt.
    let reach = (EARTH_RADIUS_KM / (EARTH_RADIUS_KM + altitude_km))
        .acos()
        .to_degrees()
        * 1.5
        + 2.0;
    let [south, west, north, east] = bounds;
    if north < lat - reach || south > lat + reach {
        return false;
    }
    let polar = lat.abs() + reach >= 89.0;
    // Zones that span half the world (Russia, Fiji and Antarctica cross 180°) are
    // always drawn.
    if polar || reach >= 180.0 || east - west >= 180.0 {
        return true;
    }
    let lon_reach = reach / (lat.abs() + reach).min(89.0).to_radians().cos();
    let offset = |edge: f64| (edge - lon + 540.0).rem_euclid(360.0) - 180.0;
    // The zone's span of longitude against the camera's, either way round.
    let (west, east) = (offset(west), offset(east));
    if west <= east {
        east >= -lon_reach && west <= lon_reach
    } else {
        true
    }
}

/// A line with a point at least every `MAX_STEP_DEGREES`: the published lines keep
/// only their corners, and a long straight side (an ocean zone's meridian) drawn
/// between far-apart points would cut across the globe.
fn densified(line: &[(f64, f64)]) -> Vec<(f64, f64)> {
    const MAX_STEP_DEGREES: f64 = 1.0;
    let mut points = Vec::with_capacity(line.len());
    for pair in line.windows(2) {
        let ((lat_a, lon_a), (lat_b, lon_b)) = (pair[0], pair[1]);
        let steps = ((lat_b - lat_a).abs().max((lon_b - lon_a).abs()) / MAX_STEP_DEGREES)
            .ceil()
            .max(1.0) as usize;
        for i in 0..steps {
            let t = i as f64 / steps as f64;
            points.push((lat_a + (lat_b - lat_a) * t, lon_a + (lon_b - lon_a) * t));
        }
    }
    points.extend(line.last().copied());
    points
}

/// The bounds of a line: south, west, north, east.
fn line_bounds(line: &[(f64, f64)]) -> [f64; 4] {
    line.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[s, w, n, e], &(lat, lon)| [s.min(lat), w.min(lon), n.max(lat), e.max(lon)],
    )
}

/// A clock's label is not repeated closer than this to itself.
const SAME_LABEL_APART_POINTS: f32 = 260.0;

/// The label of a whole-hour zone: `0`, `+1`, `−1`, … and `±12` on the date line.
fn hour_label(hour: i32) -> String {
    match hour {
        0 => "0".into(),
        12 => "±12".into(),
        hour if hour > 0 => format!("+{hour}"),
        hour => format!("\u{2212}{}", -hour),
    }
}

/// Where the row of whole-hour labels runs: the latitude of the ground a little
/// above the bottom of the view, or of the lower part of the globe from far away.
fn row_latitude(basis: &Basis, rect: Rect, scale: f32) -> Option<f64> {
    let x = rect.center().x;
    let mut y = rect.bottom() - ROW_INSET_POINTS * scale;
    while y > rect.top() {
        if let Some((lat, _)) = basis.pick(rect, Pos2::new(x, y)) {
            // From far away the first ground is the globe's rim, where the
            // meridians crowd; a little higher they spread out.
            let higher = Pos2::new(x, y - ROW_ABOVE_RIM * rect.height());
            let lat = basis.pick(rect, higher).map_or(lat, |(lat, _)| lat);
            return Some(lat.clamp(-ROW_MAX_LAT_DEG, ROW_MAX_LAT_DEG));
        }
        y -= ROW_SEARCH_STEP_POINTS;
    }
    None
}

/// The lines where the standard time changes, on screen, cut where they leave the view or
/// the visible side of the globe, and the labels. `view`: the camera's
/// latitude, longitude and altitude.
fn borders(
    painter: &egui::Painter,
    basis: &Basis,
    rect: Rect,
    zones: &TimeZones,
    look: &Look,
    view: (f64, f64, f64),
) -> Vec<Shape> {
    let scale = look.scale;
    let clip = rect.expand(globecraft_render::overlays::SEGMENT_CLIP_MARGIN_POINTS);
    let mut shapes = Vec::new();
    let mut flush = |run: &mut Vec<Pos2>| {
        super::live_layers::styled_line(&mut shapes, run, &look.line, scale, 1.0);
        run.clear();
    };
    for line in zones
        .lines
        .iter()
        .filter(|line| within_horizon(line_bounds(line), view))
    {
        let mut run: Vec<Pos2> = Vec::new();
        let mut previous = None;
        for (lat, lon) in densified(line) {
            let current =
                globecraft_render::overlays::project_unhidden(basis, rect, None, None, lat, lon);
            match previous
                .zip(current)
                .and_then(|(a, b)| globecraft_render::overlays::clip_segment([a, b], clip))
            {
                Some([a, b]) if run.last() == Some(&a) => run.push(b),
                Some([a, b]) => {
                    flush(&mut run);
                    run = vec![a, b];
                }
                None => flush(&mut run),
            }
            previous = current;
        }
        flush(&mut run);
    }
    // The file's labels where they are (Antarctica's sectors, then the zones off the
    // whole hour, largest first), then the whole hours in a row across the lower part
    // of the view, as along the bottom of a time zone map. The file's come first: a
    // whole hour of the row nearby must not hide a sector's label of the same text.
    let font = super::fonts::map_font(painter.ctx(), look.label_points * scale);
    let colour = look.label_colour();
    let mut taken: Vec<Rect> = Vec::new();
    let mut placed: Vec<(Pos2, &str)> = Vec::new();
    let row = row_latitude(basis, rect, scale)
        .map(|lat| {
            (-11..=12)
                .map(|hour| {
                    (
                        lat,
                        f64::from(hour) * 15.0,
                        hour_label(hour).into_boxed_str(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (lat, lon, text) in zones.labels.iter().chain(&row) {
        let Some(at) =
            globecraft_render::overlays::project_unhidden(basis, rect, None, None, *lat, *lon)
                .map(|p| Pos2::new(p.x as f32, p.y as f32))
        else {
            continue;
        };
        if !rect.contains(at) {
            continue;
        }
        let galley = painter.layout_no_wrap(text.to_string(), font.clone(), colour);
        let bounds = Rect::from_center_size(at, galley.size()).expand(4.0 * scale);
        let repeated = placed.iter().any(|(other, label)| {
            *label == &**text && other.distance(at) < SAME_LABEL_APART_POINTS * scale
        });
        if repeated || taken.iter().any(|other| other.intersects(bounds)) {
            continue;
        }
        taken.push(bounds);
        placed.push((at, text));
        let corner = at - galley.size() / 2.0;
        // A thin outline: two copies offset by a point and a half read as doubled digits.
        super::antarctica::outlined(&mut shapes, painter, corner, galley, LABEL_HALO);
    }
    shapes
}

impl App {
    pub(super) fn update_time_zones(&mut self, ctx: &egui::Context, earth: bool) {
        let wake = ctx.clone();
        self.live_time_zones.update(
            &self.runtime,
            earth && self.layer(MapLayer::TimeZones),
            move || wake.request_repaint(),
        );
    }

    /// The zone borders, under the borders and names.
    pub(super) fn paint_time_zones(&mut self, painter: &egui::Painter, basis: &Basis, rect: Rect) {
        if !self.layer(MapLayer::TimeZones) {
            return;
        }
        let Some(zones) = self.live_time_zones.zones() else {
            return;
        };
        let style = &self.settings.map_style;
        let look = Look {
            line: *style.line(LineType::TimeZone),
            label_points: style.labels.get(LabelType::TimeZone),
            scale: self.settings.map_scale,
        };
        let view = (self.cam.lat, self.cam.lon, self.cam.alt);
        self.time_zone_shapes
            .paint(painter, basis, rect, &zones, (look, view));
    }

    /// The local time under the cursor at the app's time, as the status bar shows it:
    /// `15:15 CET (UTC+1)`.
    pub(super) fn cursor_local_time(&self) -> Option<String> {
        let (lat, lon) = self.cursor?;
        let zones = self.live_time_zones.zones()?;
        let local = zones.zone_at(lat, lon)?.local_time(self.scene_unix())?;
        Some(match local.abbreviation {
            Some(name) => format!("{} {name} ({})", local.time, local.offset),
            None => format!("{} ({})", local.time, local.offset),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_past_the_horizon_are_skipped() {
        let europe = [35.0, -10.0, 70.0, 40.0];
        let close_over_munich = (48.0, 11.0, 300.0);
        assert!(within_horizon(europe, close_over_munich));
        assert!(!within_horizon(
            [-45.0, 165.0, -34.0, 179.0],
            close_over_munich
        ));
        // A zone across 180° (Russia) spans every longitude.
        assert!(within_horizon(
            [41.0, -180.0, 78.0, 180.0],
            close_over_munich
        ));
        // From far out, the far side of the visible half still counts.
        assert!(within_horizon(
            [30.0, 100.0, 40.0, 110.0],
            (48.0, 11.0, 12_000.0)
        ));
    }

    /// How long the borders of the published zones take to project for one view
    /// (`GLOBECRAFT_TIMEZONES_FILE`). Manual.
    #[test]
    #[ignore = "manual: needs GLOBECRAFT_TIMEZONES_FILE"]
    fn border_projection_time() {
        let path = std::env::var("GLOBECRAFT_TIMEZONES_FILE").expect("GLOBECRAFT_TIMEZONES_FILE");
        let zones = TimeZones::from_gzip(&std::fs::read(path).unwrap()).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        for alt in [12_000.0, 2_000.0, 300.0] {
            let basis = globecraft_core::camera::Cam {
                lat: 48.0,
                lon: 10.0,
                heading: 0.0,
                alt,
                tilt: 0.0,
            }
            .basis(1.6);
            let ctx = egui::Context::default();
            let raw = egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            };
            let start = std::time::Instant::now();
            let mut shapes = 0;
            let _ = crate::app::test_ui::run(&ctx, raw, |ui| {
                for _ in 0..10 {
                    let look = Look {
                        line: LineType::TimeZone.default_style(),
                        label_points: 12.0,
                        scale: 1.0,
                    };
                    shapes =
                        borders(ui.painter(), &basis, rect, &zones, &look, (48.0, 10.0, alt)).len();
                }
            });
            println!(
                "TZ BORDERS alt {alt} km: {:.2} ms, {shapes} lines",
                start.elapsed().as_secs_f64() * 100.0
            );
        }
    }
}
