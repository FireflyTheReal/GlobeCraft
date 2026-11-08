//! The ground's height along the ruler (roadmap 1.10), in the Measure pane: a filled
//! profile with the lowest and highest point and the climbing and descending, and,
//! under the pointer, the distance and height there, marked on the globe too.
use super::App;
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use globecraft_core::{
    body::Body,
    profile::{self, ProfilePoint},
};
use globecraft_data::tiles::Provider;

/// Samples along the whole ruler.
const SAMPLES: usize = 240;
const PLOT_HEIGHT: f32 = 110.0;
/// Room at the plot's left for the height labels.
const AXIS_POINTS: f32 = 44.0;
const FILL: Color32 = Color32::from_rgba_premultiplied(150, 64, 64, 150);
const LINE: Color32 = Color32::from_rgb(255, 110, 110);
const MARKER: Color32 = Color32::from_rgb(255, 230, 120);
/// Tiles of the elevation data are this many samples wide.
const DEM_TILE_SAMPLES: f64 = 256.0;
const EARTH_CIRCUMFERENCE_KM: f64 = 40_075.0;

/// The elevation level whose samples are about as far apart as the profile's, so a
/// long ruler loads a few coarse tiles and a short one the detail it needs.
fn profile_zoom(total_km: f64) -> u8 {
    let spacing_km = (total_km / SAMPLES as f64).max(1e-3);
    let level = (EARTH_CIRCUMFERENCE_KM / (spacing_km * DEM_TILE_SAMPLES)).log2();
    level.clamp(0.0, f64::from(Provider::TerrainDem.max_zoom())) as u8
}

/// The profile now: where it was sampled and the heights loaded so far.
pub(super) struct Profile {
    points: Vec<ProfilePoint>,
    heights: Vec<Option<f32>>,
}

impl Profile {
    /// For the assistant: the summary and every sample with a height (metres, km).
    pub(super) fn to_json(&self) -> serde_json::Value {
        let samples: Vec<_> = self
            .points
            .iter()
            .zip(&self.heights)
            .filter_map(|(point, height)| {
                height.map(|height| {
                    serde_json::json!([
                        (point.distance_km * 1000.0).round() / 1000.0,
                        point.lat,
                        point.lon,
                        height.round(),
                    ])
                })
            })
            .collect();
        let stats = profile::stats(&self.points, &self.heights);
        serde_json::json!({
            "loaded": samples.len(),
            "sampled": self.points.len(),
            "lowest_m": stats.map(|s| s.min_m.round()),
            "highest_m": stats.map(|s| s.max_m.round()),
            "ascent_m": stats.map(|s| s.ascent_m.round()),
            "descent_m": stats.map(|s| s.descent_m.round()),
            "samples": {"columns": ["distance_km", "lat", "lon", "height_m"], "rows": samples},
            "note": "Heights come from the elevation tiles loaded so far; ask again if loaded < sampled.",
        })
    }
}

impl App {
    /// The ruler's profile, requesting the elevation tiles it still needs. `None`
    /// off the Earth or with fewer than two points on a path.
    pub(super) fn ruler_profile(&self) -> Option<Profile> {
        if self.explorer.active != Body::Earth {
            return None;
        }
        let model = self.active_earth_model();
        let paths = self.measure.paths.measured_paths();
        let points = profile::sample(paths.iter().map(Vec::as_slice), model, SAMPLES);
        let total = points.last()?.distance_km;
        let zoom = profile_zoom(total);
        let heights = points
            .iter()
            .map(|point| self.cache.elevation_m(point.lat, point.lon, zoom))
            .collect();
        Some(Profile { points, heights })
    }

    /// The profile's section of the Measure pane.
    pub(in crate::app) fn elevation_profile(&mut self, ui: &mut egui::Ui) {
        self.measure.profile_hover = None;
        let Some(profile) = self.ruler_profile() else {
            return;
        };
        let lg = self.lang;
        let units = self.distance_units();
        super::ui::widgets::section_heading(ui, lg.text("measure-profile-title"));
        let Some(stats) = profile::stats(&profile.points, &profile.heights) else {
            ui.label(lg.text("measure-profile-loading"));
            return;
        };
        let total = profile.points.last().map_or(0.0, |point| point.distance_km);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), PLOT_HEIGHT), Sense::hover());
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();
        painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
        let plot = Rect::from_min_max(
            Pos2::new(rect.left() + AXIS_POINTS, rect.top() + 6.0),
            Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
        );
        // A little room above and below; a flat profile still gets a band.
        let span = (stats.max_m - stats.min_m).max(10.0);
        let (low, high) = (stats.min_m - span * 0.08, stats.max_m + span * 0.08);
        let x =
            |distance_km: f64| plot.left() + (distance_km / total.max(1e-9)) as f32 * plot.width();
        let y = |height_m: f64| {
            plot.bottom() - ((height_m - low) / (high - low)) as f32 * plot.height()
        };
        // Fill and line, broken where a height is missing or a path ends.
        let mut mesh = Mesh::default();
        let mut runs: Vec<Vec<Pos2>> = vec![Vec::new()];
        let mut previous: Option<(usize, Pos2)> = None;
        for (point, height) in profile.points.iter().zip(&profile.heights) {
            let Some(height) = height else {
                previous = None;
                runs.push(Vec::new());
                continue;
            };
            let at = Pos2::new(x(point.distance_km), y(f64::from(*height)));
            match previous {
                Some((path, before)) if path == point.path => {
                    let base = mesh.vertices.len() as u32;
                    for corner in [
                        before,
                        Pos2::new(before.x, plot.bottom()),
                        at,
                        Pos2::new(at.x, plot.bottom()),
                    ] {
                        mesh.colored_vertex(corner, FILL);
                    }
                    mesh.add_triangle(base, base + 1, base + 2);
                    mesh.add_triangle(base + 1, base + 3, base + 2);
                }
                Some(_) => runs.push(Vec::new()),
                None => {}
            }
            runs.last_mut().expect("one run at least").push(at);
            previous = Some((point.path, at));
        }
        painter.add(Shape::mesh(mesh));
        for run in runs.into_iter().filter(|run| run.len() >= 2) {
            painter.add(Shape::line(run, Stroke::new(1.5, LINE)));
        }
        let text = visuals.text_color();
        let font = egui::FontId::proportional(10.5);
        for (height, align) in [
            (stats.max_m, egui::Align2::RIGHT_TOP),
            (stats.min_m, egui::Align2::RIGHT_BOTTOM),
        ] {
            painter.text(
                Pos2::new(plot.left() - 4.0, y(height)),
                align,
                units.elevation(height),
                font.clone(),
                text,
            );
        }
        // Under the pointer: the nearest sample with a height.
        if let Some(pointer) = response.hover_pos()
            && plot.x_range().contains(pointer.x)
        {
            let distance = f64::from((pointer.x - plot.left()) / plot.width()) * total;
            let nearest = profile
                .points
                .iter()
                .zip(&profile.heights)
                .filter_map(|(point, height)| height.map(|height| (point, height)))
                .min_by(|a, b| {
                    (a.0.distance_km - distance)
                        .abs()
                        .total_cmp(&(b.0.distance_km - distance).abs())
                });
            if let Some((point, height)) = nearest {
                let at = Pos2::new(x(point.distance_km), y(f64::from(height)));
                painter.vline(
                    at.x,
                    plot.y_range(),
                    Stroke::new(1.0, text.gamma_multiply(0.5)),
                );
                painter.circle_filled(at, 3.5, MARKER);
                let label = lg.format(
                    "measure-profile-hover",
                    &[
                        ("distance", units.distance(point.distance_km).into()),
                        ("height", units.elevation(f64::from(height)).into()),
                    ],
                );
                let align = if at.x > plot.center().x {
                    egui::Align2::RIGHT_TOP
                } else {
                    egui::Align2::LEFT_TOP
                };
                painter.text(
                    Pos2::new(at.x, plot.top()),
                    align,
                    label,
                    font.clone(),
                    text,
                );
                self.measure.profile_hover = Some((point.lat, point.lon));
            }
        }
        ui.label(
            egui::RichText::new(lg.format(
                "measure-profile-stats",
                &[
                    ("min", units.elevation(stats.min_m).into()),
                    ("max", units.elevation(stats.max_m).into()),
                    ("ascent", units.elevation(stats.ascent_m).into()),
                    ("descent", units.elevation(stats.descent_m).into()),
                ],
            ))
            .size(11.5),
        );
        ui.label(
            egui::RichText::new(Provider::TerrainDem.attribution(lg))
                .size(10.0)
                .weak(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_ruler_reads_coarse_tiles_and_a_short_one_fine() {
        assert!(profile_zoom(10_000.0) <= 4);
        let fine = profile_zoom(2.0);
        assert_eq!(fine, Provider::TerrainDem.max_zoom().min(fine));
        assert!(fine >= 12 || fine == Provider::TerrainDem.max_zoom());
        assert!(profile_zoom(100.0) > profile_zoom(1_000.0));
    }
}
