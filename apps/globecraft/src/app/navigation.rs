use super::App;
use eframe::egui::{Pos2, Rect};
use globecraft_core::flight::Flight;
use globecraft_core::language::LanguagePreference;
use globecraft_core::layers::MapLayer;
use std::time::Instant;

impl App {
    pub(super) fn active_earth_model(&self) -> globecraft_core::earth_model::EarthModel {
        if self.explorer.active == globecraft_core::body::Body::Earth {
            self.settings.earth_model
        } else {
            globecraft_core::earth_model::EarthModel::PerfectSphere
        }
    }
    pub(super) fn set_language_preference(&mut self, preference: LanguagePreference) {
        self.language_preference = preference;
        self.system_locale = sys_locale::get_locale().unwrap_or_default();
        self.lang = preference.resolve(&self.system_locale);
        self.settings_writer.language(preference);
        self.status = self.lang.text("app-ready-status").into();
        // No private Explorer field access: keep user state intact on language changes.
    }

    pub(super) fn touch(&mut self) {
        self.last_interact = Instant::now();
    }

    pub(super) fn fly_to(&mut self, lat: f64, lon: f64, alt: f64) {
        self.fly_to_with_orientation(lat, lon, alt, None, None);
    }

    /// Starts a flight that ends at the requested heading and tilt when given.
    pub(super) fn fly_to_with_orientation(
        &mut self,
        lat: f64,
        lon: f64,
        alt: f64,
        heading_deg: Option<f64>,
        tilt_deg: Option<f64>,
    ) {
        self.fly = Some(
            Flight::new_for_model(self.cam, lat, lon, alt, self.active_earth_model())
                .with_orientation(heading_deg, tilt_deg),
        );
    }

    pub(super) fn step_fly(&mut self, dt: f64) {
        if let Some(mut flight) = self.fly.take() {
            self.cam = flight.advance(dt);
            self.touch();
            if !flight.is_finished() {
                self.fly = Some(flight);
            }
        }
    }

    pub(super) fn pan(&mut self, dx: f64, dy: f64, height_pts: f32) {
        self.cam
            .pan_for_model(dx, dy, height_pts, self.active_earth_model());
    }
    pub(super) fn zoom(&mut self, factor: f64) {
        self.cam.zoom(factor);
    }
    pub(super) fn zoom_at(&mut self, factor: f64, cursor: Option<Pos2>, rect: Rect, aspect: f64) {
        self.cam.zoom_at_over_ground(
            factor,
            cursor,
            rect,
            aspect,
            self.active_earth_model(),
            self.ground_km,
        );
    }

    /// The view as drawn: the camera around the ground point raised to the terrain.
    pub(super) fn view_basis(&self, aspect: f64) -> globecraft_core::camera::Basis {
        self.cam
            .basis_over_ground(aspect, self.active_earth_model(), self.ground_km)
    }

    /// Relief under the overlays, when terrain is drawn.
    pub(super) fn overlay_ground(&self) -> Option<globecraft_render::overlays::OverlayGround> {
        self.terrain_active()
            .then(|| globecraft_render::overlays::OverlayGround {
                exaggeration: self.settings.layers.terrain_exaggeration,
                generation: self.cache.generation(),
            })
    }

    /// What the displayed frame's relief needs to raise points onto the ground: the
    /// tile cache, the relief setting of that frame, and the camera altitude. `None`
    /// setting while relief is off. Owned, so callers can still change the app.
    pub(super) fn ground_source(&self) -> GroundSource {
        GroundSource {
            cache: self.cache.clone(),
            ground: self.renderer.displayed_ground(self.overlay_ground()),
            altitude_km: self.cam.alt,
        }
    }

    /// Whether relief is drawn for the current view.
    pub(super) fn terrain_active(&self) -> bool {
        self.layer(MapLayer::Terrain)
            && self.cam.alt <= globecraft_render::TERRAIN_MAX_ALTITUDE_KM
            && self.explorer.active == globecraft_core::body::Body::Earth
            && !self.flat_earth.open
    }

    /// Eases the looked-at ground height toward the terrain under the view centre, and
    /// keeps a tilted camera above the terrain beneath it.
    pub(super) fn follow_ground(&mut self, ctx: &eframe::egui::Context) {
        let exaggeration = f64::from(self.settings.layers.terrain_exaggeration.clamp(
            globecraft_render::MIN_EXAGGERATION,
            globecraft_render::MAX_EXAGGERATION,
        ));
        let ground_km = |app: &Self, lat: f64, lon: f64, zoom: u8| {
            app.cache
                .elevation_m(lat, lon, zoom)
                .map(|metres| f64::from(metres) / METERS_PER_KM * exaggeration)
        };
        // About a tile across the view: the height of what is looked at, not of
        // single rocks.
        let zoom = (EARTH_CIRCUMFERENCE_KM / self.cam.alt.max(0.05))
            .log2()
            .clamp(0.0, 15.0) as u8;
        let wanted = if self.terrain_active() {
            ground_km(self, self.cam.lat, self.cam.lon, zoom).unwrap_or(self.ground_km)
        } else {
            0.0
        };
        let dt = super::frame_timing::animation_delta_secs(ctx);
        let ease = 1.0 - (-dt / GROUND_EASE_SECS).exp();
        let gap = wanted - self.ground_km;
        if gap.abs() > GROUND_SETTLED_KM {
            self.ground_km += gap * ease;
            ctx.request_repaint();
        } else {
            self.ground_km = wanted;
        }
        if !self.terrain_active() || self.cam.tilt <= 0.0 {
            return;
        }
        // The camera's own position and the terrain under it.
        let basis = self.view_basis(1.0);
        let geometry = self.active_earth_model().geometry();
        let (lat, lon) = geometry.coordinates(basis.c);
        let surface = geometry.position(lat, lon);
        let camera_km = (basis.c - surface).length() * globecraft_core::EARTH_RADIUS_KM;
        let Some(below_km) = ground_km(self, lat, lon, zoom) else {
            return;
        };
        let lacking = below_km + MIN_GROUND_CLEARANCE_KM - camera_km;
        if lacking > 0.0 {
            // Back away along the view line until the camera clears the ground.
            let rise_per_km = self.cam.tilt.to_radians().cos().max(MIN_RISE_PER_KM);
            self.cam.alt += lacking / rise_per_km;
            ctx.request_repaint();
        }
    }
}

const METERS_PER_KM: f64 = 1000.0;
const EARTH_CIRCUMFERENCE_KM: f64 = 40_075.0;
/// The looked-at height follows a new elevation value over about this time.
const GROUND_EASE_SECS: f64 = 0.25;
/// Below this the eased height snaps to the target and stops asking for frames.
const GROUND_SETTLED_KM: f64 = 0.0005;
/// A tilted camera stays at least this high above the terrain beneath it.
const MIN_GROUND_CLEARANCE_KM: f64 = 0.15;
/// Steepest tilt considered when backing away (cos 80°), so the step stays finite.
const MIN_RISE_PER_KM: f64 = 0.17;

/// See [`App::ground_source`].
pub(super) struct GroundSource {
    cache: std::sync::Arc<globecraft_data::tiles::TileCache>,
    ground: Option<globecraft_render::overlays::OverlayGround>,
    altitude_km: f64,
}
impl GroundSource {
    pub(super) fn heights(&self) -> Option<globecraft_render::overlays::GroundHeights<'_>> {
        globecraft_render::overlays::GroundHeights::new(
            Some(&self.cache),
            self.ground,
            self.altitude_km,
        )
    }
    /// The ground under screen point `pos`: on the relief when it is drawn (roadmap
    /// 1.10), else on the surface.
    pub(super) fn pick(
        &self,
        basis: &globecraft_core::camera::Basis,
        rect: eframe::egui::Rect,
        pos: eframe::egui::Pos2,
    ) -> Option<(f64, f64)> {
        match self.heights() {
            Some(heights) => globecraft_render::overlays::pick_on_ground(
                basis,
                rect,
                pos,
                &heights,
                heights.max_height(),
            ),
            None => basis.pick(rect, pos),
        }
    }
}
