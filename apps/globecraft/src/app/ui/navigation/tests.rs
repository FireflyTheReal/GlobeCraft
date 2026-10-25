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
