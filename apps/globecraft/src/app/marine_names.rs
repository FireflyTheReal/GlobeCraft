//! The names of the oceans, seas, exclusive economic zones and undersea features
//! (`globecraft_data::marine_names`), in italics as on a sea chart: each at the
//! altitudes where it is about the size of the view, largest first, never two
//! overlapping. Undersea features show smaller and greyer, below the surface's names.
use super::App;
use eframe::egui::{self, Color32, Pos2, Rect};
use globecraft_core::{camera::Basis, layers::MapLayer};
use globecraft_data::marine_names::{MarineKind, MarineName};
use globecraft_render::overlays::LabelSpace;

/// A name shows while the place spans between these fractions of the view's height
/// (its ground, `VERTICAL_FOV_HALF_TAN` × 2 × altitude).
const MIN_VIEW_FRACTION: f64 = 0.08;
const MAX_VIEW_FRACTION: f64 = 4.0;
/// Names drawn at most, and the room kept around each.
const MAX_LABELS: usize = 90;
const LABEL_PADDING_POINTS: f32 = 6.0;
const HALO: Color32 = Color32::from_rgba_premultiplied(6, 18, 34, 210);

/// Size and colour of a kind's names, before the map scale.
fn look(kind: MarineKind) -> (f32, Color32) {
    match kind {
        MarineKind::Ocean => (16.0, Color32::from_rgb(170, 210, 255)),
        MarineKind::Sea => (13.0, Color32::from_rgb(160, 205, 250)),
        MarineKind::Eez => (11.0, Color32::from_rgb(235, 225, 150)),
        MarineKind::Undersea => (11.0, Color32::from_rgb(150, 175, 200)),
    }
}

/// Whether a place of this size is named at this altitude.
fn shown_at(name: &MarineName, altitude_km: f64) -> bool {
    let view_km = 2.0 * globecraft_core::VERTICAL_FOV_HALF_TAN * altitude_km;
    let fraction = name.size_km / view_km.max(1e-3);
    // Oceans name the whole globe from far out; nothing caps them.
    (MIN_VIEW_FRACTION..=MAX_VIEW_FRACTION).contains(&fraction)
        || (name.kind == MarineKind::Ocean && fraction > MIN_VIEW_FRACTION)
}

/// A name in italics, upper case for oceans as on charts.
fn layout(painter: &egui::Painter, name: &MarineName, scale: f32) -> std::sync::Arc<egui::Galley> {
    let (size, colour) = look(name.kind);
    let text = if name.kind == MarineKind::Ocean {
        name.name.to_uppercase()
    } else {
        name.name.clone()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text,
        0.0,
        egui::TextFormat {
            font_id: super::fonts::map_font(painter.ctx(), size * scale),
            color: colour,
            italics: true,
            extra_letter_spacing: if name.kind == MarineKind::Ocean {
                2.0 * scale
            } else {
                0.0
            },
            ..Default::default()
        },
    );
    painter.layout_job(job)
}

impl App {
    pub(super) fn update_marine_names(&mut self, ctx: &egui::Context, earth: bool) {
        let wake = ctx.clone();
        self.live_marine_names.update(
            &self.runtime,
            earth && self.layer(MapLayer::SeaNames),
            move || wake.request_repaint(),
        );
    }

    /// The names, where the place, user and road names left room: surface names
    /// first, then the undersea ones. Their rectangles go to `marine_placed`.
    pub(super) fn paint_marine_names(
        &mut self,
        painter: &egui::Painter,
        basis: &Basis,
        rect: Rect,
        room: &LabelSpace,
    ) {
        let names = self
            .live_marine_names
            .names()
            .filter(|_| self.layer(MapLayer::SeaNames));
        let Some(names) = names else {
            self.marine_placed.clear();
            return;
        };
        let scale = self.settings.map_scale;
        let mut taken: Vec<Rect> = Vec::new();
        let mut shapes = Vec::new();
        let order = names
            .names
            .iter()
            .filter(|name| name.kind != MarineKind::Undersea)
            .chain(
                names
                    .names
                    .iter()
                    .filter(|name| name.kind == MarineKind::Undersea),
            );
        for name in order {
            if taken.len() >= MAX_LABELS {
                break;
            }
            if !shown_at(name, self.cam.alt) {
                continue;
            }
            let Some(at) = globecraft_render::overlays::project_unhidden(
                basis, rect, None, None, name.lat, name.lon,
            )
            .map(|p| Pos2::new(p.x as f32, p.y as f32)) else {
                continue;
            };
            if !rect.contains(at) {
                continue;
            }
            let galley = layout(painter, name, scale);
            let bounds =
                Rect::from_center_size(at, galley.size()).expand(LABEL_PADDING_POINTS * scale);
            if !rect.contains_rect(bounds)
                || !room.is_free(bounds)
                || taken.iter().any(|other| other.intersects(bounds))
            {
                continue;
            }
            taken.push(bounds);
            super::antarctica::outlined(
                &mut shapes,
                painter,
                at - galley.size() / 2.0,
                galley,
                HALO,
            );
        }
        painter.extend(shapes);
        self.marine_placed.replace(taken);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(kind: MarineKind, size_km: f64) -> MarineName {
        MarineName {
            name: "x".into(),
            kind,
            lat: 0.0,
            lon: 0.0,
            size_km,
        }
    }

    #[test]
    fn names_show_while_they_are_about_the_size_of_the_view() {
        // A 1,000 km sea: named from about 120 km up to about 6,000 km.
        let sea = named(MarineKind::Sea, 1_000.0);
        assert!(shown_at(&sea, 1_000.0));
        assert!(
            !shown_at(&sea, 50.0),
            "too close: it fills far more than the view"
        );
        assert!(!shown_at(&sea, 20_000.0), "too far: a speck");
        // An ocean stays named however far out.
        assert!(shown_at(&named(MarineKind::Ocean, 15_000.0), 40_000.0));
        // A 20 km seamount only close up.
        let seamount = named(MarineKind::Undersea, 20.0);
        assert!(shown_at(&seamount, 30.0) && !shown_at(&seamount, 500.0));
    }
}
