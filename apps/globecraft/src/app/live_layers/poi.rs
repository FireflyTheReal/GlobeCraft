//! Places of interest, peaks, waters and parks: rivers and park outlines under the
//! borders, names over them, styled once per projected geometry.
use super::fade::{TiledShapes, styled_line};
use super::poi_memory::PoiMemory;
use eframe::egui::{self, Align2, Color32, FontId, Rect, Shape, Vec2};
use globecraft_data::poi::{PoiLineKind, PointKind};
use globecraft_data::settings::map_style::{LineType, MapStyle};
use globecraft_data::vector_tiles::TileFade;
use globecraft_render::live_layers::{LiveGeometry, PoiKey, PoiShown};
use globecraft_render::overlays::{LabelSpace, LabelSpaceKey, PlacedLabels};
use std::sync::Arc;
use std::time::Instant;

const LABEL_FONT_POINTS: f32 = 11.0;
const WATER_FONT_POINTS: f32 = 12.0;
/// Between a place's badge and its name.
const BADGE_GAP_POINTS: f32 = 3.0;
/// Free room kept around each marker and name, map points.
const LABEL_PADDING_POINTS: f32 = 2.0;
const PARK_LABEL: Color32 = Color32::from_rgb(190, 240, 190);
const LABEL: Color32 = Color32::from_rgb(240, 244, 250);
const WATER_LABEL: Color32 = Color32::from_rgb(150, 200, 255);
/// River names along the river below this altitude, canal names below the next.
const RIVER_NAME_MAX_ALTITUDE_KM: f64 = 120.0;
const CANAL_NAME_MAX_ALTITUDE_KM: f64 = 20.0;

#[derive(Default)]
pub(in crate::app) struct PoiShapes {
    /// What the lines were built from: the projected geometry, scale and style.
    key: Option<(usize, u32, MapStyle)>,
    /// What the names already on screen were placed for: the projected geometry,
    /// scale and the room names of higher priority took.
    shown_key: Option<(usize, u32, LabelSpaceKey)>,
    /// The same for the names newly shown.
    labels_key: Option<(usize, u32, LabelSpaceKey)>,
    lines: TiledShapes,
    /// Names already on screen: placed before the street names, so a name once
    /// drawn is not pushed out by one that appears later.
    shown_labels: TiledShapes,
    shown_pass: Vec<Drawn>,
    shown_placed: PlacedLabels,
    /// Names newly shown, placed last.
    labels: TiledShapes,
    /// The names on screen, which keep their places.
    memory: PoiMemory,
    /// River and canal names written along them, placed before the new place names.
    river_names: TiledShapes,
    river_placed: PlacedLabels,
    river_memory: crate::app::road_names::NameMemory,
}

/// A place a pass drew: its key, the side its name took (`None`: marker only), and
/// its point on screen.
type Drawn = (PoiKey, Option<usize>, egui::Pos2);

/// Which names a placement pass takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Those on screen already.
    Shown,
    /// The others.
    New,
}

/// What one placement pass made.
struct Placement {
    shapes: Vec<(TileFade, Shape)>,
    drawn: Vec<Drawn>,
    /// The room its markers and names take.
    rects: Vec<Rect>,
}

impl PoiShapes {
    pub(in crate::app) fn invalidate(&mut self) {
        self.key = None;
        self.shown_key = None;
        self.labels_key = None;
        self.memory.clear();
    }
    /// Places the names already on screen, where `taken` (place names and the
    /// user's data) is free; `shown_placed` is their room for the layers after.
    pub(in crate::app) fn prepare_shown(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        taken: &LabelSpace,
    ) {
        let source = Arc::as_ptr(&geometry.poi) as usize;
        let scale = geometry.input.view.map_scale;
        let key = (source, scale.to_bits(), taken.key());
        if self.shown_key.as_ref() == Some(&key) {
            return;
        }
        #[cfg(test)]
        self.memory.blocked.clear();
        let placement = labels(
            painter,
            geometry,
            scale,
            taken,
            &mut self.memory,
            Pass::Shown,
        );
        self.shown_labels = TiledShapes::build(painter, placement.shapes);
        self.shown_pass = placement.drawn;
        self.shown_placed.replace(placement.rects);
        self.shown_key = Some(key);
        // The new names place around these again.
        self.labels_key = None;
    }
    /// The room the names already on screen take.
    pub(in crate::app) fn shown_placed(&self) -> &PlacedLabels {
        &self.shown_placed
    }
    /// What the last placement drew: each name's key, side and point.
    #[cfg(test)]
    pub(in crate::app) fn drawn(&self) -> &[(PoiKey, Option<usize>, egui::Pos2)] {
        self.memory.last()
    }
    /// Names whose marker had no room in the last placement.
    #[cfg(test)]
    pub(in crate::app) fn blocked(&self) -> &[PoiKey] {
        &self.memory.blocked
    }
    /// The names on screen, for the projection to keep first.
    pub(in crate::app) fn shown(&self) -> Arc<PoiShown> {
        self.memory.shown()
    }
    /// Brings the shapes up to date; markers and names go only where `taken` is
    /// free (the names of every other map layer come first).
    pub(in crate::app) fn prepare(
        &mut self,
        painter: &egui::Painter,
        geometry: &LiveGeometry,
        style: &MapStyle,
        taken: &LabelSpace,
    ) {
        let source = Arc::as_ptr(&geometry.poi) as usize;
        let scale_bits = geometry.input.view.map_scale.to_bits();
        let scale = geometry.input.view.map_scale;
        let labels_key = (source, scale_bits, taken.key());
        if self.labels_key.as_ref() != Some(&labels_key) {
            let (shapes, rects) =
                river_names(painter, geometry, scale, taken, &mut self.river_memory);
            self.river_names = TiledShapes::build(painter, shapes);
            self.river_placed.replace(rects);
            let taken = taken.with(&self.river_placed);
            let placement = labels(
                painter,
                geometry,
                scale,
                &taken,
                &mut self.memory,
                Pass::New,
            );
            self.labels = TiledShapes::build(painter, placement.shapes);
            let mut drawn = self.shown_pass.clone();
            drawn.extend(placement.drawn);
            self.memory.finish(Instant::now(), drawn);
            self.labels_key = Some(labels_key);
        }
        let key = (source, scale_bits, *style);
        if self.key.as_ref() != Some(&key) {
            self.lines = TiledShapes::build(painter, lines(geometry, style, scale));
            self.key = Some(key);
        }
    }
    /// Rivers, canals and park outlines, drawn under the borders.
    pub(in crate::app) fn draw_lines(&self, painter: &egui::Painter) {
        self.lines.draw(painter, Instant::now());
    }
    /// Markers and names, drawn over the borders.
    pub(in crate::app) fn draw_labels(&self, painter: &egui::Painter) {
        let now = Instant::now();
        self.river_names.draw(painter, now);
        self.shown_labels.draw(painter, now);
        self.labels.draw(painter, now);
    }
}

fn lines(geometry: &LiveGeometry, style: &MapStyle, scale: f32) -> Vec<(TileFade, Shape)> {
    let mut shapes = Vec::new();
    for line in &geometry.poi.lines {
        let fade = geometry
            .input
            .poi
            .as_ref()
            .map_or_else(TileFade::default, |snapshot| snapshot.fade(line.tile));
        let line_style = style.line(match line.kind {
            PoiLineKind::River => LineType::River,
            PoiLineKind::Canal => LineType::Canal,
            PoiLineKind::Park => LineType::Park,
        });
        let mut drawn = Vec::new();
        for segment in &line.segments {
            styled_line(&mut drawn, segment, line_style, scale, line.opacity);
        }
        shapes.extend(drawn.into_iter().map(|shape| (fade, shape)));
    }
    shapes
}

/// River and canal names along them, where `taken` is free, as street names are.
fn river_names(
    painter: &egui::Painter,
    geometry: &LiveGeometry,
    scale: f32,
    taken: &LabelSpace,
    memory: &mut crate::app::road_names::NameMemory,
) -> (Vec<(TileFade, Shape)>, Vec<Rect>) {
    use crate::app::road_names::{NameView, NamedLine, line_names};
    let Some(snapshot) = &geometry.input.poi else {
        return (Vec::new(), Vec::new());
    };
    let view = &geometry.input.view;
    let lines = geometry
        .poi
        .lines
        .iter()
        .filter_map(|projected| {
            let line = snapshot
                .tiles
                .get(projected.tile)?
                .lines
                .get(projected.index)?;
            let max_km = match line.kind {
                PoiLineKind::River => RIVER_NAME_MAX_ALTITUDE_KM,
                PoiLineKind::Canal => CANAL_NAME_MAX_ALTITUDE_KM,
                PoiLineKind::Park => return None,
            };
            Some(NamedLine {
                name: line.name.as_deref()?,
                points: &line.points,
                segments: &projected.segments,
                opacity: projected.opacity
                    * globecraft_render::live_layers::altitude_fade(max_km, view.altitude_km),
                major: line.kind == PoiLineKind::River,
                fade: snapshot.fade(projected.tile),
            })
        })
        .collect();
    line_names(
        painter,
        lines,
        &NameView {
            basis: &view.basis,
            viewport: view.rect,
            altitude_km: view.altitude_km,
            size_points: WATER_FONT_POINTS,
            scale,
        },
        taken,
        memory,
        (WATER_LABEL, true),
    )
}

/// Markers and names. Labels come in priority order, names already on screen first
/// (`memory`); each takes the room it covers on screen, so a later one whose name
/// would overlap an earlier marker or name (or a name of another layer, in `taken`)
/// tries the other side of its marker, and else shows its marker alone (or nothing
/// when even that is covered) — as map apps thin out busy streets. A name on screen
/// tries its side first and keeps fading from when it first showed, whichever tile
/// carries it now, so zooming in neither moves nor refades it.
fn labels(
    painter: &egui::Painter,
    geometry: &LiveGeometry,
    scale: f32,
    taken: &LabelSpace,
    memory: &mut PoiMemory,
    pass: Pass,
) -> Placement {
    let Some(snapshot) = &geometry.input.poi else {
        return Placement {
            shapes: Vec::new(),
            drawn: Vec::new(),
            rects: Vec::new(),
        };
    };
    let font = crate::app::fonts::map_font(painter.ctx(), LABEL_FONT_POINTS * scale);
    let water_font = FontId::new(WATER_FONT_POINTS * scale, font.family.clone());
    let padding = LABEL_PADDING_POINTS * scale;
    let mut placed: Vec<Rect> = Vec::new();
    let mut markers = Vec::new();
    let mut texts = Vec::new();
    let now = Instant::now();
    let mut shown: Vec<Drawn> = Vec::new();
    // Names already on screen go in the order `PoiMemory::tier` gives (stable, so
    // priority order holds within each tier).
    let mut order: Vec<_> = geometry.poi.labels.iter().collect();
    if pass == Pass::Shown {
        order.sort_by_key(|label| {
            snapshot
                .tiles
                .get(label.tile)
                .and_then(|tile| tile.points.get(label.index))
                .and_then(|point| memory.get(&PoiKey::of(point)))
                .map_or(u8::MAX, |entry| memory.tier(entry))
        });
    }
    for label in order {
        let Some(point) = snapshot
            .tiles
            .get(label.tile)
            .and_then(|tile| tile.points.get(label.index))
        else {
            continue;
        };
        let key = PoiKey::of(point);
        let remembered = memory.get(&key);
        if remembered.is_some() != (pass == Pass::Shown) {
            continue;
        }
        let fade = TileFade {
            shown: Some(remembered.map_or(now, |entry| entry.since)),
            retired: snapshot.fade(label.tile).retired,
        };
        let at = label.position;
        let opacity = label.opacity;
        let mut marker = Vec::new();
        let badge = super::poi_icons::point_badge(point);
        if let Some(badge) = badge {
            super::poi_icons::badge(&mut marker, at, badge, scale, opacity);
        }
        let text = match (point.kind, point.elevation_m) {
            (PointKind::Peak, Some(metres)) => format!("{} {metres} m", point.name),
            _ => point.name.clone(),
        };
        let (color, font) = match point.kind {
            // Seas and oceans in the larger type; lakes, ponds and fountains (also
            // OpenMapTiles water names) as small as the other names.
            PointKind::Water if super::poi_icons::is_open_water(&point.class) => {
                (WATER_LABEL, water_font.clone())
            }
            PointKind::Water => (WATER_LABEL, font.clone()),
            PointKind::Park => (PARK_LABEL, font.clone()),
            PointKind::Peak | PointKind::Poi => (LABEL, font.clone()),
        };
        // Names with a badge go beside it; the others are centred on their point.
        let beside =
            badge.map(|_| (super::poi_icons::BADGE_RADIUS_POINTS + BADGE_GAP_POINTS) * scale);
        let marker_rect = Shape::Vec(marker.clone())
            .visual_bounding_rect()
            .expand(padding);
        if marker_rect.is_positive()
            && (placed.iter().any(|other| other.intersects(marker_rect))
                || !taken.is_free(marker_rect))
        {
            #[cfg(test)]
            memory.blocked.push(key.clone());
            continue;
        }
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), color);
        let size = galley.size();
        // Right of the marker, else left of it; centred names have one place.
        let candidates: Vec<egui::Pos2> = match beside {
            None => vec![Align2::CENTER_CENTER.anchor_size(at, size).min],
            Some(offset) => vec![
                Align2::LEFT_CENTER
                    .anchor_size(at + Vec2::new(offset, 0.0), size)
                    .min,
                Align2::RIGHT_CENTER
                    .anchor_size(at - Vec2::new(offset, 0.0), size)
                    .min,
            ],
        };
        // The side it had first.
        let first = remembered.and_then(|entry| entry.side).unwrap_or(0);
        let order = std::iter::once(first).chain((0..candidates.len()).filter(|&i| i != first));
        let side = order.filter(|&i| i < candidates.len()).find(|&i| {
            let rect = Rect::from_min_size(candidates[i], size).expand(padding);
            !placed.iter().any(|other| other.intersects(rect)) && taken.is_free(rect)
        });
        if marker_rect.is_positive() {
            placed.push(marker_rect);
            markers.extend(marker.into_iter().map(|shape| (fade, shape)));
        }
        shown.push((key, side, at));
        let Some(position) = side.map(|i| candidates[i]) else {
            continue;
        };
        placed.push(Rect::from_min_size(position, size).expand(padding));
        let shadow = painter.layout_no_wrap(text, font, Color32::BLACK);
        texts.push((
            fade,
            super::transport::faded_text(position + Vec2::splat(1.0), shadow, opacity),
        ));
        texts.push((
            fade,
            super::transport::faded_text(position, galley, opacity),
        ));
    }
    markers.extend(texts);
    Placement {
        shapes: markers,
        drawn: shown,
        rects: placed,
    }
}

/// A view of a made-up river winding west to east across the screen, for the tests
/// and the screenshot of river names.
#[cfg(test)]
pub(in crate::app) fn river_scene(rect: Rect) -> Arc<LiveGeometry> {
    use globecraft_core::camera::Cam;
    use globecraft_data::poi::{PoiLine, PoiSnapshot, PoiTile};
    use globecraft_render::live_layers::{LiveInput, LivePreparer, LiveView, PoiLayers};
    let (lat, lon, alt) = (50.0, 8.5, 30.0);
    let river: Vec<(f64, f64)> = (0..=80)
        .map(|i| {
            let x = -0.35 + 0.7 * f64::from(i) / 80.0;
            (lat + 0.04 * (x * 9.0).sin(), lon + x)
        })
        .collect();
    let tile = PoiTile {
        points: Vec::new(),
        lines: vec![PoiLine {
            kind: PoiLineKind::River,
            name: Some("Main".into()),
            points: river,
        }],
    };
    let input = LiveInput {
        view: LiveView {
            basis: Cam {
                lat,
                lon,
                heading: 0.0,
                alt,
                tilt: 0.0,
            }
            .basis(f64::from(rect.width() / rect.height())),
            rect,
            altitude_km: alt,
            map_scale: 1.0,
            pixels_per_point: 1.0,
            traffic_width_px: 1,
            traffic_height_px: 1,
            ground: None,
        },
        roads: None,
        vessels: None,
        satellites: None,
        traffic: None,
        transport: None,
        transport_layers: Default::default(),
        poi: Some(Arc::new(PoiSnapshot {
            tiles: vec![Arc::new(tile)],
            zoom: 12,
            fades: Vec::new(),
        })),
        poi_layers: PoiLayers {
            places: true,
            peaks: true,
            waters: true,
            parks: true,
        },
        poi_shown: None,
        unix_secs: 0.0,
    };
    LivePreparer::default().prepare(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_river_is_named_along_its_course() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 500.0));
        let geometry = river_scene(rect);
        let mut shapes = PoiShapes::default();
        let raw = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        let _ = crate::app::test_ui::run(&ctx, raw, |ui| {
            shapes.prepare(
                ui.painter(),
                &geometry,
                &MapStyle::default(),
                &LabelSpace::default(),
            );
        });
        // Placed once (the same name repeats only far apart), on screen.
        let placed = shapes.river_placed.rects();
        assert!(!placed.is_empty(), "the river got no name");
        assert!(placed.iter().all(|r| rect.contains_rect(*r)), "{placed:?}");
        // Another layer's name over the river's middle moves it along.
        let middle = placed[0];
        let mut taken = PlacedLabels::default();
        taken.replace(vec![middle]);
        let space = LabelSpace::default().with(&taken);
        let _ = crate::app::test_ui::run(
            &ctx,
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                shapes.prepare(ui.painter(), &geometry, &MapStyle::default(), &space);
            },
        );
        let moved = shapes.river_placed.rects();
        assert!(moved.iter().all(|r| !r.intersects(middle)), "{moved:?}");
    }
}
