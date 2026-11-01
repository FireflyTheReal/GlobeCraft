//! The layers the user added (Add Data): their tile streams and feature loads, kept in
//! step with `settings.user_layers`, and drawn over the imagery: web maps as one
//! projected image under the borders, features (points with names, lines and outlines)
//! over them.
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Shape, Stroke, TextureHandle, Vec2};
use globecraft_core::camera::Basis;
use globecraft_data::user_data::{
    FeatureSet, RasterSnapshot, RasterTiles, UserLayer, UserSource, VectorLayer,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Point markers and their names (points).
const MARKER_POINTS: f32 = 4.0;
const LINE_POINTS: f32 = 2.0;
const LABEL_POINTS: f32 = 11.0;
/// The most names one layer writes in a view.
const MAX_NAMES: usize = 150;
const NAME_SEPARATION_POINTS: f32 = 70.0;

enum Stream {
    Raster {
        tiles: RasterTiles,
        snapshot: Option<Arc<RasterSnapshot>>,
    },
    Vector(VectorLayer),
}

struct Loaded {
    source: UserSource,
    stream: Stream,
}

/// What a layer's feature shapes were built from.
#[derive(PartialEq)]
struct FeatureKey {
    basis: Basis,
    rect: Rect,
    revision: u64,
    opacity: u32,
    color: [u8; 3],
    scale: u32,
}

/// What the web-map image was projected for: the view, its size in pixels, and each
/// layer's id, tile snapshot and opacity.
type RasterKey = (Basis, usize, usize, Vec<(u64, usize, u32)>);

#[derive(Default)]
pub(super) struct UserLayers {
    loaded: HashMap<u64, Loaded>,
    texture: Option<TextureHandle>,
    texture_for: Option<RasterKey>,
    features: HashMap<u64, (FeatureKey, Vec<Shape>)>,
}

/// A layer's state for the Layers pane.
pub(super) enum LayerStatus {
    Ready,
    Loading,
    Failed(String),
}

impl UserLayers {
    /// Starts, keeps or drops each layer's loading to match `layers`, and requests what
    /// the visible ones need for the view.
    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        runtime: &globecraft_data::runtime::ServiceRuntime,
        layers: &[UserLayer],
        view: (f64, f64, f64, f64),
        earth: bool,
    ) {
        self.loaded.retain(|id, loaded| {
            layers
                .iter()
                .any(|l| l.id == *id && l.source == loaded.source)
        });
        self.features
            .retain(|id, _| layers.iter().any(|l| l.id == *id && l.visible));
        for layer in layers {
            let loaded = self.loaded.entry(layer.id).or_insert_with(|| Loaded {
                source: layer.source.clone(),
                stream: match &layer.source {
                    UserSource::Tiles {
                        template,
                        min_zoom,
                        max_zoom,
                        tile_px,
                    } => Stream::Raster {
                        tiles: RasterTiles::new(template, *min_zoom, *max_zoom, *tile_px),
                        snapshot: None,
                    },
                    UserSource::Features {
                        location,
                        format,
                        refresh_minutes,
                    } => Stream::Vector(VectorLayer::new(location, *format, *refresh_minutes)),
                },
            });
            if !layer.visible || !earth {
                continue;
            }
            let wake = ctx.clone();
            let repaint = move || wake.request_repaint();
            match &mut loaded.stream {
                Stream::Raster { tiles, snapshot } => {
                    *snapshot = tiles.update(runtime, view, repaint);
                }
                Stream::Vector(vector) => vector.update(runtime, repaint),
            }
        }
    }

    pub(super) fn status(&self, id: u64) -> LayerStatus {
        let Some(loaded) = self.loaded.get(&id) else {
            return LayerStatus::Loading;
        };
        let (loading, error) = match &loaded.stream {
            Stream::Raster { tiles, .. } => (tiles.loading(), tiles.error()),
            Stream::Vector(vector) => (vector.loading(), vector.error()),
        };
        match (error, loading) {
            (Some(error), _) => LayerStatus::Failed(error),
            (None, true) => LayerStatus::Loading,
            (None, false) => LayerStatus::Ready,
        }
    }

    /// The area a feature layer covers: south, west, north, east.
    pub(super) fn bounds(&self, id: u64) -> Option<[f64; 4]> {
        match &self.loaded.get(&id)?.stream {
            Stream::Vector(vector) => vector.features()?.0.bounds,
            Stream::Raster { .. } => None,
        }
    }

    /// The visible web maps, as one image over the globe.
    pub(super) fn draw_rasters(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        layers: &[UserLayer],
    ) {
        let shown: Vec<(&UserLayer, &Arc<RasterSnapshot>)> = layers
            .iter()
            .filter(|layer| layer.visible && layer.opacity > 0.0)
            .filter_map(|layer| match &self.loaded.get(&layer.id)?.stream {
                Stream::Raster {
                    snapshot: Some(snapshot),
                    ..
                } if !snapshot.is_empty() => Some((layer, snapshot)),
                _ => None,
            })
            .collect();
        if shown.is_empty() {
            return;
        }
        // Coarser while the view moves, as the weather maps.
        let max_width = if painter.ctx().input(|i| i.pointer.any_down()) {
            640
        } else {
            1280
        };
        let width =
            ((rect.width() * painter.ctx().pixels_per_point()) as usize).clamp(160, max_width);
        let height =
            ((width as f32 * rect.height() / rect.width().max(1.0)) as usize).clamp(1, 1280);
        let parts: Vec<(u64, usize, u32)> = shown
            .iter()
            .map(|(layer, snapshot)| {
                (
                    layer.id,
                    Arc::as_ptr(snapshot) as usize,
                    layer.opacity.to_bits(),
                )
            })
            .collect();
        let key = (*basis, width, height, parts);
        if self.texture_for.as_ref() != Some(&key) {
            let inputs: Vec<(&RasterSnapshot, f32)> = shown
                .iter()
                .map(|(layer, snapshot)| (snapshot.as_ref(), layer.opacity))
                .collect();
            let rgba = globecraft_render::weather::project_user_rasters(
                basis, rect, width, height, &inputs,
            );
            let image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            match self.texture.as_mut() {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(painter.ctx().load_texture(
                        "user-web-maps",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.texture_for = Some(key);
        }
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    /// The visible feature layers: lines, then points and their names.
    pub(super) fn draw_features(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        layers: &[UserLayer],
        scale: f32,
    ) {
        for layer in layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
            let Some(Loaded {
                stream: Stream::Vector(vector),
                ..
            }) = self.loaded.get(&layer.id)
            else {
                continue;
            };
            let Some((features, revision)) = vector.features() else {
                continue;
            };
            let key = FeatureKey {
                basis: *basis,
                rect,
                revision,
                opacity: layer.opacity.to_bits(),
                color: layer.rgb(),
                scale: scale.to_bits(),
            };
            let cached = self.features.get(&layer.id);
            if cached.is_none_or(|(built, _)| *built != key) {
                let shapes = feature_shapes(painter, basis, rect, &features, layer, scale);
                self.features.insert(layer.id, (key, shapes));
            }
            if let Some((_, shapes)) = self.features.get(&layer.id) {
                painter.extend(shapes.iter().cloned());
            }
        }
    }
}

fn feature_shapes(
    painter: &egui::Painter,
    basis: &Basis,
    rect: Rect,
    features: &FeatureSet,
    layer: &UserLayer,
    scale: f32,
) -> Vec<Shape> {
    let [r, g, b] = layer.rgb();
    let color = Color32::from_rgb(r, g, b).gamma_multiply(layer.opacity);
    let outline = Color32::from_rgb(20, 24, 30).gamma_multiply(layer.opacity);
    let clip = rect.expand(globecraft_render::overlays::SEGMENT_CLIP_MARGIN_POINTS);
    let mut lines = Vec::new();
    for line in &features.lines {
        let mut run: Vec<Pos2> = Vec::new();
        let mut previous = None;
        for &(lat, lon) in line {
            let current =
                globecraft_render::overlays::project_unhidden(basis, rect, None, None, lat, lon);
            match previous
                .zip(current)
                .and_then(|(a, b)| globecraft_render::overlays::clip_segment([a, b], clip))
            {
                Some([a, b]) if run.last() == Some(&a) => run.push(b),
                Some([a, b]) => {
                    if run.len() >= 2 {
                        for part in super::live_layers::stroke_runs(&std::mem::take(&mut run)) {
                            lines.push(Shape::line(part, Stroke::new(LINE_POINTS * scale, color)));
                        }
                    }
                    run = vec![a, b];
                }
                None if run.len() >= 2 => {
                    for part in super::live_layers::stroke_runs(&std::mem::take(&mut run)) {
                        lines.push(Shape::line(part, Stroke::new(LINE_POINTS * scale, color)));
                    }
                }
                None => run.clear(),
            }
            previous = current;
        }
        if run.len() >= 2 {
            for part in super::live_layers::stroke_runs(&run) {
                lines.push(Shape::line(part, Stroke::new(LINE_POINTS * scale, color)));
            }
        }
    }
    let mut shapes = super::live_geometry::mesh_shapes(painter, lines);
    let font = crate::app::fonts::map_font(painter.ctx(), LABEL_POINTS * scale);
    let separation = NAME_SEPARATION_POINTS * scale;
    let radius = MARKER_POINTS * scale;
    let mut markers = Vec::new();
    let mut named: Vec<Pos2> = Vec::new();
    let mut texts = Vec::new();
    for point in &features.points {
        let Some(at) = basis.project(
            globecraft_render::overlays::ground_position(basis, None, point.lat, point.lon),
            rect,
        ) else {
            continue;
        };
        if !rect.contains(at) {
            continue;
        }
        markers.push(Shape::circle_filled(at, radius, color));
        markers.push(Shape::circle_stroke(
            at,
            radius,
            Stroke::new(1.0 * scale, outline),
        ));
        if point.name.is_empty()
            || named.len() >= MAX_NAMES
            || named.iter().any(|other| other.distance(at) < separation)
        {
            continue;
        }
        named.push(at);
        let position = at + Vec2::new(radius + 4.0 * scale, 0.0);
        let galley = painter.layout_no_wrap(point.name.clone(), font.clone(), Color32::WHITE);
        let shadow = painter.layout_no_wrap(point.name.clone(), font.clone(), Color32::BLACK);
        let top_left = egui::Align2::LEFT_CENTER
            .anchor_size(position, galley.size())
            .min;
        texts.push(Shape::Text(
            egui::epaint::TextShape::new(top_left + Vec2::splat(1.0), shadow, Color32::BLACK)
                .with_opacity_factor(layer.opacity),
        ));
        texts.push(Shape::Text(
            egui::epaint::TextShape::new(top_left, galley, Color32::WHITE)
                .with_opacity_factor(layer.opacity),
        ));
    }
    shapes.extend(super::live_geometry::mesh_shapes(painter, markers));
    shapes.extend(texts);
    shapes
}

/// The highest altitude a fly to a layer's area ends at, and the lowest.
const ZOOM_TO_MAX_KM: f64 = 12_000.0;
const ZOOM_TO_MIN_KM: f64 = 2.0;

impl super::App {
    /// Adds a layer on top of the user's layers; returns its id.
    pub(super) fn add_user_layer(&mut self, mut layer: UserLayer) -> u64 {
        layer.id = self
            .settings
            .user_layers
            .iter()
            .map(|l| l.id)
            .max()
            .map_or(1, |id| id + 1);
        let id = layer.id;
        self.settings.user_layers.push(layer);
        id
    }

    fn user_layer_index(&self, params: &serde_json::Value) -> Result<usize, String> {
        let id = params
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or("id must be a user layer's id (see info.view)")?;
        self.settings
            .user_layers
            .iter()
            .position(|layer| layer.id == id)
            .ok_or_else(|| format!("there is no user layer {id}"))
    }

    /// The `data.*` commands and `ui.addData`.
    pub(super) fn run_user_data(
        &mut self,
        id: &str,
        p: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use globecraft_data::user_data::{FeatureFormat, is_url, presets, wms_template};
        use serde_json::json;
        let text = |key: &str| {
            p.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
        };
        Ok(match id {
            "ui.addData" => {
                self.add_data = Some(Default::default());
                json!({})
            }
            "data.add" => {
                let layer = if let Some(code) = text("preset") {
                    let preset = presets::find(code).ok_or_else(|| {
                        let codes: Vec<&str> = presets::PRESETS.iter().map(|p| p.code).collect();
                        format!("unknown preset; one of {}", codes.join(", "))
                    })?;
                    UserLayer {
                        id: 0,
                        name: self.lang.text(preset.name_key).to_owned(),
                        source: preset.source(),
                        visible: true,
                        opacity: preset.opacity,
                        attribution: preset.attribution.to_owned(),
                        color: preset.color.to_owned(),
                        preset: Some(preset.code.to_owned()),
                    }
                } else {
                    let url = text("url").ok_or("give a preset, or a url and its kind")?;
                    // Files on this computer go through the Add Data window, which
                    // the user drives; commands only take web addresses.
                    if !is_url(url) {
                        return Err("url must start with https:// or http://".into());
                    }
                    let source = match text("kind").unwrap_or("tiles") {
                        "tiles" => UserSource::Tiles {
                            template: url.to_owned(),
                            min_zoom: 0,
                            max_zoom: p
                                .get("max_zoom")
                                .and_then(serde_json::Value::as_u64)
                                .map_or(18, |z| z.min(22) as u8),
                            tile_px: 256,
                        },
                        "wms" => UserSource::Tiles {
                            template: wms_template(
                                url,
                                text("layers")
                                    .ok_or("a WMS needs layers (names, comma-separated)")?,
                            ),
                            min_zoom: 0,
                            max_zoom: 18,
                            tile_px: 256,
                        },
                        "features" => UserSource::Features {
                            location: url.to_owned(),
                            format: text("format")
                                .and_then(FeatureFormat::from_code)
                                .or_else(|| FeatureFormat::guess(url))
                                .ok_or("format must be geojson, gpx or csv")?,
                            refresh_minutes: p
                                .get("refresh_minutes")
                                .and_then(serde_json::Value::as_u64)
                                .map_or(0, |m| m.min(24 * 60) as u32),
                        },
                        _ => return Err("kind must be tiles, wms or features".into()),
                    };
                    UserLayer {
                        id: 0,
                        name: text("name").unwrap_or(url).to_owned(),
                        source,
                        visible: true,
                        opacity: 1.0,
                        attribution: text("attribution").unwrap_or_default().to_owned(),
                        color: "#ffb000".into(),
                        preset: None,
                    }
                };
                let name = layer.name.clone();
                let id = self.add_user_layer(layer);
                json!({"id": id, "name": name})
            }
            "data.remove" => {
                let index = self.user_layer_index(p)?;
                let layer = self.settings.user_layers.remove(index);
                json!({"removed": layer.id, "name": layer.name})
            }
            "data.show" => {
                let index = self.user_layer_index(p)?;
                let layer = &mut self.settings.user_layers[index];
                layer.visible = p
                    .get("visible")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(!layer.visible);
                json!({"id": layer.id, "visible": layer.visible})
            }
            "data.opacity" => {
                let index = self.user_layer_index(p)?;
                let opacity = p
                    .get("opacity")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or("opacity must be a number from 0 to 1")?;
                let layer = &mut self.settings.user_layers[index];
                layer.opacity = (opacity as f32).clamp(0.0, 1.0);
                json!({"id": layer.id, "opacity": layer.opacity})
            }
            "data.move" => {
                let index = self.user_layer_index(p)?;
                let up = text("direction") != Some("down");
                let layers = &mut self.settings.user_layers;
                // Up is towards the top of the drawing order: the end of the list.
                let target = if up {
                    (index + 1).min(layers.len() - 1)
                } else {
                    index.saturating_sub(1)
                };
                layers.swap(index, target);
                json!({"id": layers[target].id, "position": target})
            }
            "data.zoomTo" => {
                let index = self.user_layer_index(p)?;
                let layer_id = self.settings.user_layers[index].id;
                let [south, west, north, east] = self
                    .user_layers
                    .bounds(layer_id)
                    .ok_or("only a loaded feature layer has an area to show")?;
                let lat = (south + north) / 2.0;
                let lon = (west + east) / 2.0;
                let span_km = (north - south).max((east - west) * lat.to_radians().cos()) * 111.0;
                let altitude_km = (span_km * 1.6).clamp(ZOOM_TO_MIN_KM, ZOOM_TO_MAX_KM);
                self.run(
                    "camera.flyTo",
                    json!({"lat": lat, "lon": lon, "altitude_km": altitude_km}),
                )?;
                json!({"lat": lat, "lon": lon, "altitude_km": altitude_km})
            }
            _ => return Err(format!("unknown command '{id}'")),
        })
    }

    /// The user's layers, for the view description.
    pub(super) fn user_layers_json(&self) -> serde_json::Value {
        self.settings
            .user_layers
            .iter()
            .map(|layer| {
                serde_json::json!({
                    "id": layer.id,
                    "name": layer.name,
                    "visible": layer.visible,
                    "opacity": layer.opacity,
                })
            })
            .collect()
    }
}
