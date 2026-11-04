//! The Time zones layer (roadmap 5, `REF-05`): zone borders over the globe and the
//! local time under the cursor, at the app's time, in the status bar.
use super::App;
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};
use globecraft_core::camera::Basis;
use globecraft_core::layers::MapLayer;
use globecraft_data::timezones::TimeZones;
use std::sync::Arc;

const BORDER: Color32 = Color32::from_rgba_premultiplied(200, 150, 60, 200);
const BORDER_POINTS: f32 = 1.0;

/// What the borders were drawn for: the view, the zones and the map scale.
type BordersKey = (Basis, Rect, usize, u32);

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
        (scale, view): (f32, (f64, f64, f64)),
    ) {
        let key = (*basis, rect, Arc::as_ptr(zones) as usize, scale.to_bits());
        if self.built.as_ref().is_none_or(|(built, _)| *built != key) {
            self.built = Some((key, borders(basis, rect, zones, scale, view)));
        }
        if let Some((_, shapes)) = &self.built {
            painter.extend(shapes.iter().cloned());
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

/// Every zone's rings as lines on screen, cut where they leave the view or the
/// visible side of the globe. `view`: the camera's latitude, longitude and altitude.
fn borders(
    basis: &Basis,
    rect: Rect,
    zones: &TimeZones,
    scale: f32,
    view: (f64, f64, f64),
) -> Vec<Shape> {
    let clip = rect.expand(globecraft_render::overlays::SEGMENT_CLIP_MARGIN_POINTS);
    let stroke = Stroke::new(BORDER_POINTS * scale, BORDER);
    let mut shapes = Vec::new();
    let mut flush = |run: &mut Vec<Pos2>| {
        if run.len() >= 2 {
            shapes.push(Shape::line(std::mem::take(run), stroke));
        }
        run.clear();
    };
    for ring in zones
        .zones
        .iter()
        .filter(|zone| within_horizon(zone.bounds(), view))
        .flat_map(|zone| zone.polygons.iter().flatten())
    {
        let mut run: Vec<Pos2> = Vec::new();
        let mut previous = None;
        for &(lat, lon) in ring {
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
        let scale = self.settings.map_scale;
        let view = (self.cam.lat, self.cam.lon, self.cam.alt);
        self.time_zone_shapes
            .paint(painter, basis, rect, &zones, (scale, view));
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
            let start = std::time::Instant::now();
            let mut shapes = 0;
            for _ in 0..10 {
                shapes = borders(&basis, rect, &zones, 1.0, (48.0, 10.0, alt)).len();
            }
            println!(
                "TZ BORDERS alt {alt} km: {:.2} ms, {shapes} lines",
                start.elapsed().as_secs_f64() * 100.0
            );
        }
    }
}
