//! Cache styled AIS/orbital meshes without changing polygon/label ordering.
use eframe::egui::{self, Align2, Color32, Pos2, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::{satellites::Satellite, vessels::Vessel};
use globecraft_render::live_layers::{LiveGeometry, ProjectedSatellite, ProjectedVessel};
use std::sync::Arc;

const VESSEL_LABEL_ALTITUDE_KM: f64 = 75.0;
const VESSEL_LABEL_LIMIT: usize = 75;
const SATELLITE_LABEL_ALTITUDE_KM: f64 = 900.0;
const SATELLITE_LABEL_LIMIT: usize = 60;
const LABEL_FONT_POINTS: f32 = 10.0;
const LABEL_OFFSET_POINTS: f32 = 8.0;
const SATELLITE_ICON_POINTS: f32 = 5.0;
struct Cached<T> {
    geometry: Arc<Vec<T>>,
    shapes: Vec<Shape>,
}
#[derive(Default)]
pub(in crate::app) struct WorldShapes {
    vessels: Option<Cached<ProjectedVessel>>,
    satellites: Option<Cached<ProjectedSatellite>>,
}
impl WorldShapes {
    pub fn vessels(&mut self, painter: &egui::Painter, geometry: &LiveGeometry) -> &[Shape] {
        let stale = self
            .vessels
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(&old.geometry, &geometry.vessels));
        if stale {
            let shapes = build_vessels(painter, geometry);
            self.vessels = Some(Cached {
                geometry: geometry.vessels.clone(),
                shapes: super::super::live_geometry::mesh_shapes(painter, shapes),
            });
        }
        self.vessels.as_ref().map_or(&[], |cached| &cached.shapes)
    }
    pub fn satellites(&mut self, painter: &egui::Painter, geometry: &LiveGeometry) -> &[Shape] {
        let stale = self
            .satellites
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(&old.geometry, &geometry.satellites));
        if stale {
            let shapes = build_satellites(painter, geometry);
            self.satellites = Some(Cached {
                geometry: geometry.satellites.clone(),
                shapes: super::super::live_geometry::mesh_shapes(painter, shapes),
            });
        }
        self.satellites
            .as_ref()
            .map_or(&[], |cached| &cached.shapes)
    }
}
fn label(painter: &egui::Painter, at: Pos2, text: &str, scale: f32, color: Color32) -> Shape {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * scale),
        color,
    );
    let position = Align2::LEFT_CENTER
        .anchor_size(
            at + Vec2::new(LABEL_OFFSET_POINTS * scale, 0.0),
            galley.size(),
        )
        .min;
    Shape::galley(position, galley, color)
}
fn build_vessels(painter: &egui::Painter, geometry: &LiveGeometry) -> Vec<Shape> {
    let Some(vessels) = &geometry.input.vessels else {
        return Vec::new();
    };
    let scale = geometry.input.view.map_scale;
    let mut shapes = Vec::with_capacity(geometry.vessels.len() + VESSEL_LABEL_LIMIT);
    for (index, projected) in geometry.vessels.iter().enumerate() {
        let vessel = &vessels[projected.source_index];
        shapes.push(Shape::convex_polygon(
            projected.icon.to_vec(),
            Color32::from_rgb(52, 224, 229).gamma_multiply(projected.opacity),
            Stroke::new(
                1.0 * scale,
                Color32::from_rgb(5, 60, 65).gamma_multiply(projected.opacity),
            ),
        ));
        if geometry.input.view.altitude_km < VESSEL_LABEL_ALTITUDE_KM && index < VESSEL_LABEL_LIMIT
        {
            let text = if vessel.name.is_empty() {
                &vessel.mmsi
            } else {
                &vessel.name
            };
            shapes.push(label(
                painter,
                projected.position,
                text,
                scale,
                Color32::from_rgb(120, 245, 245),
            ));
        }
    }
    shapes
}
/// AIS ship-and-cargo groups the tooltip names.
fn vessel_class(code: Option<u16>) -> &'static str {
    match code {
        Some(30) => "fishing",
        Some(31 | 32 | 52) => "tug",
        Some(35 | 55) => "military",
        Some(36 | 37) => "pleasure",
        Some(40..=49) => "high_speed",
        Some(50 | 51 | 53 | 54 | 58 | 59) => "service",
        Some(60..=69) => "passenger",
        Some(70..=79) => "cargo",
        Some(80..=89) => "tanker",
        Some(90..=99) => "other",
        _ => "unknown",
    }
}

pub(super) fn vessel_tooltip(lang: Lang, ship: &Vessel) -> String {
    let name = if ship.name.is_empty() {
        lang.text("vessels-unnamed")
    } else {
        &ship.name
    };
    let destination = ship.destination.as_deref().unwrap_or("—");
    let dimensions = match (ship.length_m, ship.beam_m) {
        (Some(length), Some(beam)) => format!("{length:.0} × {beam:.0} m"),
        (Some(length), None) => format!("{length:.0} m"),
        (None, Some(beam)) => format!("{beam:.0} m"),
        (None, None) => "—".to_owned(),
    };
    let draught = ship
        .draught_m
        .map_or_else(|| "—".to_owned(), |draught| format!("{draught:.1} m"));
    lang.format(
        "vessels-hover-tooltip",
        &[
            ("name", name.into()),
            ("mmsi", ship.mmsi.as_str().into()),
            ("ship_type", vessel_class(ship.ship_type).into()),
            ("destination", destination.into()),
            ("dimensions", dimensions.into()),
            ("draught", draught.into()),
            ("latitude", format!("{:.3}", ship.lat).into()),
            ("longitude", format!("{:.3}", ship.lon).into()),
            (
                "course",
                globecraft_core::compass::heading_text(ship.course, lang).into(),
            ),
            (
                "speed",
                ship.speed_knots
                    .map_or_else(|| "?".to_owned(), |speed| format!("{speed:.1}"))
                    .into(),
            ),
            (
                "minutes",
                format!(
                    "{:.0}",
                    ((globecraft_core::math::now_unix() - ship.observed_unix).max(0.0) / 60.0)
                        .round()
                )
                .into(),
            ),
        ],
    )
}

pub(super) fn satellite_tooltip(lang: Lang, satellite: &Satellite) -> String {
    let period = satellite
        .orbit
        .period_minutes()
        .map_or_else(|| "—".to_owned(), |minutes| format!("{minutes:.1} min"));
    let altitude = satellite
        .orbit
        .altitude_km()
        .map_or_else(|| "—".to_owned(), |km| format!("{km:.0} km"));
    let inclination = format!("{:.1}°", satellite.orbit.inclination_deg);
    lang.format(
        "satellites-hover-tooltip",
        &[
            ("name", satellite.name.as_str().into()),
            ("norad", satellite.norad.as_str().into()),
            ("period", period.into()),
            ("altitude", altitude.into()),
            ("inclination", inclination.into()),
        ],
    )
}

fn build_satellites(painter: &egui::Painter, geometry: &LiveGeometry) -> Vec<Shape> {
    let Some(satellites) = &geometry.input.satellites else {
        return Vec::new();
    };
    let scale = geometry.input.view.map_scale;
    let s = SATELLITE_ICON_POINTS * scale;
    let mut shapes = Vec::with_capacity(geometry.satellites.len() + SATELLITE_LABEL_LIMIT);
    for (index, projected) in geometry.satellites.iter().enumerate() {
        let at = projected.position;
        shapes.push(Shape::convex_polygon(
            vec![
                at + Vec2::new(0.0, -s),
                at + Vec2::new(s, 0.0),
                at + Vec2::new(0.0, s),
                at + Vec2::new(-s, 0.0),
            ],
            Color32::from_rgb(207, 159, 255),
            Stroke::new(scale, Color32::from_rgb(62, 22, 91)),
        ));
        if geometry.input.view.altitude_km < SATELLITE_LABEL_ALTITUDE_KM
            && index < SATELLITE_LABEL_LIMIT
        {
            shapes.push(label(
                painter,
                at,
                &satellites[projected.source_index].name,
                scale,
                Color32::from_rgb(219, 178, 252),
            ));
        }
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;
    use globecraft_core::camera::Cam;
    use globecraft_core::orbit::OrbitalElements;
    use globecraft_render::live_layers::{LiveInput, LivePreparer, LiveView};

    fn ship() -> Vessel {
        Vessel {
            mmsi: "123456789".into(),
            name: "Ship".into(),
            lat: 0.0,
            lon: 0.0,
            course: 45.0,
            speed_knots: None,
            ship_type: Some(70),
            destination: Some("RIO".into()),
            draught_m: Some(8.4),
            length_m: Some(200.0),
            beam_m: Some(32.0),
            observed_unix: 1_700_000_000.0,
        }
    }

    #[test]
    fn vessel_tooltip_surfaces_type_destination_and_dimensions() {
        let text = vessel_tooltip(Lang::En, &ship());
        assert!(text.contains("Cargo ship"), "{text}");
        assert!(text.contains("Destination: RIO"), "{text}");
        assert!(text.contains("200 × 32 m"), "{text}");
        assert!(text.contains("Draught: 8.4 m"), "{text}");
        assert!(text.contains("MMSI 123456789"), "{text}");
        let unnamed = Vessel {
            name: String::new(),
            destination: None,
            length_m: None,
            beam_m: None,
            draught_m: None,
            ship_type: None,
            ..ship()
        };
        assert!(vessel_tooltip(Lang::En, &unnamed).contains("Unidentified vessel"));
        assert!(vessel_tooltip(Lang::PtBr, &unnamed).contains("Navio não identificado"));
        assert!(vessel_tooltip(Lang::En, &unnamed).contains("Vessel type not reported"));
    }

    #[test]
    fn satellite_tooltip_reports_period_altitude_and_inclination() {
        let satellite = Satellite {
            name: "ISS".into(),
            norad: "25544".into(),
            orbit: OrbitalElements {
                inclination_deg: 51.6,
                raan_deg: 0.0,
                eccentricity: 0.001,
                argument_perigee_deg: 0.0,
                mean_anomaly_deg: 0.0,
                mean_motion_rev_per_day: 15.0,
                epoch_unix_secs: 1_700_000_000.0,
            },
        };
        let text = satellite_tooltip(Lang::En, &satellite);
        assert!(text.contains("Period 96.0 min"), "{text}");
        assert!(text.contains("Altitude 574 km"), "{text}");
        assert!(text.contains("Inclination 51.6°"), "{text}");
    }

    #[test]
    fn vessel_label_order_limits_and_cached_meshes_survive_unchanged_frames() {
        let ctx = egui::Context::default();
        let ship = Vessel {
            mmsi: "123456789".into(),
            name: "Ship".into(),
            lat: 0.0,
            lon: 0.0,
            course: 45.0,
            speed_knots: None,
            ship_type: None,
            destination: None,
            draught_m: None,
            length_m: None,
            beam_m: None,
            observed_unix: 1_700_000_000.0,
        };
        let input = LiveInput {
            view: LiveView {
                basis: Cam {
                    lat: 0.0,
                    lon: 0.0,
                    heading: 0.0,
                    alt: 10.0,
                    tilt: 0.0,
                }
                .basis(1.0),
                rect: egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0)),
                altitude_km: 10.0,
                map_scale: 1.0,
                pixels_per_point: 1.0,
                traffic_width_px: 16,
                traffic_height_px: 16,
                ground: None,
            },
            roads: None,
            vessels: Some(vec![ship; 100].into()),
            satellites: None,
            traffic: None,
            transport: None,
            transport_layers: Default::default(),
            poi: None,
            poi_layers: Default::default(),
            unix_secs: 1_700_000_000.0,
        };
        let geometry = LivePreparer::default().prepare(input);
        crate::app::test_ui::run(&ctx, egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let raw = build_vessels(painter, &geometry);
            assert_eq!(raw.len(), 100 + VESSEL_LABEL_LIMIT);
            assert!(matches!(&raw[0], Shape::Path(_)));
            assert!(matches!(&raw[1], Shape::Text(_)));
            assert!(matches!(&raw[2], Shape::Path(_)));
            let mut cache = WorldShapes::default();
            let first = cache.vessels(painter, &geometry)[0].clone();
            let repeated = &cache.vessels(painter, &geometry)[0];
            assert!(
                matches!((&first,repeated),(Shape::Mesh(a),Shape::Mesh(b)) if Arc::ptr_eq(a,b))
            );
        });
    }
}
