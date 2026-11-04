use eframe::egui::{self, Align2, Color32, Shape, Stroke, Vec2};
use globecraft_data::aircraft::AircraftClass;
use globecraft_render::aircraft::{AircraftGeometry, ProjectedFlight};
use std::sync::OnceLock;

const TRAIL_WIDTH_POINTS: f32 = 1.2;
const LABEL_FONT_POINTS: f32 = 11.0;
const LABEL_OFFSET_POINTS: Vec2 = Vec2::new(9.0, -6.0);
const SHADOW_OFFSET_POINTS: Vec2 = Vec2::new(1.0, 1.0);
// The plane geometry is taken from the bundled SVGs: an airliner for most traffic,
// and a symbol of its own for each other kind of aircraft.
const AIRPLANE_SVG: &str = include_str!("../../../assets/airplane.svg");
const CLASS_SVGS: [(AircraftClass, &str, f32); 8] = [
    (
        AircraftClass::Heavy,
        include_str!("../../../assets/aircraft/heavy.svg"),
        1.25,
    ),
    (
        AircraftClass::Light,
        include_str!("../../../assets/aircraft/light.svg"),
        0.8,
    ),
    (
        AircraftClass::Fast,
        include_str!("../../../assets/aircraft/fast.svg"),
        0.95,
    ),
    (
        AircraftClass::Helicopter,
        include_str!("../../../assets/aircraft/helicopter.svg"),
        0.85,
    ),
    (
        AircraftClass::Glider,
        include_str!("../../../assets/aircraft/glider.svg"),
        0.9,
    ),
    (
        AircraftClass::Balloon,
        include_str!("../../../assets/aircraft/balloon.svg"),
        0.75,
    ),
    (
        AircraftClass::Ultralight,
        include_str!("../../../assets/aircraft/ultralight.svg"),
        0.75,
    ),
    (
        AircraftClass::Drone,
        include_str!("../../../assets/aircraft/drone.svg"),
        0.7,
    ),
];
const SVG_CENTER_POINTS: f32 = 12.0;
const AIRPLANE_SCALE_POINTS: f32 = 0.78;
const AIRPLANE_OUTLINE_SCALE: f32 = 1.11;
const AIRPLANE_FILL: Color32 = Color32::from_rgb(243, 250, 255);
const AIRPLANE_OUTLINE: Color32 = Color32::from_rgb(22, 52, 72);
const LABEL_COLOR: Color32 = Color32::from_rgb(227, 245, 255);
const TRAIL_RGBA: [u8; 4] = [114, 211, 249, 90];
/// The selected plane: amber, a third larger, drawn above everything else.
const SELECTED_FILL: Color32 = Color32::from_rgb(255, 196, 61);
const SELECTED_OUTLINE: Color32 = Color32::from_rgb(46, 28, 0);
const SELECTED_SCALE: f32 = 1.35;

struct SvgAirplane {
    // Each SVG polygon is convex, allowing efficient, indexed triangle fans.
    polygons: Vec<Vec<Vec2>>,
    /// Its size next to the airliner's.
    scale: f32,
}

/// The symbol of a kind of aircraft.
fn airplane_symbol(class: AircraftClass) -> &'static SvgAirplane {
    static SYMBOLS: OnceLock<Vec<(AircraftClass, SvgAirplane)>> = OnceLock::new();
    let symbols = SYMBOLS.get_or_init(|| {
        CLASS_SVGS
            .iter()
            .map(|(class, svg, scale)| (*class, parse_symbol(svg, *scale)))
            .collect()
    });
    static AIRLINER: OnceLock<SvgAirplane> = OnceLock::new();
    symbols
        .iter()
        .find(|(kind, _)| *kind == class)
        .map(|(_, symbol)| symbol)
        .unwrap_or_else(|| AIRLINER.get_or_init(|| parse_symbol(AIRPLANE_SVG, 1.0)))
}

fn parse_symbol(svg: &str, scale: f32) -> SvgAirplane {
    {
        let polygons = svg
            .split("<polygon")
            .skip(1)
            .map(|element| {
                let (_, attributes) = element
                    .split_once("points=\"")
                    .expect("bundled airplane SVG must have polygon points");
                let (points, _) = attributes
                    .split_once('"')
                    .expect("bundled airplane SVG must close polygon points");
                let polygon: Vec<Vec2> = points
                    .split_ascii_whitespace()
                    .map(|pair| {
                        let (x, y) = pair.split_once(',').expect("bundled SVG point must be x,y");
                        Vec2::new(
                            x.parse().expect("SVG x must be a number"),
                            y.parse().expect("SVG y must be a number"),
                        )
                    })
                    .collect();
                assert!(
                    polygon.len() >= 3,
                    "airplane SVG polygons need three points"
                );
                polygon
            })
            .collect();
        SvgAirplane { polygons, scale }
    }
}

/// The selected plane, in its own colours above the others.
pub(super) fn selected_plane(
    flight: &ProjectedFlight,
    class: AircraftClass,
    map_scale: f32,
) -> Shape {
    let mut mesh = egui::Mesh::default();
    let size = map_scale * SELECTED_SCALE;
    add_airplane_mesh(
        &mut mesh,
        (flight.position, heading(flight, class)),
        size,
        airplane_symbol(class),
        (SELECTED_FILL, SELECTED_OUTLINE),
        1.0,
    );
    Shape::mesh(mesh)
}

/// A kind of aircraft's symbol, nose up, for the legend.
pub(in crate::app) fn class_symbol(class: AircraftClass, center: egui::Pos2, size: f32) -> Shape {
    let mut mesh = egui::Mesh::default();
    let symbol = airplane_symbol(class);
    add_airplane_mesh(
        &mut mesh,
        (center, Vec2::new(0.0, -1.0)),
        size / (24.0 * AIRPLANE_SCALE_POINTS * 1.25),
        symbol,
        (AIRPLANE_FILL, AIRPLANE_OUTLINE),
        1.0,
    );
    Shape::mesh(mesh)
}

/// Which way a flight's nose points on screen; a balloon stays upright.
fn heading(flight: &ProjectedFlight, class: AircraftClass) -> Vec2 {
    if class == AircraftClass::Balloon {
        Vec2::new(0.0, -1.0)
    } else {
        (flight.icon[0] - flight.position).normalized()
    }
}

fn add_airplane_mesh(
    mesh: &mut egui::Mesh,
    (position, forward): (egui::Pos2, Vec2),
    map_scale: f32,
    symbol: &SvgAirplane,
    (fill, outline): (Color32, Color32),
    opacity: f32,
) {
    // The projection already computes the direction of the aircraft on screen, so
    // the SVG turns as the camera/globe turns, even near the poles.
    let right = Vec2::new(-forward.y, forward.x);
    let icon_size = AIRPLANE_SCALE_POINTS * map_scale * symbol.scale;

    // Draw the dark silhouette beneath all SVG parts, then the light silhouette.
    // This creates an exterior outline without outlines between overlapping wings.
    for (scale, color) in [(AIRPLANE_OUTLINE_SCALE, outline), (1.0, fill)] {
        for polygon in &symbol.polygons {
            let base = mesh.vertices.len() as u32;
            for point in polygon {
                let dx = (point.x - SVG_CENTER_POINTS) * icon_size * scale;
                let dy = (SVG_CENTER_POINTS - point.y) * icon_size * scale;
                mesh.colored_vertex(
                    position + right * dx + forward * dy,
                    color.gamma_multiply(opacity),
                );
            }
            for i in 1..polygon.len() - 1 {
                mesh.indices
                    .extend_from_slice(&[base, base + i as u32, base + i as u32 + 1]);
            }
        }
    }
}

pub(super) fn build(painter: &egui::Painter, geometry: &AircraftGeometry) -> Vec<Shape> {
    #[cfg(feature = "profiling")]
    let _profile = globecraft_data::profiling::Span::new("ui.aircraft_shapes");
    let ms = geometry.view.map_scale;
    let [r, g, b, a] = TRAIL_RGBA;
    let trail_stroke = Stroke::new(
        TRAIL_WIDTH_POINTS * ms,
        Color32::from_rgba_unmultiplied(r, g, b, a),
    );
    let mut output = Vec::new();
    let mut trails = Vec::new();
    let mut planes = egui::Mesh::default();
    // Resolved before `fonts_mut`: `map_font` reads the fonts too, and egui's context
    // lock is not re-entrant, so resolving it inside the closure froze the app as soon
    // as the first label appeared (below 650 km).
    let label_font = crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * ms);
    let mut labels = geometry.labels.iter().copied().peekable();
    for (index, flight) in geometry.flights.iter().enumerate() {
        let stroke = Stroke::new(
            trail_stroke.width,
            trail_stroke.color.gamma_multiply(flight.opacity),
        );
        trails.push(Shape::line_segment(flight.trail, stroke));
        let class = geometry.data.aircraft[flight.source_index].class;
        add_airplane_mesh(
            &mut planes,
            (flight.position, heading(flight, class)),
            ms,
            airplane_symbol(class),
            (AIRPLANE_FILL, AIRPLANE_OUTLINE),
            flight.opacity,
        );
        if labels.peek() == Some(&index) {
            labels.next();
            flush_icons(painter, &mut trails, &mut planes, &mut output);
            let sample = &geometry.data.aircraft[flight.source_index];
            let code = if sample.callsign.is_empty() {
                sample.hex.to_uppercase()
            } else {
                sample.callsign.to_string()
            };
            let position = flight.position + LABEL_OFFSET_POINTS * ms;
            for (offset, color) in [
                (SHADOW_OFFSET_POINTS, Color32::BLACK),
                (Vec2::ZERO, LABEL_COLOR),
            ] {
                output.push(painter.fonts_mut(|fonts| {
                    Shape::text(
                        fonts,
                        position + offset,
                        Align2::LEFT_BOTTOM,
                        &code,
                        label_font.clone(),
                        color,
                    )
                }));
            }
        }
    }
    flush_icons(painter, &mut trails, &mut planes, &mut output);
    output
}

fn flush_icons(
    painter: &egui::Painter,
    trails: &mut Vec<Shape>,
    planes: &mut egui::Mesh,
    output: &mut Vec<Shape>,
) {
    if !planes.indices.is_empty() {
        trails.push(Shape::mesh(std::mem::take(planes)));
    }
    if trails.is_empty() {
        return;
    }
    let clipped = std::mem::take(trails)
        .into_iter()
        .map(|shape| egui::epaint::ClippedShape {
            clip_rect: painter.clip_rect(),
            shape,
        })
        .collect();
    // Cache the planes/trails as meshes; retain label Galleys and original ordering.
    for primitive in painter
        .ctx()
        .tessellate(clipped, painter.ctx().pixels_per_point())
    {
        match primitive.primitive {
            egui::epaint::Primitive::Mesh(mesh) => output.push(Shape::mesh(mesh)),
            egui::epaint::Primitive::Callback(_) => {
                unreachable!("aircraft contain only polygons and lines")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_svg_contains_real_airplane_wings_and_fuselage() {
        let symbol = airplane_symbol(AircraftClass::Unknown);
        assert_eq!(symbol.polygons.len(), 5);
        assert!(symbol.polygons.iter().all(|polygon| polygon.len() >= 4));
        assert!(std::ptr::eq(
            symbol,
            airplane_symbol(AircraftClass::Airliner)
        ));
    }

    /// Every kind of aircraft has its own symbol, inside the 24-unit box, and every
    /// polygon is convex (they are drawn as triangle fans).
    #[test]
    fn every_aircraft_class_has_a_convex_symbol() {
        for class in AircraftClass::ALL {
            let symbol = airplane_symbol(class);
            if class != AircraftClass::Airliner {
                assert!(
                    !std::ptr::eq(symbol, airplane_symbol(AircraftClass::Unknown)),
                    "{class:?}"
                );
            }
            for polygon in &symbol.polygons {
                assert!(polygon.len() >= 3, "{class:?}");
                assert!(
                    polygon
                        .iter()
                        .all(|p| (0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y)),
                    "{class:?}"
                );
                let turns: Vec<f32> = (0..polygon.len())
                    .map(|i| {
                        let [a, b, c] = [0, 1, 2].map(|k| polygon[(i + k) % polygon.len()]);
                        (b - a).x * (c - b).y - (b - a).y * (c - b).x
                    })
                    .filter(|turn| turn.abs() > 1e-4)
                    .collect();
                assert!(
                    turns.iter().all(|t| *t > 0.0) || turns.iter().all(|t| *t < 0.0),
                    "{class:?}: {polygon:?}"
                );
            }
        }
    }
}
