use super::*;
use eframe::egui::{Color32, LayerId, Pos2, pos2, vec2};
use globecraft_core::camera::Cam;
use globecraft_data::boundaries::{LabelKind, MapLabel};
use std::sync::Arc;

fn scene() -> (OverlayView, OverlayStyle, BoundaryData) {
    let cam = Cam {
        lat: 0.0,
        lon: 0.0,
        heading: 0.0,
        alt: 1000.0,
    };
    let view = OverlayView {
        basis: cam.basis(4.0 / 3.0),
        rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
        altitude_km: cam.alt,
        state_max_km: globecraft_data::settings::STATE_MAX_ALTITUDE_DEFAULT_KM,
        grid_step_deg: None,
        pixels_per_point: 1.0,
    };
    let style = OverlayStyle {
        layers: OverlayLayers {
            countries: true,
            states: true,
            cities: false,
            labels: true,
        },
        map_scale: 1.0,
        lang: Lang::En,
        arrival: ArrivalFade::SETTLED,
    };
    let mut data = BoundaryData::default();
    data.countries = vec![Arc::new(vec![(0.0, 0.0), (1.0, 1.0)])];
    data.labels = vec![label("Country", LabelKind::Country, 0)];
    (view, style, data)
}

fn label(text: &str, kind: LabelKind, rank: u8) -> MapLabel {
    MapLabel {
        lat: 0.0,
        lon: 0.0,
        en: text.into(),
        pt: format!("PT {text}"),
        kind,
        rank,
        big: true,
    }
}

fn texts(shapes: &[Shape]) -> Vec<&str> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Text(text) if text.fallback_color != Color32::from_black_alpha(180) => {
                Some(text.galley.job.text.as_str())
            }
            _ => None,
        })
        .collect()
}

fn headless(test: impl FnOnce(&egui::Painter)) {
    let ctx = egui::Context::default();
    ctx.begin_pass(egui::RawInput::default());
    test(&ctx.layer_painter(LayerId::background()));
    let mut output = ctx.end_pass();
    output.textures_delta.clear();
}

#[test]
fn grid_numbers_toggle_while_stationary_and_fit_without_overlapping() {
    headless(|painter| {
        let (mut view, mut style, data) = scene();
        let data = Arc::new(data);
        let mut cache = OverlayCache::new(painter.ctx().clone());
        for lang in Lang::ALL {
            style.lang = lang;
            for scale in [0.6, 1.0, 3.0] {
                style.map_scale = scale;
                view.grid_step_deg = Some(2.0);
                draw_ready(&mut cache, painter, view, &data, style);
                assert!(texts(&cache.shapes).iter().any(|text| text.contains('°')));
                let mut areas = Vec::new();
                for shape in &cache.shapes {
                    if let Shape::Rect(rect) = shape {
                        assert!(view.rect.contains_rect(rect.rect));
                        assert!(!areas.iter().any(|area: &Rect| area.intersects(rect.rect)));
                        areas.push(rect.rect);
                    }
                }
                view.grid_step_deg = None;
                draw_ready(&mut cache, painter, view, &data, style);
                assert!(!texts(&cache.shapes).iter().any(|text| text.contains('°')));
            }
        }
    });
}

#[test]
fn boundary_merge_holds_the_previous_shapes_until_replacement_arrives() {
    headless(|painter| {
        let (view, style, data) = scene();
        let mut data = Arc::new(data);
        let mut cache = OverlayCache::new(painter.ctx().clone());
        draw_ready(&mut cache, painter, view, &data, style);
        let before = cache.shapes.clone();
        Arc::make_mut(&mut data).countries = vec![Arc::new(vec![(0.0, 0.0), (2.0, 2.0)])];
        cache.merge_boundaries();
        // The draw immediately after a merge cannot have replacement geometry
        // yet: the previous dataset must keep painting instead of leaving a
        // blank frame between the patch and the worker result.
        cache.draw(painter, view, &data, style);
        assert!(cache.key.is_none());
        assert_eq!(cache.stale, Some(DrawKey { view, style }));
        assert_eq!(cache.shapes, before);
        // When the worker finishes, the swap is atomic and content changes.
        draw_ready(&mut cache, painter, view, &data, style);
        assert_ne!(cache.shapes, before);
        assert_eq!(cache.stale, Some(DrawKey { view, style }));
    });
}

#[test]
fn unchanged_view_reuses_shapes_and_boundary_invalidation_rebuilds() {
    headless(|painter| {
        let (view, style, data) = scene();
        let mut data = Arc::new(data);
        let mut cache = OverlayCache::new(painter.ctx().clone());
        draw_ready(&mut cache, painter, view, &data, style);
        let before = cache.shapes.clone();
        Arc::make_mut(&mut data).labels[0].en = "Replacement".into();
        // A data patch explicitly invalidates. Until then unchanged keys reuse
        // exactly the same projected shapes, including their shared text galleys.
        draw_ready(&mut cache, painter, view, &data, style);
        assert_eq!(cache.shapes, before);
        cache.invalidate();
        draw_ready(&mut cache, painter, view, &data, style);
        assert_eq!(texts(&cache.shapes), ["Replacement"]);
        assert_ne!(cache.shapes, before);
    });
}

#[test]
fn camera_viewport_altitude_density_layers_language_and_map_scale_invalidate() {
    headless(|painter| {
        let (view, style, data) = scene();
        let mut data = Arc::new(data);
        let variants = [
            (
                OverlayView {
                    basis: Cam {
                        lat: 0.0,
                        lon: 0.0,
                        heading: 30.0,
                        alt: 1000.0,
                    }
                    .basis(4.0 / 3.0),
                    ..view
                },
                style,
            ),
            (
                OverlayView {
                    rect: view.rect.translate(vec2(25.0, 0.0)),
                    ..view
                },
                style,
            ),
            (
                OverlayView {
                    altitude_km: 2000.0,
                    ..view
                },
                style,
            ),
            (
                OverlayView {
                    pixels_per_point: 2.0,
                    ..view
                },
                style,
            ),
            (
                view,
                OverlayStyle {
                    layers: OverlayLayers {
                        states: false,
                        ..style.layers
                    },
                    ..style
                },
            ),
            (
                view,
                OverlayStyle {
                    lang: Lang::PtBr,
                    ..style
                },
            ),
            (
                view,
                OverlayStyle {
                    map_scale: 1.5,
                    ..style
                },
            ),
        ];
        for (next_view, next_style) in variants {
            let mut cache = OverlayCache::new(painter.ctx().clone());
            Arc::make_mut(&mut data).labels[0].en = "Before".into();
            Arc::make_mut(&mut data).labels[0].pt = "Antes".into();
            draw_ready(&mut cache, painter, view, &data, style);
            Arc::make_mut(&mut data).labels[0].en = "After".into();
            Arc::make_mut(&mut data).labels[0].pt = "Depois".into();
            draw_ready(&mut cache, painter, next_view, &data, next_style);
            assert_eq!(
                texts(&cache.shapes),
                [if next_style.lang == Lang::PtBr {
                    "Depois"
                } else {
                    "After"
                }]
            );
        }
    });
}

#[test]
fn projected_shapes_preserve_priority_stable_ties_and_border_endpoints() {
    headless(|painter| {
        let (view, style, mut data) = scene();
        data.labels = vec![
            label("City", LabelKind::City, 0),
            label("State", LabelKind::State, 0),
            label("First country", LabelKind::Country, 1),
            label("Second country", LabelKind::Country, 1),
        ];
        let shapes = projected::build(painter, view, &data, style);
        assert_eq!(texts(&shapes), ["First country"]);
        match &shapes[0] {
            Shape::LineSegment { points, stroke } => {
                assert_eq!(points[0], pos2(400.0, 300.0));
                assert_eq!(
                    points[1],
                    view.basis
                        .project(globecraft_core::math::ll_to_v3(1.0, 1.0), view.rect)
                        .unwrap()
                );
                assert_eq!(stroke.width, 1.6);
                assert_eq!(
                    stroke.color,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 245)
                );
            }
            _ => panic!("country border must precede labels"),
        }
        let hidden = OverlayStyle {
            layers: OverlayLayers {
                countries: false,
                labels: false,
                ..style.layers
            },
            ..style
        };
        assert!(projected::build(painter, view, &data, hidden).is_empty());
        data.labels = vec![
            label("Later rank", LabelKind::Country, 5),
            label("Earlier rank", LabelKind::Country, 0),
        ];
        assert_eq!(
            texts(&projected::build(painter, view, &data, style)),
            ["Earlier rank"]
        );
        data.labels = vec![
            label("City", LabelKind::City, 0),
            label("State", LabelKind::State, 0),
        ];
        assert_eq!(
            texts(&projected::build(painter, view, &data, style)),
            ["State"]
        );
        let states_hidden = OverlayStyle {
            layers: OverlayLayers {
                states: false,
                ..style.layers
            },
            ..style
        };
        assert_eq!(
            texts(&projected::build(painter, view, &data, states_hidden)),
            ["City"]
        );
    });
}

#[test]
fn municipal_outlines_match_the_state_visual_for_a_smooth_handoff() {
    headless(|painter| {
        let (mut view, mut style, mut data) = scene();
        view.altitude_km = 100.0;
        data.countries.clear();
        data.labels.clear();
        let line = Arc::new(vec![(0.0, -1.2), (0.0, 1.2)]);
        data.states = vec![line.clone()];
        data.world_cities = vec![line.clone()];
        data.municipal_detail_max_km =
            globecraft_data::boundaries::MUNICIPAL_VISIBLE_MAX_ALTITUDE_KM;
        let strokes = |shapes: &[Shape]| -> Vec<(f32, [u8; 4])> {
            shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::LineSegment { stroke, .. } => {
                        Some((stroke.width, stroke.color.to_array()))
                    }
                    _ => None,
                })
                .collect()
        };

        style.layers.cities = false;
        let state_style = strokes(&projected::build(painter, view, &data, style));

        // Only the municipal layer draws, over the same line geometry.
        data.states.clear();
        style.layers.cities = true;
        let city_style = strokes(&projected::build(painter, view, &data, style));

        assert!(state_style.len() > 1, "state borders must be dashed");
        assert_eq!(
            state_style, city_style,
            "municipal outlines must use the same color, width and dash as states"
        );
        // Municipal state borders, which take over first, look the same too.
        data.world_cities = vec![];
        data.world_cities_major = vec![line];
        let major_style = strokes(&projected::build(painter, view, &data, style));
        assert_eq!(state_style, major_style);
    });
}

#[test]
fn label_limit_remains_five_hundred() {
    headless(|painter| {
        let (mut view, style, mut data) = scene();
        view.rect = Rect::from_min_size(Pos2::ZERO, vec2(50_000.0, 50_000.0));
        view.altitude_km = 14_000.0;
        view.basis = Cam {
            lat: 0.0,
            lon: 0.0,
            heading: 0.0,
            alt: view.altitude_km,
        }
        .basis(1.0);
        data.labels = (0..625)
            .map(|i| MapLabel {
                lat: -30.0 + (i / 25) as f64 * 2.4,
                lon: -30.0 + (i % 25) as f64 * 2.4,
                ..label("Z", LabelKind::Country, 0)
            })
            .collect();
        assert_eq!(
            texts(&projected::build(painter, view, &data, style)).len(),
            500
        );
    });
}

fn draw_ready(
    cache: &mut OverlayCache,
    painter: &egui::Painter,
    view: OverlayView,
    data: &Arc<BoundaryData>,
    style: OverlayStyle,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cache.draw(painter, view, data, style);
        if cache.key == Some(DrawKey { view, style }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "overlay preparation completes"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn worker_meshes_tinted_on_the_ui_match_the_triangles_egui_would_draw() {
    use super::super::meshes::{BorderMeshes, MeshSettings};
    use globecraft_render::overlays::BorderKind;
    headless(|painter| {
        let ctx = painter.ctx();
        let (view, _, data) = scene();
        let clip = view.rect;
        let segments = [
            (BorderKind::State, [pos2(10.0, 10.0), pos2(200.0, 40.0)]),
            (BorderKind::State, [pos2(200.0, 40.0), pos2(200.0, 300.0)]),
            // Entirely outside the view: culled in both paths.
            (BorderKind::State, [pos2(-50.0, -50.0), pos2(-20.0, -40.0)]),
            (BorderKind::Country, [pos2(30.0, 500.0), pos2(700.0, 520.0)]),
        ];
        let geometry = globecraft_render::overlays::OverlayGeometry {
            view: globecraft_render::overlays::OverlayView {
                basis: view.basis,
                rect: clip,
                altitude_km: view.altitude_km,
                state_visible_max_km: view.state_max_km,
                grid_step_deg: None,
                layers: OverlayLayers {
                    countries: true,
                    states: true,
                    cities: false,
                    labels: false,
                },
            },
            data: Arc::new(data),
            borders: segments.to_vec(),
            labels: Vec::new(),
            grid_labels: Vec::new(),
        };
        // Built off the UI thread, exactly as the workers do it.
        let settings = MeshSettings::of(ctx);
        let meshes = std::thread::scope(|scope| {
            scope
                .spawn(|| BorderMeshes::tessellate(&geometry, settings))
                .join()
                .unwrap()
        });
        let state = Color32::from_rgba_unmultiplied(150, 205, 240, 120);
        let country = Color32::from_rgba_unmultiplied(255, 255, 255, 245);
        let mut colors = [Color32::TRANSPARENT; 5];
        colors[BorderKind::State as usize] = state;
        colors[BorderKind::Country as usize] = country;
        let expected = |colors: [Color32; 5]| -> Vec<_> {
            // egui draws the kinds in declaration order; `project_borders` emits them so.
            let mut shapes: Vec<_> = segments
                .iter()
                .filter(|(kind, _)| colors[*kind as usize].a() > 0)
                .collect();
            shapes.sort_by_key(|(kind, _)| *kind as usize);
            ctx.tessellate(
                shapes
                    .into_iter()
                    .map(|(kind, points)| egui::epaint::ClippedShape {
                        clip_rect: clip,
                        shape: Shape::line_segment(
                            *points,
                            egui::Stroke::new(
                                if *kind as usize == BorderKind::Country as usize {
                                    1.6
                                } else {
                                    1.0
                                },
                                colors[*kind as usize],
                            ),
                        ),
                    })
                    .collect(),
                ctx.pixels_per_point(),
            )
            .iter()
            .filter_map(|primitive| match &primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => Some(mesh.vertices.clone()),
                egui::epaint::Primitive::Callback(_) => None,
            })
            .flatten()
            .collect()
        };
        let mesh = meshes.tinted(colors);
        assert_eq!(mesh.vertices, expected(colors));
        assert!(mesh.is_valid());
        // A fully faded kind adds no triangles at all.
        colors[BorderKind::State as usize] = Color32::TRANSPARENT;
        assert_eq!(meshes.tinted(colors).vertices, expected(colors));
        colors = [Color32::TRANSPARENT; 5];
        assert!(meshes.tinted(colors).is_empty());
    });
}
