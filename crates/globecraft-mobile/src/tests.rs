use super::*;

fn globe() -> Globe {
    let dir = std::env::temp_dir().join(format!("globecraft-mobile-test-{}", std::process::id()));
    let mut globe = Globe::new(dir).expect("start the globe");
    globe.set_size(400, 300, 2.0);
    globe
}

/// Gestures move the camera the way the desktop's mouse does, in points.
#[test]
fn gestures_move_the_camera() {
    let mut g = globe();
    let start = g.cam;
    g.pinch(2.0, 100.0, 75.0);
    assert!(
        g.cam.alt < start.alt * 0.6,
        "pinching out comes closer: {}",
        g.cam.alt
    );
    let before = g.cam;
    g.pan(50.0, 0.0);
    assert!(
        g.cam.lon < before.lon,
        "dragging right brings the west into view"
    );
    g.rotate(30.0);
    assert!((g.cam.heading - 330.0).abs() < 1e-6 || (g.cam.heading + 30.0).abs() < 1e-6);
    g.tilt(20.0);
    assert!(g.cam.view_tilt() > 0.0);
    // Nonsense from a host is ignored.
    let alt = g.cam.alt;
    g.pinch(f32::NAN, 0.0, 0.0);
    g.pinch(0.0, 0.0, 0.0);
    assert_eq!(g.cam.alt, alt);
}

/// A frame of the requested size arrives without network (tiles missing): the host can
/// show something at once, then sharper frames as tiles land. Motion frames are half size.
#[test]
fn frames_arrive_at_full_and_motion_size() {
    let mut g = globe();
    g.set_relief(false);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !g.tick() {
        assert!(Instant::now() < deadline, "no frame");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!((g.frame().width, g.frame().height), (400, 300));
    assert_eq!(g.frame().pixels.len(), 400 * 300 * 4);
    g.pan(10.0, 0.0);
    loop {
        assert!(Instant::now() < deadline, "no motion frame");
        if g.tick() && g.frame().width == 200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(g.frame().height, 150);
    // Once the gesture settles, a full-size frame follows.
    loop {
        assert!(Instant::now() < deadline, "no full frame after the gesture");
        if g.tick() && g.frame().width == 400 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(g.stats().contains("400×300"), "{}", g.stats());
}
