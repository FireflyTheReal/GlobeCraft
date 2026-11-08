use super::*;

#[test]
fn held_navigation_retains_pointer_until_release() {
    for move_pointer in [false, true] {
        let ctx = egui::Context::default();
        let button_rect = Rect::from_center_size(Pos2::new(100.0, 100.0), Vec2::splat(15.0));
        let frame = |time, events| {
            let mut held = false;
            let _ = crate::app::test_ui::run(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        // Navigation buttons overlap the draggable globe and rotation ring.
                        let globe =
                            ui.interact(ui.max_rect(), ui.id().with("globe"), Sense::drag());
                        let ring = ui.interact(
                            button_rect.expand(30.0),
                            ui.id().with("ring"),
                            Sense::drag(),
                        );
                        let control = hold_control(ui, button_rect, ui.id().with("control"));
                        held = control.is_pointer_button_down_on();
                        assert!(!globe.dragged());
                        assert!(!ring.dragged());
                    });
                },
            );
            held
        };
        let pointer_button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let start = button_rect.center();
        assert!(!frame(0.0, vec![egui::Event::PointerMoved(start)]));
        assert!(frame(0.1, vec![pointer_button(start, true)]));
        let mut pos = start;
        for tick in 1..=100 {
            let events = if move_pointer {
                // Also retain ownership when pointer motion exceeds the click threshold.
                pos = start + Vec2::new(20.0, 0.0);
                vec![egui::Event::PointerMoved(pos)]
            } else {
                vec![]
            };
            assert!(frame(0.1 + tick as f64 * 0.1, events), "hold tick {tick}");
        }
        assert!(!frame(10.2, vec![pointer_button(pos, false)]));
        assert!(!frame(10.3, vec![]));
    }
}

#[test]
fn cardinal_snaps_take_the_short_way_round() {
    assert_eq!(nearest_heading(350.0, 0.0), 360.0);
    assert_eq!(nearest_heading(10.0, 270.0), -90.0);
    assert_eq!(nearest_heading(100.0, 90.0), 90.0);
    assert_eq!(nearest_heading(-170.0, 180.0), -180.0);
}

#[test]
fn the_view_disc_grows_with_altitude_up_to_the_visible_hemisphere() {
    let close = view_cap_fraction(10.0, 1.6);
    let mid = view_cap_fraction(2_000.0, 1.6);
    let far = view_cap_fraction(40_000.0, 1.6);
    assert!(close < 0.01, "{close}");
    assert!(close < mid && mid < far, "{close} {mid} {far}");
    // Far out the whole visible side is in view: the horizon's cap.
    let radius = globecraft_core::EARTH_RADIUS_KM;
    let horizon = (radius / (radius + 40_000.0)).acos().sin();
    assert!((far - horizon).abs() < 1e-9);
    // A portrait view is limited by its width.
    assert!(view_cap_fraction(2_000.0, 0.5) < mid);
}

/// The view footprint on a mini globe of radius 60 at (100, 100).
fn footprint(cam: &globecraft_core::camera::Cam, rect: Rect) -> Vec<Pos2> {
    let globe = MiniGlobe::new(Pos2::new(100.0, 100.0), 60.0, cam.lat, cam.lon, cam.heading);
    globe.view_outline(&cam.basis(f64::from(rect.width() / rect.height())), rect)
}

/// The polygon's area by the shoelace formula (positive either way round).
fn area(outline: &[Pos2]) -> f32 {
    let twice: f32 = (0..outline.len())
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

#[test]
fn the_footprint_is_convex_and_inside_the_mini_globe_at_every_tilt() {
    let centre = Pos2::new(100.0, 100.0);
    for (width, height) in [(900.0, 700.0), (1600.0, 900.0), (600.0, 900.0)] {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
        for alt in [0.3, 5.0, 120.0, 900.0, 4_000.0, 12_000.0, 40_000.0] {
            for tilt in [
                0.0,
                0.5,
                1.5,
                15.0,
                35.0,
                55.0,
                70.0,
                globecraft_core::camera::MAX_TILT_DEG,
            ] {
                for (lat, heading) in [(46.5, 20.0), (-75.0, 200.0), (89.0, 0.0)] {
                    let cam = globecraft_core::camera::Cam {
                        lat,
                        lon: 8.0,
                        heading,
                        alt,
                        tilt,
                    };
                    let outline = footprint(&cam, rect);
                    let case = format!("{width}x{height} alt {alt} tilt {tilt} lat {lat}");
                    // Close in the footprint is too small to keep its shape (it is
                    // drawn as a dot).
                    assert!(outline.len() >= 3 || alt < 1.0, "{case}");
                    let mut sign = 0.0_f32;
                    for i in 0..outline.len() {
                        let a = outline[i];
                        let b = outline[(i + 1) % outline.len()];
                        let c = outline[(i + 2) % outline.len()];
                        assert!((a - centre).length() <= 60.0 + 0.01, "{case}: {a:?}");
                        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
                        if cross.abs() > 1e-3 {
                            assert!(sign * cross >= 0.0, "{case}: not convex at {b:?}");
                            sign = cross.signum();
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_footprint_grows_smoothly_as_the_view_tilts() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0));
    for alt in [50.0, 900.0, 6_000.0] {
        let mut previous: Option<f32> = None;
        // Past the old switch at 1° from the cap disc to the polygon, in small steps.
        for step in 0..=8 {
            let cam = globecraft_core::camera::Cam {
                lat: 30.0,
                lon: 100.0,
                heading: 10.0,
                alt,
                tilt: f64::from(step) * 0.5,
            };
            let now = area(&footprint(&cam, rect));
            if let Some(before) = previous {
                assert!(
                    (now - before).abs() <= before * 0.1,
                    "alt {alt} tilt {}: {before} -> {now}",
                    f64::from(step) * 0.5
                );
            }
            previous = Some(now);
        }
    }
}

#[test]
fn a_straight_down_footprint_is_the_view_rectangle_on_the_ground() {
    let centre = Pos2::new(100.0, 100.0);
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0));
    let aspect = f64::from(rect.width() / rect.height());
    let cam = globecraft_core::camera::Cam {
        lat: 10.0,
        lon: 20.0,
        heading: 0.0,
        alt: 900.0,
        tilt: 0.0,
    };
    let outline = footprint(&cam, rect);
    let half = |axis: Vec2| {
        outline
            .iter()
            .map(|p| (*p - centre).dot(axis))
            .fold(0.0_f32, f32::max)
    };
    let (up, right) = (half(-Vec2::Y), half(Vec2::X));
    // The short side reaches the cap the drag uses; the long side is wider.
    let cap = 60.0 * view_cap_fraction(cam.alt, aspect) as f32;
    assert!((up - cap).abs() < cap * 0.05, "{up} {cap}");
    assert!(right > up * 1.4, "{right} {up}");
}

#[test]
fn a_tilted_view_shows_a_forward_footprint() {
    let centre = Pos2::new(100.0, 100.0);
    let radius = 60.0;
    let (lat, lon, heading) = (46.5, 8.0, 20.0);
    let globe = MiniGlobe::new(centre, radius, lat, lon, heading);
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
    let aspect = f64::from(rect.width() / rect.height());
    let cam = globecraft_core::camera::Cam {
        lat,
        lon,
        heading,
        alt: 900.0,
        tilt: 55.0,
    };
    let outline = globe.view_outline(&cam.basis(aspect), rect);
    assert!(outline.len() >= 3);
    let (north_screen, _) = screen_axes(heading);
    let forward = outline
        .iter()
        .map(|p| (*p - centre).dot(north_screen))
        .fold(f32::MIN, f32::max);
    let backward = outline
        .iter()
        .map(|p| (*p - centre).dot(north_screen))
        .fold(f32::MAX, f32::min);
    let cap = radius * view_cap_fraction(cam.alt, aspect) as f32;
    // A tilted view reaches much further forward than the top-down cap while its
    // near edge stays close behind the view centre.
    assert!(forward > cap * 2.0, "forward {forward}, cap {cap}");
    assert!(
        forward > -backward * 2.0,
        "forward {forward}, backward {backward}"
    );
    for p in &outline {
        assert!((*p - centre).length() <= radius + 0.01, "{p:?}");
    }
}

#[test]
fn a_tilted_footprint_follows_the_horizon_not_the_sky() {
    let centre = Pos2::new(100.0, 100.0);
    let radius = 60.0;
    let (lat, lon, heading) = (46.5, 8.0, 20.0);
    let globe = MiniGlobe::new(centre, radius, lat, lon, heading);
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
    let aspect = f64::from(rect.width() / rect.height());
    let cam = globecraft_core::camera::Cam {
        lat,
        lon,
        heading,
        alt: 60.0,
        tilt: 75.0,
    };
    let basis = cam.basis(aspect);
    let top = Pos2::new(rect.center().x, rect.top());
    assert!(
        basis.pick(rect, top).is_none(),
        "the top edge looks at the sky"
    );
    let horizon = mini_globe::horizon_point(&basis, rect, top).unwrap();
    let horizon_at = globe.project(horizon.0, horizon.1).unwrap();
    // The boundary is the camera's horizon circle, well inside the mini globe.
    assert!(
        (horizon_at - centre).length() < radius * 0.25,
        "{horizon_at:?}"
    );
    let outline = globe.view_outline(&basis, rect);
    assert!(outline.len() >= 3);
    for p in &outline {
        assert!((*p - centre).length() < radius * 0.25, "{p:?}");
    }
}

#[test]
fn the_mini_globe_centres_the_camera_and_turns_with_the_heading() {
    let centre = Pos2::new(100.0, 100.0);
    let north_up = MiniGlobe::new(centre, 60.0, 48.0, 11.0, 0.0);
    let at = north_up.project(48.0, 11.0).unwrap();
    assert!((at - centre).length() < 1e-3);
    // North of the camera is up on screen, east to the right; the far side is hidden.
    assert!(north_up.project(58.0, 11.0).unwrap().y < centre.y);
    assert!(north_up.project(48.0, 25.0).unwrap().x > centre.x);
    assert!(north_up.project(-48.0, -169.0).is_none());
    // Heading 90°: east is up.
    let east_up = MiniGlobe::new(centre, 60.0, 48.0, 11.0, 90.0);
    let east = east_up.project(48.0, 25.0).unwrap();
    assert!(
        east.y < centre.y && (east.x - centre.x).abs() < 5.0,
        "{east:?}"
    );
}

#[test]
fn the_mini_globe_drags_finer_close_in_and_slower_far_out() {
    let close = globe_drag_deg_per_radius(5.0, 1.6);
    let mid = globe_drag_deg_per_radius(1_000.0, 1.6);
    let far = globe_drag_deg_per_radius(30_000.0, 1.6);
    assert!(close < 0.05, "{close}");
    assert!(close < mid && mid <= far, "{close} {mid} {far}");
    // Far out it is well below the old radian (57°) per radius.
    assert!(far <= MAX_GLOBE_DRAG_DEG_PER_RADIUS);
}
