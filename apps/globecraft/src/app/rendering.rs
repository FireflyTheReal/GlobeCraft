use super::App;
use eframe::egui;
use globecraft_core::layers::MapLayer;
use globecraft_core::{
    camera::{Basis, Cam},
    math::{ll_to_v3, now_unix, subsolar},
};
use globecraft_data::tiles::Provider;
use globecraft_render::RenderCfg;
use globecraft_render::worker::{RenderRequest, RenderWorker};
use std::sync::Arc;

/// Raster scale of the swipe compare's frame while the camera moves.
const COMPARE_MOTION_SCALE: f32 = 0.5;

struct PreparedFrame {
    image: egui::ColorImage,
    /// The swipe compare's release of the same view, and that release.
    compare: Option<(egui::ColorImage, u16)>,
    overlays: Option<super::overlays::PreparedOverlay>,
    /// The border set being exchanged out, projected for the same view.
    fading_out: Option<super::overlays::PreparedOverlay>,
    aircraft: Option<Arc<globecraft_render::aircraft::AircraftGeometry>>,
    live_layers: Option<Arc<globecraft_render::live_layers::LiveGeometry>>,
}

/// The compare release's raster, handed from the render step to the prepare step of
/// the same job.
type CompareRaster = Arc<std::sync::Mutex<Option<(usize, usize, u16, Vec<u8>)>>>;

pub(super) struct Renderer {
    worker: RenderWorker<PreparedFrame>,
    displayed: Option<(Cam, Basis)>,
    displayed_rect: Option<egui::Rect>,
    displayed_grid_step: Option<f64>,
    /// The relief setting the displayed frame's overlays were projected with (`None`
    /// before the first frame). Drawing must use it: the live one carries the tile
    /// cache's generation, which moves with every arriving tile, so overlays prepared
    /// with a frame never matched it while tiles streamed in and lines flickered.
    displayed_ground: Option<Option<globecraft_render::overlays::OverlayGround>>,
    pub(super) displayed_sky: bool,
    pub(super) displayed_credits: String,
    waiting: Vec<globecraft_data::tiles::TileKey>,
    required: Vec<globecraft_data::tiles::TileKey>,
    retry_at: Option<std::time::Instant>,
}

impl Renderer {
    pub(super) fn new(cache: Arc<globecraft_data::tiles::TileCache>, ctx: egui::Context) -> Self {
        let admission_cache = cache.clone();
        let mut backend = globecraft_render::backend::FrameRenderer::new(
            globecraft_render::backend::BackendPreference::from_environment(),
        );
        let mut overlay_preparer =
            globecraft_render::overlays::OverlayPreparer::with_elevation(cache.clone());
        // The last published overlay, reused while the geometry is (tile-only frames).
        let mut last_overlay: Option<super::overlays::PreparedOverlay> = None;
        // A second preparer for the border set fading out, so the two sets do not
        // evict each other's index.
        let mut fading_preparer =
            globecraft_render::overlays::OverlayPreparer::with_elevation(cache.clone());
        let mut last_fading: Option<super::overlays::PreparedOverlay> = None;
        let overlay_ctx = ctx.clone();
        let mut live_preparer = globecraft_render::live_layers::LivePreparer::default();
        let mut aircraft_preparer = globecraft_render::aircraft::AircraftPreparer::default();
        // The swipe compare renders in the same job as the main frame, right after it:
        // both images arrive together for the same camera, their missing tiles reach
        // the cache as one list, and a tile of either release wakes the next frame.
        let compare_raster = CompareRaster::default();
        let compare_out = compare_raster.clone();
        Self {
            worker: RenderWorker::with_prepare(
                move |request, buffer| {
                    #[cfg(feature = "profiling")]
                    let mut profile = globecraft_data::profiling::Span::new("frame.raster");
                    #[cfg(feature = "profiling")]
                    {
                        profile.values.width = request.width;
                        profile.values.height = request.height;
                        profile.values.inflight = cache.inflight();
                        // The view: altitude in metres and tilt in tenths of a degree, so a
                        // capture shows where slow frames happened.
                        profile.values.resident_bytes = (request.cam.alt * 1000.0) as usize;
                        profile.values.count = (request.cam.tilt * 10.0) as usize;
                    }
                    let mut frame = backend.render(request, &cache, buffer);
                    if let Some(compare) = request.compare {
                        let size = |n: usize| ((n as f32 * compare.scale).round() as usize).max(1);
                        let compare_request = RenderRequest {
                            width: size(request.width),
                            height: size(request.height),
                            cam: request.cam,
                            basis: request.basis,
                            cfg: RenderCfg {
                                rel: compare.rel,
                                lod_scale: request.cfg.lod_scale * compare.scale,
                                ..request.cfg.clone()
                            },
                            overlays: None,
                            aircraft: None,
                            live_layers: None,
                            compare: None,
                            presentation: request.presentation,
                        };
                        let second = backend.render(&compare_request, &cache, Vec::new());
                        frame.waiting.extend(second.waiting);
                        frame.requests.extend(second.requests);
                        if let Ok(mut slot) = compare_raster.lock() {
                            *slot = Some((
                                compare_request.width,
                                compare_request.height,
                                compare.rel,
                                second.pixels,
                            ));
                        }
                    }
                    frame
                },
                move || ctx.request_repaint(),
                move |frame| {
                    admission_cache.prioritize_view(&frame.required_tiles());
                },
                move |request, pixels| {
                    #[cfg(feature = "profiling")]
                    let _prepare = globecraft_data::profiling::Span::new("frame.prepare");
                    #[cfg(feature = "profiling")]
                    let conversion = globecraft_data::profiling::Span::new("frame.convert");
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [request.width, request.height],
                        pixels,
                    );
                    #[cfg(feature = "profiling")]
                    drop(conversion);
                    let geometry = request.overlays.as_ref().map(|input| {
                        let geometry =
                            overlay_preparer.prepare(globecraft_render::overlays::OverlayInput {
                                view: input.view,
                                data: input.data.clone(),
                                fading_out: None,
                            });
                        super::overlays::PreparedOverlay::reuse_or_new(
                            &mut last_overlay,
                            geometry,
                            &overlay_ctx,
                        )
                    });
                    let fading_out = request.overlays.as_ref().and_then(|input| {
                        let data = input.fading_out.clone()?;
                        let geometry =
                            fading_preparer.prepare(globecraft_render::overlays::OverlayInput {
                                view: input.view,
                                data,
                                fading_out: None,
                            });
                        Some(super::overlays::PreparedOverlay::reuse_or_new(
                            &mut last_fading,
                            geometry,
                            &overlay_ctx,
                        ))
                    });
                    let compare = request
                        .compare
                        .and_then(|_| compare_out.lock().ok()?.take())
                        .map(|(width, height, rel, pixels)| {
                            let image =
                                egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
                            (image, rel)
                        });
                    PreparedFrame {
                        image,
                        compare,
                        overlays: geometry,
                        fading_out,
                        live_layers: request.live_layers.clone().map(|mut input| {
                            input.unix_secs = now_unix();
                            live_preparer.prepare(input)
                        }),
                        aircraft: request.aircraft.clone().map(|mut input| {
                            input.unix_secs = now_unix();
                            aircraft_preparer.prepare(input)
                        }),
                    }
                },
            )
            .expect("start frame renderer"),
            displayed: None,
            displayed_rect: None,
            displayed_grid_step: None,
            displayed_ground: None,
            displayed_sky: false,
            displayed_credits: String::new(),
            waiting: Vec::new(),
            required: Vec::new(),
            retry_at: None,
        }
    }
    pub(super) fn tiles_changed(&self, cache: &globecraft_data::tiles::TileCache) -> bool {
        self.waiting
            .iter()
            .any(|&key| matches!(cache.lookup(key), globecraft_data::tiles::Lookup::Ready(_)))
    }
    pub(super) fn refill_requests(
        &mut self,
        cache: &globecraft_data::tiles::TileCache,
        changed: bool,
        ctx: &egui::Context,
    ) {
        let now = std::time::Instant::now();
        if changed || self.retry_at.is_some_and(|deadline| now >= deadline) {
            cache.prioritize_view(&self.required);
            self.retry_at = cache.retry_delay(&self.required).map(|delay| now + delay);
        }
        if let Some(deadline) = self.retry_at {
            ctx.request_repaint_after(deadline.saturating_duration_since(now));
        }
    }
    /// Whether the last frame still misses imagery it wants.
    pub(super) fn waits_for_tiles(&self) -> bool {
        !self.required.is_empty()
    }
    pub(super) fn is_idle(&self) -> bool {
        self.worker.is_idle()
    }
    pub(super) fn picking_basis(&self, fallback: Basis) -> Basis {
        displayed_basis(self.displayed, fallback)
    }
    /// The relief setting of the displayed frame's overlays, or `current` before one.
    pub(super) fn displayed_ground(
        &self,
        current: Option<globecraft_render::overlays::OverlayGround>,
    ) -> Option<globecraft_render::overlays::OverlayGround> {
        self.displayed_ground.unwrap_or(current)
    }
    pub(super) fn displayed_grid_step(&self) -> Option<f64> {
        self.displayed_grid_step
    }
    pub(super) fn displayed_rect(&self) -> Option<egui::Rect> {
        self.displayed_rect
    }
    pub(super) fn displayed_view(&self) -> Option<(Cam, Basis)> {
        self.displayed
    }
}

/// Raster size and the displayed-density scale that chose it, passed together
/// into a frame request so the level of detail and the sampling resolution
/// stay decidable in one place.
pub(super) struct RenderSize {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) lod_scale: f32,
}

impl App {
    /// What a frame of the current view shows (basemap, lighting, terrain, layers), for
    /// a raster of `width`×`height` sampled at `lod_scale` of the displayed density.
    pub(super) fn frame_config(
        &mut self,
        width: usize,
        height: usize,
        lod_scale: f32,
    ) -> RenderCfg {
        let grid_step = globecraft_render::grid_step(self.cam.alt, width, height);
        let sun = if self.layer(MapLayer::Sunlight) {
            let (la, lo) = subsolar(now_unix());
            Some(ll_to_v3(la, lo))
        } else {
            None
        };
        let prov = self.effective_provider();
        let rel = if prov == Provider::Wayback {
            self.wayback_idx as u16
        } else {
            0
        };
        let sky = if let Some(image) = self.sky.image.clone().filter(|_| self.sky_ready()) {
            let interval = globecraft_render::view::SCENE_TIME_UPDATE_INTERVAL_SECS;
            let time = (now_unix() / interval).floor() * interval;
            let (latitude, longitude) = subsolar(time);
            match globecraft_render::celestial::SkyBackdrop::new(
                image,
                time,
                self.cam,
                ll_to_v3(latitude, longitude),
                // Switched on, the Milky Way shows at any hour (user, 2026-10-27).
                true,
            ) {
                Ok(sky) => Some(sky),
                Err(error) => {
                    self.status = error.to_string();
                    None
                }
            }
        } else {
            None
        };
        RenderCfg {
            sky,
            provider: prov,
            terrain: self.terrain_active().then(|| {
                globecraft_render::TerrainCfg::new(self.settings.layers.terrain_exaggeration)
            }),
            rel,
            grid: self.layer(MapLayer::Grid),
            grid_step,
            atmo: self.layer(MapLayer::Atmosphere),
            sun,
            lod_scale,
            level_blend: true,
            fixed_level: None,
            night_lights: self.layer(MapLayer::NightLights)
                && self.explorer.active == globecraft_core::body::Body::Earth,
            stars: self.layer(MapLayer::Stars),
        }
    }

    pub(super) fn request_render(
        &mut self,
        size: RenderSize,
        aspect: f64,
        presentation: u64,
        rect: egui::Rect,
        pixels_per_point: f32,
    ) {
        let basis = self.view_basis(aspect);
        let cfg = self.frame_config(size.width, size.height, size.lod_scale);
        let grid_step = cfg.grid_step;
        // While the camera moves the compare side renders at a lower resolution, so it
        // costs a quarter of the main frame; at rest it is full size.
        let compare =
            self.active_compare()
                .map(|compare| globecraft_render::worker::CompareRelease {
                    rel: compare.index as u16,
                    scale: if size.lod_scale < 1.0 {
                        COMPARE_MOTION_SCALE
                    } else {
                        1.0
                    },
                });
        if compare.is_none() {
            self.compare_texture = None;
        }
        self.renderer.worker.submit(RenderRequest {
            width: size.width,
            height: size.height,
            cam: self.cam,
            basis,
            cfg,
            compare,
            presentation,
            aircraft: (self.layer(MapLayer::Flights)
                && self.explorer.active == globecraft_core::body::Body::Earth)
                .then(|| self.aircraft_input(basis, rect, self.cam.alt, pixels_per_point)),
            live_layers: (self.layer(MapLayer::Roads)
                || self.layer(MapLayer::Traffic)
                || self.layer(MapLayer::Vessels)
                || self.layer(MapLayer::Satellites))
            .then(|| self.live_layer_input(basis, rect, self.cam, pixels_per_point)),
            overlays: Some(globecraft_render::overlays::OverlayInput {
                view: globecraft_render::overlays::OverlayView {
                    basis,
                    rect,
                    altitude_km: self.cam.alt,
                    state_visible_max_km: self.settings.state_max_altitude_km,
                    grid_step_deg: self.layer(MapLayer::GridLabels).then_some(grid_step),
                    layers: self.overlay_layers(),
                    dashes: globecraft_render::overlays::BorderDashes::of(&self.settings.map_style),
                    ground: self.overlay_ground(),
                },
                data: self.boundaries.clone(),
                fading_out: self.layer_arrivals.fading_out(std::time::Instant::now()),
            }),
        });
    }

    pub(super) fn poll_render(&mut self, presentation: u64, ctx: &egui::Context) {
        let Some(frame) = self.renderer.worker.take_completed(presentation) else {
            return;
        };
        #[cfg(feature = "profiling")]
        globecraft_data::profiling::Span::elapsed(
            "frame.delivery",
            frame.submitted,
            globecraft_data::profiling::Values {
                width: frame.request.width,
                height: frame.request.height,
                ..Default::default()
            },
        );
        #[cfg(feature = "profiling")]
        let _upload = globecraft_data::profiling::Span::new("ui.texture_submit");
        let (w, h, buf) = (frame.request.width, frame.request.height, frame.pixels);

        let PreparedFrame {
            image: img,
            compare,
            overlays,
            fading_out,
            aircraft,
            live_layers,
        } = frame.prepared;
        if let Some(geometry) = live_layers {
            self.live_geometry.accept(geometry);
        }
        if let Some(geometry) = aircraft
            && Arc::ptr_eq(&geometry.data, &self.live_aircraft.snapshot())
            && geometry.view.map_scale == self.settings.map_scale
            && geometry.view.pixels_per_point == ctx.pixels_per_point()
        {
            self.aircraft_overlay.accept(geometry, now_unix());
        }
        if let Some(geometry) = overlays {
            self.overlay_cache.accept(geometry);
        }
        self.overlay_cache.accept_fading_out(fading_out);
        match compare {
            Some((image, rel)) => {
                let size = image.size;
                match &mut self.compare_texture {
                    Some((texture, shown)) if texture.size() == size => {
                        texture.set_partial([0, 0], image, egui::TextureOptions::LINEAR);
                        *shown = rel;
                    }
                    Some((texture, shown)) => {
                        texture.set(image, egui::TextureOptions::LINEAR);
                        *shown = rel;
                    }
                    None => {
                        let texture =
                            ctx.load_texture("globe-compare", image, egui::TextureOptions::LINEAR);
                        self.compare_texture = Some((texture, rel));
                    }
                }
            }
            None => self.compare_texture = None,
        }
        // A new frame of the view on screen (tiles arrived, or the view came to
        // rest) fades in over the picture it replaces instead of popping in (roadmap
        // 1.6). The two textures swap roles, so the old picture costs no upload.
        let same_view = self.texture.is_some()
            && self
                .renderer
                .displayed
                .is_some_and(|(cam, _)| same_camera(cam, frame.request.cam));
        if same_view {
            std::mem::swap(&mut self.texture, &mut self.fade_from);
            self.fade_started = Some(std::time::Instant::now());
        } else {
            self.fade_started = None;
        }
        match &mut self.texture {
            Some(t) if t.size() == [w, h] => {
                t.set_partial([0, 0], img, egui::TextureOptions::LINEAR)
            }
            Some(t) => t.set(img, egui::TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture("globe", img, egui::TextureOptions::LINEAR))
            }
        }
        if let Some((_, _, previous)) = self.image.replace((w, h, Arc::new(buf)))
            && let Ok(previous) = Arc::try_unwrap(previous)
        {
            self.renderer.worker.recycle(previous);
        }
        self.renderer.waiting = frame.waiting;
        self.renderer.required = frame.required;
        self.renderer.retry_at = None;
        // Recheck dependencies even if a tile finished before frame publication.
        // Recheck dependencies even if a tile finished before frame publication.
        self.wake.dependencies_may_have_changed();
        self.renderer.displayed_rect = frame.request.overlays.as_ref().map(|input| input.view.rect);
        self.renderer.displayed_ground = frame
            .request
            .overlays
            .as_ref()
            .map(|input| input.view.ground);
        self.renderer.displayed_grid_step = frame
            .request
            .cfg
            .grid
            .then_some(frame.request.cfg.grid_step);
        self.renderer.displayed_sky = frame.request.cfg.sky.is_some();
        self.renderer.displayed_credits = frame
            .request
            .cfg
            .provider
            .attribution(globecraft_core::language::Lang::En)
            .to_string();
        if frame.request.cfg.night_lights {
            self.renderer.displayed_credits.push_str("; ");
            self.renderer.displayed_credits.push_str(
                globecraft_data::tiles::Provider::NightLights
                    .attribution(globecraft_core::language::Lang::En),
            );
        }
        if self.renderer.displayed_sky {
            self.renderer.displayed_credits.push_str("; ");
            self.renderer
                .displayed_credits
                .push_str(globecraft_data::sky::CREDIT);
            self.renderer
                .displayed_credits
                .push_str("; https://svs.gsfc.nasa.gov/4851/; GlobeCraft SDR derivative");
        }
        self.renderer.displayed = Some((frame.request.cam, frame.request.basis));
    }
}

/// Two frames show the same view when their cameras match exactly.
fn same_camera(a: Cam, b: Cam) -> bool {
    [a.lat, a.lon, a.alt, a.heading, a.tilt]
        .iter()
        .zip([b.lat, b.lon, b.alt, b.heading, b.tilt])
        .all(|(x, y)| x.to_bits() == y.to_bits())
}

// Input must describe the same globe as the frame being presented.
fn displayed_basis(displayed: Option<(Cam, Basis)>, fallback: Basis) -> Basis {
    displayed.map_or(fallback, |(_, basis)| basis)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picking_tracks_displayed_camera_while_new_view_is_pending() {
        let shown = Cam {
            lat: 20.0,
            lon: 30.0,
            heading: 0.0,
            alt: 14000.0,
            tilt: 0.0,
        };
        let requested = Cam { lon: 90.0, ..shown };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let shown_basis = shown.basis(4.0 / 3.0);
        let requested_basis =
            requested.basis_for_model(4.0 / 3.0, globecraft_core::earth_model::EarthModel::Wgs84);
        assert!(displayed_basis(Some((shown, shown_basis)), requested_basis) == shown_basis);
        let pick = displayed_basis(Some((shown, shown_basis)), requested_basis)
            .pick(rect, rect.center())
            .unwrap();
        assert!((pick.0 - shown.lat).abs() < 1e-8);
        assert!((pick.1 - shown.lon).abs() < 1e-8);
        assert_eq!(
            displayed_basis(None, requested_basis).pick(rect, rect.center()),
            requested_basis.pick(rect, rect.center())
        );
    }
}
