//! Cache styled AIS/orbital meshes without changing polygon/label ordering.
use eframe::egui::{self, Align2, Color32, Pos2, Shape, Stroke, Vec2};
use globecraft_core::language::Lang;
use globecraft_data::{
    satellites::{Satellite, SatelliteGroup},
    vessels::Vessel,
};
use globecraft_render::live_layers::{LiveGeometry, ProjectedSatellite, ProjectedVessel};
use std::sync::Arc;

const VESSEL_LABEL_ALTITUDE_KM: f64 = 75.0;
const VESSEL_LABEL_LIMIT: usize = 75;
const SATELLITE_LABEL_ALTITUDE_KM: f64 = 900.0;
const SATELLITE_LABEL_LIMIT: usize = 60;
const LABEL_FONT_POINTS: f32 = 10.0;
const LABEL_OFFSET_POINTS: f32 = 8.0;
const SATELLITE_ICON_POINTS: f32 = 5.0;
/// Ships are drawn larger the longer they are, between these sizes (a 120 m ship is
/// drawn at the usual size).
const VESSEL_REFERENCE_LENGTH_M: f32 = 120.0;
const VESSEL_SIZE_RANGE: (f32, f32) = (0.7, 1.6);
/// The line from a satellite down to the point below it, when that is this long and
/// the satellite in a low orbit (from the navigation satellites' 20,000 km, the lines
/// only crossed the screen).
const LEASH_MIN_POINTS: f32 = 6.0;
const LEASH_MAX_ALTITUDE_KM: f64 = 2_000.0;
const LEASH_ALPHA: f32 = 0.3;

/// What kind of ship it is, from its AIS ship-and-cargo type: each kind its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum VesselClass {
    Cargo,
    Tanker,
    Passenger,
    Fishing,
    HighSpeed,
    Tug,
    Service,
    Military,
    Pleasure,
    Other,
    Unknown,
}

impl VesselClass {
    pub const ALL: [Self; 11] = [
        Self::Cargo,
        Self::Tanker,
        Self::Passenger,
        Self::Fishing,
        Self::HighSpeed,
        Self::Tug,
        Self::Service,
        Self::Military,
        Self::Pleasure,
        Self::Other,
        Self::Unknown,
    ];

    /// The AIS ship-and-cargo groups (ITU-R M.1371, table 53).
    pub fn from_code(code: Option<u16>) -> Self {
        match code {
            Some(30) => Self::Fishing,
            Some(31 | 32 | 52) => Self::Tug,
            Some(35 | 55) => Self::Military,
            Some(36 | 37) => Self::Pleasure,
            Some(40..=49) => Self::HighSpeed,
            Some(50 | 51 | 53 | 54 | 58 | 59) => Self::Service,
            Some(60..=69) => Self::Passenger,
            Some(70..=79) => Self::Cargo,
            Some(80..=89) => Self::Tanker,
            Some(90..=99) => Self::Other,
            _ => Self::Unknown,
        }
    }

    /// The selector the tooltip's message chooses its wording by.
    fn selector(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Tanker => "tanker",
            Self::Passenger => "passenger",
            Self::Fishing => "fishing",
            Self::HighSpeed => "high_speed",
            Self::Tug => "tug",
            Self::Service => "service",
            Self::Military => "military",
            Self::Pleasure => "pleasure",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Cargo => "vessel-class-cargo",
            Self::Tanker => "vessel-class-tanker",
            Self::Passenger => "vessel-class-passenger",
            Self::Fishing => "vessel-class-fishing",
            Self::HighSpeed => "vessel-class-high-speed",
            Self::Tug => "vessel-class-tug",
            Self::Service => "vessel-class-service",
            Self::Military => "vessel-class-military",
            Self::Pleasure => "vessel-class-pleasure",
            Self::Other => "vessel-class-other",
            Self::Unknown => "vessel-class-unknown",
        }
    }

    pub fn colour(self) -> Color32 {
        match self {
            Self::Cargo => Color32::from_rgb(112, 216, 96),
            Self::Tanker => Color32::from_rgb(240, 92, 92),
            Self::Passenger => Color32::from_rgb(84, 154, 255),
            Self::Fishing => Color32::from_rgb(255, 162, 92),
            Self::HighSpeed => Color32::from_rgb(255, 224, 72),
            Self::Tug => Color32::from_rgb(52, 224, 229),
            Self::Service => Color32::from_rgb(40, 178, 150),
            Self::Military => Color32::from_rgb(176, 136, 255),
            Self::Pleasure => Color32::from_rgb(255, 112, 204),
            Self::Other => Color32::from_rgb(222, 222, 222),
            Self::Unknown => Color32::from_rgb(160, 168, 178),
        }
    }
}

/// The dark edge around a symbol of this colour.
fn edge(colour: Color32) -> Color32 {
    Color32::from_rgb(colour.r() / 5, colour.g() / 5, colour.b() / 5)
}

/// Each group of satellites its colour…
pub(in crate::app) fn satellite_colour(group: SatelliteGroup) -> Color32 {
    match group {
        SatelliteGroup::Station => Color32::from_rgb(255, 212, 92),
        SatelliteGroup::Navigation => Color32::from_rgb(118, 232, 140),
        SatelliteGroup::Weather => Color32::from_rgb(112, 200, 255),
        SatelliteGroup::Bright => Color32::from_rgb(207, 159, 255),
    }
}

/// …and its symbol: a station with its solar panels, a navigation satellite's
/// diamond, a weather satellite's disc, and a bright satellite's four-pointed star.
pub(in crate::app) fn satellite_symbol(
    group: SatelliteGroup,
    at: Pos2,
    size: f32,
    scale: f32,
) -> Vec<Shape> {
    let fill = satellite_colour(group);
    let stroke = Stroke::new(scale, edge(fill));
    let s = size;
    let polygon = |points: Vec<Vec2>| {
        Shape::convex_polygon(points.into_iter().map(|p| at + p).collect(), fill, stroke)
    };
    match group {
        SatelliteGroup::Station => {
            let panel = |side: f32| {
                polygon(vec![
                    Vec2::new(side * 0.55 * s, -0.45 * s),
                    Vec2::new(side * 1.7 * s, -0.45 * s),
                    Vec2::new(side * 1.7 * s, 0.45 * s),
                    Vec2::new(side * 0.55 * s, 0.45 * s),
                ])
            };
            vec![
                panel(-1.0),
                panel(1.0),
                polygon(vec![
                    Vec2::new(-0.55 * s, -0.7 * s),
                    Vec2::new(0.55 * s, -0.7 * s),
                    Vec2::new(0.55 * s, 0.7 * s),
                    Vec2::new(-0.55 * s, 0.7 * s),
                ]),
            ]
        }
        SatelliteGroup::Navigation => vec![polygon(vec![
            Vec2::new(0.0, -s),
            Vec2::new(s, 0.0),
            Vec2::new(0.0, s),
            Vec2::new(-s, 0.0),
        ])],
        SatelliteGroup::Weather => vec![
            Shape::circle_filled(at, 0.8 * s, fill),
            Shape::circle_stroke(at, 0.8 * s, stroke),
        ],
        SatelliteGroup::Bright => {
            let star: Vec<Vec2> = (0..8)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::FRAC_PI_4;
                    let radius = if i % 2 == 0 { 1.1 * s } else { 0.4 * s };
                    Vec2::new(angle.sin(), -angle.cos()) * radius
                })
                .collect();
            // Not convex, but every point sees the centre: a triangle fan from it.
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(at, fill);
            for point in &star {
                mesh.colored_vertex(at + *point, fill);
            }
            for i in 0..star.len() as u32 {
                mesh.indices
                    .extend_from_slice(&[0, 1 + i, 1 + (i + 1) % star.len() as u32]);
            }
            vec![
                Shape::mesh(mesh),
                Shape::closed_line(star.into_iter().map(|p| at + p).collect(), stroke),
            ]
        }
    }
}
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
        let colour = VesselClass::from_code(vessel.ship_type).colour();
        let size = vessel.length_m.map_or(1.0, |length| {
            (length as f32 / VESSEL_REFERENCE_LENGTH_M)
                .sqrt()
                .clamp(VESSEL_SIZE_RANGE.0, VESSEL_SIZE_RANGE.1)
        });
        let at = projected.position;
        shapes.push(Shape::convex_polygon(
            projected
                .icon
                .iter()
                .map(|corner| at + (*corner - at) * size)
                .collect(),
            colour.gamma_multiply(projected.opacity),
            Stroke::new(1.0 * scale, edge(colour).gamma_multiply(projected.opacity)),
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
            (
                "ship_type",
                VesselClass::from_code(ship.ship_type).selector().into(),
            ),
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

/// A satellite's name, group, orbit and height: `altitude_km` where it is now, or
/// its orbit's mean height when that is not known.
pub(super) fn satellite_tooltip(
    lang: Lang,
    satellite: &Satellite,
    altitude_km: Option<f64>,
) -> String {
    let period = satellite
        .orbit
        .period_minutes()
        .map_or_else(|| "—".to_owned(), |minutes| format!("{minutes:.1} min"));
    let altitude = altitude_km
        .or_else(|| satellite.orbit.altitude_km())
        .map_or_else(|| "—".to_owned(), |km| format!("{km:.0} km"));
    let inclination = format!("{:.1}°", satellite.orbit.inclination_deg);
    lang.format(
        "satellites-hover-tooltip",
        &[
            ("name", satellite.name.as_str().into()),
            ("norad", satellite.norad.as_str().into()),
            ("group", lang.text(satellite.group.label_key()).into()),
            ("period", period.into()),
            ("altitude", altitude.into()),
            ("inclination", inclination.into()),
            (
                "model",
                if satellite.sgp4.is_some() {
                    "sgp4"
                } else {
                    "simple"
                }
                .into(),
            ),
        ],
    )
}

fn build_satellites(painter: &egui::Painter, geometry: &LiveGeometry) -> Vec<Shape> {
    let Some(satellites) = &geometry.input.satellites else {
        return Vec::new();
    };
    let scale = geometry.input.view.map_scale;
    let size = SATELLITE_ICON_POINTS * scale;
    let mut shapes = Vec::with_capacity(3 * geometry.satellites.len() + SATELLITE_LABEL_LIMIT);
    // The lines down to the ground first, under every symbol.
    for projected in geometry.satellites.iter() {
        let Some(ground) = projected.ground else {
            continue;
        };
        if projected.altitude_km > LEASH_MAX_ALTITUDE_KM {
            continue;
        }
        if (ground - projected.position).length() >= LEASH_MIN_POINTS * scale {
            let colour = satellite_colour(satellites[projected.source_index].group);
            shapes.push(Shape::line_segment(
                [ground, projected.position],
                Stroke::new(scale, colour.gamma_multiply(LEASH_ALPHA)),
            ));
        }
    }
    for (index, projected) in geometry.satellites.iter().enumerate() {
        let satellite = &satellites[projected.source_index];
        shapes.extend(satellite_symbol(
            satellite.group,
            projected.position,
            size,
            scale,
        ));
        if geometry.input.view.altitude_km < SATELLITE_LABEL_ALTITUDE_KM
            && index < SATELLITE_LABEL_LIMIT
        {
            shapes.push(label(
                painter,
                projected.position,
                &satellite.name,
                scale,
                satellite_colour(satellite.group),
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
            group: SatelliteGroup::Station,
            sgp4: None,
        };
        let text = satellite_tooltip(Lang::En, &satellite, None);
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
            poi_shown: None,
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
