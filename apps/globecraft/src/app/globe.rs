use super::App;
use super::render_wake;
use super::render_wake::WakeOutcome;
use super::rendering::RenderSize;
use super::{OVERVIEW_WARMUP_ALTITUDE_KM, PAN_SPEED_POINTS_PER_SECOND};
use eframe::egui::{
    self, Align2, Color32, FontId, Key, PointerButton, Pos2, Rect, Sense, Stroke, Vec2,
};
use globecraft_core::layers::MapLayer;
use globecraft_core::math::{now_unix, wrap_lon};
use globecraft_data::tiles::Provider;
use globecraft_render::overlays::LabelSpace;
use std::time::{Duration, Instant};
mod ruler_input;

/// A new frame of the view on screen fades in over the old one for this long
/// (roadmap 1.6: arriving tiles and the sharp frame after motion no longer pop in).
const FRAME_FADE: Duration = Duration::from_millis(250);
const DRAG_HEADING_DEG_PER_POINT: f64 = 0.3;
/// Right-drag or Shift+drag up and down tilts the view.
const DRAG_TILT_DEG_PER_POINT: f64 = 0.25;
/// Shift+scroll (a laptop's two-finger scroll) tilts the view.
const SCROLL_TILT_DEG_PER_POINT: f64 = 0.15;
/// Shift+Up and Shift+Down tilt the view.
const KEY_TILT_DEG_PER_SECOND: f64 = 40.0;
const SCROLL_ZOOM_RATE_PER_POINT: f64 = 0.0025;
const POINT_FLY_ALTITUDE_RATIO: f64 = 0.35;
const MIN_POINT_FLY_ALTITUDE_KM: f64 = 0.3;
const AUTO_ROTATE_DEG_PER_SECOND: f64 = 6.0;
// Lower resolution while interacting; recover full resolution after the idle delay.
const IDLE_RENDER_DELAY: Duration = Duration::from_millis(250);
const OVERVIEW_WARMUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

impl App {
    pub(super) fn globe_view(&mut self, ui: &mut egui::Ui) {
        let lg = self.lang;
        let viewport = ui.available_rect_before_wrap();
        let rect = self.renderer.displayed_rect().unwrap_or(viewport);
        let resp = ui.allocate_rect(viewport, Sense::click_and_drag());
        let ppp = ui.pixels_per_point();
        let aspect = (rect.width() / rect.height()) as f64;
        let shift = ui.input(|i| i.modifiers.shift);

        let ruler_captured = self.edit_ruler(ui, &resp, rect, aspect);

        // ── input
        // Press and hold opens "What's here?"; once it has, the press neither pans nor
        // clicks.
        let press = {
            let (down, pos, now) =
                ui.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos(), i.time));
            let on_globe = down && resp.is_pointer_button_down_on() && !ruler_captured;
            self.long_press.update(on_globe, pos, now)
        };
        if let Some(secs) = press.wake_in_secs {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(secs));
        }
        let mut interacting = self.controller_input(ui, rect.height());
        if press.swallow {
            // The held press stays put.
        } else if !ruler_captured && resp.dragged_by(PointerButton::Primary) && !shift {
            let d =
                ui.input(|i| i.pointer.delta()) * self.settings.navigation.pan_sensitivity as f32;
            self.fly = None;
            if self.explorer.active == globecraft_core::body::Body::Earth {
                self.pan(d.x as f64, d.y as f64, rect.height());
            } else {
                self.cam.orbit_body(d.x as f64, d.y as f64, rect.height());
            }
            interacting = true;
        } else if !ruler_captured
            && (resp.dragged_by(PointerButton::Secondary)
                || (resp.dragged_by(PointerButton::Primary) && shift))
        {
            let drag = ui.input(|i| i.pointer.delta());
            let sensitivity = self.settings.navigation.rotation_sensitivity;
            self.cam.heading = wrap_lon(
                self.cam.heading - drag.x as f64 * DRAG_HEADING_DEG_PER_POINT * sensitivity,
            );
            // Up and down tilt: dragging up leans the view toward the horizon.
            if self.explorer.active == globecraft_core::body::Body::Earth {
                self.cam.tilt_zooming_out(
                    self.cam.tilt - drag.y as f64 * DRAG_TILT_DEG_PER_POINT * sensitivity,
                );
            }
            interacting = true;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            // Shift+scroll tilts (the system may turn it into a sideways scroll).
            let tilt_scroll = shift && self.explorer.active == globecraft_core::body::Body::Earth;
            let sc = if tilt_scroll { 0.0 } else { scroll.y as f64 };
            if tilt_scroll {
                let amount = if scroll.y != 0.0 { scroll.y } else { scroll.x } as f64;
                if amount != 0.0 {
                    self.fly = None;
                    self.cam
                        .tilt_zooming_out(self.cam.tilt + amount * SCROLL_TILT_DEG_PER_POINT);
                    interacting = true;
                }
            }
            let zd = ui.input(|i| i.zoom_delta()) as f64;
            let cur = resp.hover_pos();
            if sc != 0.0 {
                self.fly = None;
                let direction = if self.settings.navigation.invert_scroll {
                    -1.0
                } else {
                    1.0
                };
                let factor = (-sc
                    * SCROLL_ZOOM_RATE_PER_POINT
                    * self.settings.navigation.zoom_sensitivity
                    * direction)
                    .exp();
                if self.explorer.active == globecraft_core::body::Body::Earth {
                    self.zoom_at(factor, cur, rect, aspect);
                } else {
                    self.zoom(factor);
                }
                interacting = true;
            }
            // Trackpad two-finger rotation turns the map with the fingers, as the
            // navigator's ring does.
            let turn = f64::from(ui.input(|i| i.rotation_delta()));
            if turn != 0.0 {
                self.fly = None;
                self.cam.heading = wrap_lon(
                    self.cam.heading
                        - turn.to_degrees() * self.settings.navigation.rotation_sensitivity,
                );
                interacting = true;
            }
            if (zd - 1.0).abs() > 1e-4 {
                self.fly = None;
                if self.explorer.active == globecraft_core::body::Body::Earth {
                    self.zoom_at(1.0 / zd, cur, rect, aspect);
                } else {
                    self.zoom(1.0 / zd);
                }
                interacting = true;
            }
        }
        // The arrow keys move the globe unless they move the focus between controls.
        if !ui.egui_wants_keyboard_input() && ui.memory(|m| m.focused().is_none()) {
            let dt = super::frame_timing::animation_delta_secs(ui);
            let sp = PAN_SPEED_POINTS_PER_SECOND * dt;
            let (mut dx, mut dy) = (0.0, 0.0);
            // Shift+Up/Down tilt instead of moving.
            let tilt_keys = ui.input(|i| i.modifiers.shift)
                && self.explorer.active == globecraft_core::body::Body::Earth;
            if tilt_keys {
                let tilt = ui.input(|i| {
                    f64::from(u8::from(i.key_down(Key::ArrowUp)))
                        - f64::from(u8::from(i.key_down(Key::ArrowDown)))
                });
                if tilt != 0.0 {
                    self.fly = None;
                    self.cam
                        .tilt_zooming_out(self.cam.tilt + tilt * KEY_TILT_DEG_PER_SECOND * dt);
                    interacting = true;
                }
            }
            ui.input(|i| {
                if tilt_keys {
                    return;
                }
                if i.key_down(Key::ArrowLeft) {
                    dx += sp;
                }
                if i.key_down(Key::ArrowRight) {
                    dx -= sp;
                }
                if i.key_down(Key::ArrowUp) {
                    dy += sp;
                }
                if i.key_down(Key::ArrowDown) {
                    dy -= sp;
                }
            });
            if dx != 0.0 || dy != 0.0 {
                self.fly = None;
                if self.explorer.active == globecraft_core::body::Body::Earth {
                    self.pan(dx, dy, rect.height());
                } else {
                    self.cam.orbit_body(dx, dy, rect.height());
                }
                interacting = true;
            }
            let (zin, zout, north, top_down) = ui.input(|i| {
                (
                    i.key_down(Key::Plus) || i.key_down(Key::Equals) || i.key_down(Key::PageUp),
                    i.key_down(Key::Minus) || i.key_down(Key::PageDown),
                    i.key_pressed(Key::N),
                    i.key_pressed(Key::U),
                )
            });
            if top_down {
                self.cam.tilt = 0.0;
                interacting = true;
            }
            if zin {
                self.zoom((-1.2 * dt).exp());
                interacting = true;
            }
            if zout {
                self.zoom((1.2 * dt).exp());
                interacting = true;
            }
            if north {
                self.cam.heading = 0.0;
            }
        }
        if interacting || resp.dragged() {
            self.touch();
        }

        let basis = self.view_basis(aspect);
        // ── rendering
        // A playing time moves the sun every frame: it renders like a moving camera.
        let idle = self.last_interact.elapsed() > IDLE_RENDER_DELAY
            && !resp.dragged()
            && self.time.speed() == 0.0;
        let render_aspect = (viewport.width() / viewport.height()) as f64;
        let relief = self.terrain_active();
        let (w, h) = globecraft_render::view::raster_size(viewport.size(), ppp, idle, relief);
        let keys = globecraft_render::view::RasterView {
            camera: self.cam,
            model: self.active_earth_model(),
            width: w,
            height: h,
            aspect: render_aspect,
            provider: self.effective_provider(),
            release_index: self.wayback.index,
            compare: self.active_compare().map(|compare| compare.index),
            grid: self.layer(MapLayer::Grid),
            atmosphere: self.layer(MapLayer::Atmosphere),
            terrain: relief,
            terrain_exaggeration: self.settings.layers.terrain_exaggeration,
            ground_km: self.ground_km,
            sunlight: self.layer(MapLayer::Sunlight),
            night_lights: self
                .layer(MapLayer::NightLights)
                .then(|| self.night_lights_edition()),
            sky: self.sky_ready(),
            cinematic_sky: true,
            stars: self.layer(MapLayer::Stars),
        }
        .keys(now_unix());
        let state = keys.state;
        let generation = self.cache.generation();
        let presentation = keys.presentation;
        self.poll_render(presentation, ui);
        let rect = self.renderer.displayed_rect().unwrap_or(viewport);

        let pick_basis = self.renderer.picking_basis(basis);
        let ground = self.ground_source();
        let pick = |p: egui::Pos2| ground.pick(&pick_basis, rect, p);
        self.cursor = resp
            .hover_pos()
            .filter(|p| rect.contains(*p))
            .and_then(pick);

        let start_ruler_path = self.measure.on && ruler_input::starts_new_path(&resp);
        if let Some((lat, lon)) = press.fire_at.filter(|p| rect.contains(*p)).and_then(pick) {
            self.run_reporting("ui.here", serde_json::json!({"lat": lat, "lon": lon}));
        }
        if press.swallow {
            // The long press's own release is not a click.
        } else if !ruler_captured && !start_ruler_path && resp.double_clicked() {
            if let Some((la, lo)) = resp
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
                .and_then(pick)
            {
                self.fly_to(
                    la,
                    lo,
                    (self.cam.alt * POINT_FLY_ALTITUDE_RATIO).max(MIN_POINT_FLY_ALTITUDE_KM),
                );
            }
        } else if !ruler_captured
            && resp.clicked()
            && !self.measure.on
            && self.layer(MapLayer::Satellites)
            && let Some(norad) = resp
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
                .and_then(|p| self.satellite_selection.at(p, self.settings.map_scale))
                .map(str::to_owned)
        {
            self.run_reporting("satellite.select", serde_json::json!({"norad": norad}));
        } else if !ruler_captured
            && resp.clicked()
            && !self.measure.on
            && self.layer(MapLayer::Flights)
            && let Some(plane) = resp
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
                .and_then(|p| self.aircraft_overlay.flight_at(p))
        {
            self.run_reporting(
                "flight.select",
                serde_json::json!({"hex": &*plane.hex, "callsign": plane.callsign.trim()}),
            );
        } else if !ruler_captured
            && resp.clicked()
            && !self.measure.on
            && (self.flight_selection.selected().is_some()
                || self.satellite_selection.selected().is_some())
        {
            // A click beside the planes and satellites closes what was selected.
            if self.flight_selection.selected().is_some() {
                self.run_reporting("flight.clear", serde_json::json!({}));
            }
            if self.satellite_selection.selected().is_some() {
                self.run_reporting("satellite.clear", serde_json::json!({}));
            }
        } else if !ruler_captured
            && resp.clicked()
            && self.measure.on
            && let Some(pt) = resp
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
                .and_then(pick)
        {
            self.measure.paths.add_point(pt, start_ruler_path);
        }
        // On touch screens egui opens the context menu on its own, longer, long touch:
        // the card has opened by then.
        let menu_suppressed = press.swallow && resp.secondary_clicked();
        if resp.secondary_clicked() && !menu_suppressed {
            self.menu_pick = resp
                .interact_pointer_pos()
                .filter(|p| rect.contains(*p))
                .and_then(pick);
        }

        // What this pass owes the frame pipeline: a render now, a scheduled
        // wake-up for tiles arriving inside the redraw horizon, or nothing.
        // `changed_before` preserves the pre-decision marker for tile request
        // prioritization, which runs after the pass consumed its own result.
        let changed_before = self.wake.changed(generation);
        let outcome = self.wake.decide(
            render_wake::RenderPass {
                state,
                generation,
                tiles_changed: self.renderer.tiles_changed(&self.cache),
                has_texture: self.texture.is_some(),
                needs_first_frame: self.texture.is_none() && self.renderer.is_idle(),
                // The panes do not animate, so the view is always at rest.
                at_rest: true,
            },
            self.last_state,
            Some(self.last_render.elapsed()),
        );
        match outcome {
            WakeOutcome::Submit => {
                let lod_scale =
                    globecraft_render::view::render_scale(viewport.size(), ppp, idle, relief);
                self.request_render(
                    RenderSize {
                        width: w,
                        height: h,
                        lod_scale,
                    },
                    render_aspect,
                    presentation,
                    viewport,
                    ppp,
                );
                self.last_state = state;
                self.last_render = Instant::now();
            }
            WakeOutcome::Defer(remaining) => ui.request_repaint_after(remaining),
            WakeOutcome::Pause | WakeOutcome::Settled => {}
        }
        if state == self.last_state && self.renderer.is_idle() {
            let refill = changed_before && !matches!(outcome, WakeOutcome::Submit);
            self.renderer.refill_requests(&self.cache, refill, ui);
        }
        if !idle {
            ui.request_repaint();
        }
        // The frame worker admits visible tiles before becoming idle. Speculative
        // work waits for it, and pauses during interaction/closer zoom/other layers.
        if idle
            && self.renderer.is_idle()
            && self.settings.layers.basemap == Provider::Satellite
            && self.cam.alt >= OVERVIEW_WARMUP_ALTITUDE_KM
            && self.tile_warmup.pump(&self.cache)
        {
            ui.request_repaint_after(OVERVIEW_WARMUP_POLL_INTERVAL);
        }
        // A flight's destination imagery loads while it flies.
        match &self.fly {
            Some(flight) => {
                let destination = flight.destination();
                let listed_for = [
                    destination.lat,
                    destination.lon,
                    destination.alt,
                    destination.heading,
                    destination.tilt,
                ]
                .map(f64::to_bits);
                if self
                    .arrival_warmup
                    .as_ref()
                    .is_none_or(|(listed, _)| *listed != listed_for)
                {
                    let provider = self.effective_provider();
                    let mut releases = vec![if provider == Provider::Wayback {
                        self.wayback.index as u16
                    } else {
                        0
                    }];
                    if let Some(compare) = self.active_compare() {
                        releases.push(compare.index as u16);
                    }
                    let keys = globecraft_render::zoom_out::arrival_tiles(
                        destination,
                        h,
                        render_aspect,
                        provider,
                        &releases,
                    );
                    self.arrival_warmup = Some((
                        listed_for,
                        globecraft_data::tiles::TileWarmup::of_keys(keys),
                    ));
                }
                if let Some((_, warmup)) = &mut self.arrival_warmup {
                    warmup.pump(&self.cache);
                }
            }
            None => self.arrival_warmup = None,
        }
        // The coarser imagery a zoom out will show, loaded once the view rests (see
        // `globecraft_render::zoom_out`); a new view starts a new list.
        if idle && self.renderer.is_idle() {
            if self
                .zoom_out_warmup
                .as_ref()
                .is_none_or(|(view, _)| *view != state)
            {
                let provider = self.effective_provider();
                let mut releases = vec![if provider == Provider::Wayback {
                    self.wayback.index as u16
                } else {
                    0
                }];
                if let Some(compare) = self.active_compare() {
                    releases.push(compare.index as u16);
                }
                let keys = globecraft_render::zoom_out::zoom_out_tiles(
                    self.cam,
                    h,
                    render_aspect,
                    provider,
                    &releases,
                );
                self.zoom_out_warmup =
                    Some((state, globecraft_data::tiles::TileWarmup::of_keys(keys)));
            }
            if let Some((_, warmup)) = &mut self.zoom_out_warmup
                && warmup.pump(&self.cache)
            {
                ui.request_repaint_after(OVERVIEW_WARMUP_POLL_INTERVAL);
            }
        }

        // ── drawing
        ui.painter_at(viewport)
            .rect_filled(viewport, 0.0, Color32::BLACK);
        // Until a matching frame arrives, clip the previous view at its original
        // size rather than stretching its texture and projected overlays.
        let painter = ui.painter_at(rect);
        let fade = self
            .fade_started
            .map(|started| started.elapsed().as_secs_f32() / FRAME_FADE.as_secs_f32())
            .filter(|&progress| progress < 1.0);
        if fade.is_none() {
            self.fade_started = None;
        }
        let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        if let (Some(progress), Some(from)) = (fade, &self.fade_from) {
            painter.image(from.id(), rect, whole, Color32::WHITE);
            ui.ctx().request_repaint();
            if let Some(tex) = &self.texture {
                // Eased, so the change starts and ends gently.
                let alpha = progress * progress * (3.0 - 2.0 * progress);
                painter.image(tex.id(), rect, whole, Color32::WHITE.gamma_multiply(alpha));
            }
        } else if let Some(tex) = &self.texture {
            painter.image(tex.id(), rect, whole, Color32::WHITE);
        }
        // The swipe compare: the compare release's frame, right of the split.
        if let (Some(compare), Some((texture, _))) = (
            self.active_compare(),
            self.wayback
                .compare_texture
                .as_ref()
                .filter(|(_, shown)| *shown == compare_release(self)),
        ) {
            let x = rect.left() + rect.width() * compare.split;
            painter
                .with_clip_rect(Rect::from_min_max(Pos2::new(x, rect.top()), rect.max))
                .image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
        }
        if self.explorer.active != globecraft_core::body::Body::Earth {
            painter.rect_filled(rect, 0.0, Color32::BLACK);
            if let Some(sky) = self
                .explorer
                .sky
                .as_ref()
                .filter(|_| self.explorer.active == globecraft_core::body::Body::Custom)
            {
                painter.image(
                    sky.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            if let Some(body) = self.explorer.active_texture() {
                draw_planet(
                    &painter,
                    rect,
                    body,
                    self.cam.lat,
                    self.cam.lon,
                    self.cam.alt,
                );
            }
        }
        // Keep projected borders and labels aligned with the displayed raster
        // while the coordinator works on the latest camera position.
        let (display_cam, display_basis) =
            self.renderer.displayed_view().unwrap_or((self.cam, basis));
        if self.explorer.active == globecraft_core::body::Body::Earth {
            // Layers with an opacity (roadmap 6.1) draw through a faded painter.
            let opacity = self.settings.layers.opacity;
            let faded = |layer: MapLayer| {
                let mut faded = painter.clone();
                faded.multiply_opacity(opacity.get(layer));
                faded
            };
            self.live_maps.draw(
                &painter,
                &display_basis,
                rect,
                self.layer(MapLayer::Clouds)
                    .then(|| opacity.get(MapLayer::Clouds)),
                self.layer(MapLayer::Radar)
                    .then(|| opacity.get(MapLayer::Radar)),
            );
            self.user_layers.draw_rasters(
                &painter,
                &display_basis,
                rect,
                &self.settings.user_layers,
            );
            let live_geometry = if self.layer(MapLayer::Roads)
                || self.layer(MapLayer::Traffic)
                || self.layer(MapLayer::Vessels)
                || self.layer(MapLayer::Satellites)
                || self.map_data_shown()
            {
                let input = self.live_layer_input(
                    display_basis,
                    rect,
                    display_cam,
                    painter.ctx().pixels_per_point(),
                );
                self.live_geometry.update(input)
            } else {
                None
            };
            self.paint_aurora(&faded(MapLayer::Aurora), &display_basis, rect);
            self.paint_time_zones(&faded(MapLayer::TimeZones), &display_basis, rect);
            self.paint_antarctic_claims(&painter, &display_basis, rect);
            // All map names share one screen, placed in priority order, each layer
            // where the ones before it left room (painting keeps its own order):
            // the markers of the user's data (always drawn), the room of place
            // names already on screen, places of interest already on screen (a
            // name once drawn stays while the view zooms), place names, the
            // user's point names, street names, airports, then new places of
            // interest.
            self.user_layers.prepare_features(
                &painter,
                &display_basis,
                rect,
                &self.settings.user_layers,
                self.settings.map_scale,
                &super::user_layers::FeatureTime {
                    time: self.time,
                    wall: now_unix(),
                },
            );
            let frame = self.overlay_frame(&painter, &display_basis, rect, display_cam.alt);
            let mut taken = LabelSpace::default();
            taken.add(self.user_layers.markers());
            if let Some(geometry) = &live_geometry {
                // Place names keep their room even when the places of interest
                // were projected first, as after a jump. When places of interest
                // are on screen but none has settled yet, every place name
                // reserves: this is the first view after the jump, and a
                // district or city name that was never drawn must still be able
                // to claim its room.
                let include_new =
                    self.poi_shapes.has_shown_name() && !self.poi_shapes.has_settled_name();
                let remembered = self.reserve_place_names(&painter, frame, &taken, include_new);
                self.poi_shapes
                    .prepare_shown(&painter, geometry, &taken.with(&remembered));
                taken.add(self.poi_shapes.shown_placed());
            }
            self.prepare_overlays(&painter, frame, &taken);
            taken.add(&self.overlay_cache.placed());
            self.user_layers
                .prepare_names(&painter, self.settings.map_scale, &taken);
            taken.add(self.user_layers.placed());
            // Sea and undersea names where the land's names left room.
            self.paint_marine_names(&painter, &display_basis, rect, &taken);
            taken.add(&self.marine_placed.clone());
            if self.layer(MapLayer::Roads)
                && let Some(geometry) = &live_geometry
            {
                self.roads.draw(
                    &painter,
                    geometry,
                    self.settings.map_scale,
                    &self.settings.map_style,
                    &taken,
                );
                taken.add(self.roads.placed());
            }
            if self.layer(MapLayer::Traffic)
                && let Some(geometry) = &live_geometry
            {
                self.traffic.draw(&faded(MapLayer::Traffic), geometry);
            }
            if let Some(geometry) = &live_geometry {
                let style = self.settings.map_style;
                self.transport_shapes
                    .prepare(&painter, geometry, &style, &taken);
                taken.add(self.transport_shapes.placed());
                self.poi_shapes.prepare(&painter, geometry, &style, &taken);
                self.poi_shapes.draw_lines(&painter);
                self.transport_shapes.draw_lines(&painter);
            }
            self.draw_overlays(&painter, &display_basis, rect);
            if let Some(geometry) = &live_geometry {
                self.poi_shapes.draw_labels(&painter);
                self.transport_shapes.draw_airports(&painter);
                self.transport_shapes.hover(&painter, geometry, self.lang);
            }
            self.user_layers.paint_features(&painter);
            self.user_layers.hover(&painter, rect, self.lang);
            self.draw_live_layers(
                &painter,
                &display_basis,
                rect,
                display_cam.alt,
                live_geometry.as_deref(),
            );
            self.draw_tile_diagnostics(&painter, &display_basis, rect);
        }

        // Google Earth style navigation controls (rotate / move / zoom)
        let dt_nav = super::frame_timing::navigation_delta_secs(ui);
        if self.nav_controls(ui, rect, dt_nav) {
            self.touch();
            ui.request_repaint();
        }
        self.timeline_bar(&ui.ctx().clone(), rect);
        // One card at a time (the last selected; see `close_other_cards`).
        self.selected_flight_card(&ui.ctx().clone(), rect);
        if self.layer(MapLayer::Satellites) {
            self.selected_satellite_card(&ui.ctx().clone(), rect);
        }
        self.here_window(&ui.ctx().clone(), rect);
        self.legend_watch();

        // scale bar
        if let Some(scale_bar) = self.scale_bar_cache.for_view(
            &display_basis,
            rect,
            display_cam.alt,
            self.distance_units(),
        ) {
            let len = scale_bar.width_points;
            let y = rect.bottom() - scale_bar.bottom_inset_points;
            let x0 = rect.left() + globecraft_render::scale_bar::SCALE_LEFT_INSET_POINTS;
            painter.line_segment(
                [Pos2::new(x0, y), Pos2::new(x0 + len, y)],
                Stroke::new(3.0_f32, Color32::BLACK),
            );
            painter.line_segment(
                [Pos2::new(x0, y), Pos2::new(x0 + len, y)],
                Stroke::new(1.5_f32, Color32::WHITE),
            );
            let label = self.distance_units().scale_label(scale_bar.distance_m);
            painter.text(
                Pos2::new(x0, y - 6.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::proportional(12.0 * self.settings.map_scale),
                Color32::WHITE,
            );
        }
        self.compare_split(ui, &painter, rect);
        let credits = self.map_credits(lg, &display_basis, rect);
        self.paint_credits(&painter, rect, &credits);

        if self.explorer.active == globecraft_core::body::Body::Earth
            && (self.layer(MapLayer::Clouds) || self.layer(MapLayer::Radar))
        {
            let source = self.live_maps.status(
                self.layer(MapLayer::Clouds),
                self.layer(MapLayer::Radar),
                lg,
            );
            let credit = if self.layer(MapLayer::Clouds) && self.layer(MapLayer::Radar) {
                format!(
                    "{}\n{}",
                    lg.text("live-cloud-credit"),
                    lg.text("live-radar-credit")
                )
            } else if self.layer(MapLayer::Clouds) {
                lg.text("live-cloud-credit").into()
            } else {
                lg.text("live-radar-credit").into()
            };
            painter.text(
                rect.left_bottom() + Vec2::new(12.0, -58.0),
                Align2::LEFT_BOTTOM,
                format!("{source}\n{credit}"),
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        // Context menu (right-click): every entry is a command at the picked position.
        let pick = self.menu_pick;
        let mut chosen: Option<(&str, serde_json::Value)> = None;
        let show_menu = |ui: &mut egui::Ui| match pick {
            Some((la, lo)) => {
                match self.settings.coordinate_display.format(la, lo) {
                    Ok(text) => {
                        ui.label(text);
                    }
                    Err(error) => {
                        ui.label(error.to_string());
                    }
                }
                ui.separator();
                let at = serde_json::json!({"lat": la, "lon": lo});
                let fly = serde_json::json!({"lat": la, "lon": lo,
                    "altitude_km": (self.cam.alt * POINT_FLY_ALTITUDE_RATIO).max(MIN_POINT_FLY_ALTITUDE_KM)});
                for (icon, key, id, params) in [
                    ("places", "globe-placemark-action", "places.add", at.clone()),
                    ("fly", "globe-fly-to-action", "camera.flyTo", fly),
                    (
                        "measure",
                        "globe-ruler-point-action",
                        "ruler.addPoint",
                        at.clone(),
                    ),
                    ("copy", "globe-copy-coordinates-action", "copy", at.clone()),
                    ("identify", "globe-whats-here-action", "ui.here", at),
                ] {
                    if super::ui::icons::button(ui, icon, lg.text(key)).clicked() {
                        chosen = Some((id, params));
                        ui.close();
                    }
                }
            }
            None => {
                ui.label(lg.text("globe-context-menu-hint"));
            }
        };
        if !menu_suppressed {
            resp.context_menu(show_menu);
        }
        match (chosen, pick) {
            (Some(("copy", _)), Some((la, lo))) => {
                match self.settings.coordinate_display.format(la, lo) {
                    Ok(text) => ui.copy_text(text),
                    Err(error) => self.status = error.to_string(),
                }
            }
            (Some((id, params)), _) => self.run_reporting(id, params),
            _ => {}
        }

        // auto-rotate (keeps repainting)
        if self.auto_rotate && self.fly.is_none() && !ruler_captured {
            let dt = super::frame_timing::animation_delta_secs(ui);
            self.cam.lon = wrap_lon(self.cam.lon + AUTO_ROTATE_DEG_PER_SECOND * dt);
            self.touch();
            ui.request_repaint();
        }
    }
}

fn draw_planet(
    painter: &egui::Painter,
    rect: Rect,
    tex: &egui::TextureHandle,
    lat: f64,
    lon: f64,
    altitude_km: f64,
) {
    let geometry = globecraft_render::planet::planet_mesh(rect, lat, lon, altitude_km);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    mesh.vertices = geometry
        .vertices
        .into_iter()
        .map(|vertex| egui::epaint::Vertex {
            pos: vertex.pos,
            uv: vertex.uv,
            color: Color32::WHITE,
        })
        .collect();
    mesh.indices = geometry.indices;
    painter.add(egui::Shape::mesh(mesh));
}

/// The release index the swipe compare shows right of the split.
fn compare_release(app: &App) -> u16 {
    app.active_compare()
        .map_or(0, |compare| compare.index as u16)
}

/// Width of the grab area around the swipe-compare split, in points.
const SPLIT_GRAB_POINTS: f32 = 24.0;
const SPLIT_KNOB_RADIUS_POINTS: f32 = 13.0;
/// The date drop-downs at the top of the split.
const SPLIT_MENU_WIDTH_POINTS: f32 = 170.0;
const SPLIT_MENU_HEIGHT_POINTS: f32 = 24.0;
const SPLIT_MENU_GAP_POINTS: f32 = 14.0;
const SPLIT_MENU_MARGIN_POINTS: f32 = 6.0;
const SPLIT_SWAP_SIZE_POINTS: f32 = 22.0;

impl App {
    /// The Wayback swipe compare's split: a vertical line with a knob that drags left and
    /// right, and each side's date at the top. Registered after the globe's own input,
    /// so a drag that starts on the split moves the split, not the globe.
    fn compare_split(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, rect: Rect) {
        let Some(mut compare) = self.active_compare() else {
            return;
        };
        let x = rect.left() + rect.width() * compare.split;
        let grab = Rect::from_x_y_ranges(
            (x - SPLIT_GRAB_POINTS / 2.0)..=(x + SPLIT_GRAB_POINTS / 2.0),
            rect.y_range(),
        );
        let handle = ui.interact(grab, ui.id().with("wayback_compare_split"), Sense::drag());
        if handle.hovered() || handle.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if handle.dragged()
            && let Some(pointer) = handle.interact_pointer_pos()
        {
            let split = ((pointer.x - rect.left()) / rect.width())
                .clamp(super::wayback::MIN_SPLIT, 1.0 - super::wayback::MIN_SPLIT);
            if split != compare.split {
                compare.split = split;
                self.wayback.compare = Some(compare);
                // Only the cut between the two finished frames moves: no new raster,
                // and no switch to the lower-resolution frames of a moving camera.
                ui.ctx().request_repaint();
            }
        }
        let x = rect.left() + rect.width() * compare.split;
        let line = Stroke::new(2.0, Color32::WHITE);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            line,
        );
        let knob = Pos2::new(x, rect.center().y);
        painter.circle(
            knob,
            SPLIT_KNOB_RADIUS_POINTS,
            Color32::from_rgba_unmultiplied(20, 28, 40, 220),
            line,
        );
        painter.text(
            knob,
            Align2::CENTER_CENTER,
            "◀▶",
            FontId::proportional(10.0),
            Color32::WHITE,
        );
        // Each side's date, as a drop-down at the top, and a swap button under the knob.
        let top = rect.top() + SPLIT_MENU_MARGIN_POINTS;
        let sides = [
            (
                Rect::from_min_max(
                    Pos2::new(x - SPLIT_MENU_WIDTH_POINTS - SPLIT_MENU_GAP_POINTS, top),
                    Pos2::new(x - SPLIT_MENU_GAP_POINTS, top + SPLIT_MENU_HEIGHT_POINTS),
                ),
                egui::Layout::right_to_left(egui::Align::Center),
                "split_left_date",
                self.wayback.index,
                "left_date",
            ),
            (
                Rect::from_min_max(
                    Pos2::new(x + SPLIT_MENU_GAP_POINTS, top),
                    Pos2::new(
                        x + SPLIT_MENU_GAP_POINTS + SPLIT_MENU_WIDTH_POINTS,
                        top + SPLIT_MENU_HEIGHT_POINTS,
                    ),
                ),
                egui::Layout::left_to_right(egui::Align::Center),
                "split_right_date",
                compare.index,
                "date",
            ),
        ];
        for (area, layout, id, current, param) in sides {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(layout));
            if let Some(index) = self.wayback_date_menu(&mut child, id, current) {
                let date = self.wayback.releases[index].date.clone();
                self.run_reporting("layer.waybackCompare", serde_json::json!({ param: date }));
            }
        }
        let swap = Rect::from_center_size(
            knob + Vec2::new(0.0, SPLIT_KNOB_RADIUS_POINTS + SPLIT_SWAP_SIZE_POINTS),
            Vec2::splat(SPLIT_SWAP_SIZE_POINTS),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(swap));
        if child
            .add(egui::Button::new("⇄").small())
            .on_hover_text(self.lang.text("wayback-compare-swap-tooltip"))
            .clicked()
        {
            self.run_reporting("layer.waybackCompare", serde_json::json!({ "swap": true }));
        }
    }
}

/// The credit for the border tiles' sources (names, so never translated).
pub(super) const OSM_TILES_CREDIT: &str = "OpenFreeMap © OpenMapTiles Data from OpenStreetMap";
