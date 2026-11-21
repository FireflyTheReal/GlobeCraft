//! The window, panes, menus, settings and icons.
use super::*;

#[test]
#[ignore = "manual: renders the full interface offscreen; see the module documentation"]
fn interface_screenshots() {
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");

    let mut h = harness(ThemePreference::Light);
    settle(&mut h, 6.0);
    save(&mut h, &dir, "01-home-light");
    drag_the_tilt_thumb(&mut h);
    save(&mut h, &dir, "01-tilt-arc-light");
    let _ = h.state_mut().run("camera.topDown", Value::Null);
    let _ = h
        .state_mut()
        .run("ui.navigatorStyle", json!({"style": "classic"}));
    settle(&mut h, 0.5);
    save(&mut h, &dir, "01-classic-navigator-light");
    let _ = h
        .state_mut()
        .run("ui.navigatorStyle", json!({"style": "globe"}));

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Map;
    let _ = app.run("ui.pane.layers", Value::Null);
    let _ = app.run("layer.show", json!({"layer": "grid"}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "02-layers-light");

    // The search narrows the list to matching rows and keeps their groups open.
    let app = h.state_mut();
    app.set_layers_search("parks", false);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-search-light");
    let app = h.state_mut();
    app.set_layers_search("", true);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-active-light");
    let app = h.state_mut();
    app.set_layers_search("no such layer", false);
    settle(&mut h, 0.5);
    save(&mut h, &dir, "02-layers-search-empty-light");
    let app = h.state_mut();
    app.set_layers_search("", false);

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::Live;
    app.settings.workspace.open_right(PaneTab::Measure);
    let _ = app.run("ruler.addPoint", json!({"lat": 38.72, "lon": -9.14}));
    let _ = app.run("ruler.addPoint", json!({"lat": -22.9, "lon": -43.2}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "03-live-measure-light");

    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.settings.workspace.open_right(PaneTab::Assistant);
    let _ = app.run("ui.pane.places", Value::Null);
    let _ = app.run(
        "places.add",
        json!({"lat": 27.988, "lon": 86.925, "name": "Mount Everest"}),
    );
    settle(&mut h, 6.0);
    save(&mut h, &dir, "04-assistant-dark");

    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = Tab::View;
    let _ = app.run("ui.commandSearch", Value::Null);
    settle(&mut h, 1.0);
    save(&mut h, &dir, "05-command-search-dark");

    // A tilted view toward the horizon, then a steeper one closer in.
    h.key_press(egui::Key::Escape);
    settle(&mut h, 0.3);
    let _ = h.state_mut().run(
        "camera.set",
        json!({"lat": 46.5, "lon": 8.0, "altitude_km": 900.0, "heading_deg": 20.0, "tilt_deg": 55.0}),
    );
    settle(&mut h, 2.0);
    save(&mut h, &dir, "06-tilted-dark");
    let _ = h
        .state_mut()
        .run("camera.set", json!({"altitude_km": 60.0, "tilt_deg": 75.0}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "07-tilted-close-dark");
    let _ = h.state_mut().run("ui.tileDiagnostics", json!({"on": true}));
    settle(&mut h, 2.0);
    save(&mut h, &dir, "08-tile-diagnostics-dark");
}

/// The navigator's view footprint across altitudes and tilts (user, 2026-11-08: the
/// tilt representation was buggy): one screenshot per case, `nav-<alt>-<tilt>`.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn navigator_tilt_sweep() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    settle(&mut h, 1.0);
    for alt in [20_000.0, 6_000.0, 900.0, 60.0, 2.0] {
        for tilt in [
            0.0,
            1.0,
            20.0,
            45.0,
            65.0,
            globecraft_core::camera::MAX_TILT_DEG,
        ] {
            let _ = h.state_mut().run(
                "camera.set",
                json!({"lat": 30.0, "lon": 105.0, "altitude_km": alt, "heading_deg": 15.0, "tilt_deg": tilt}),
            );
            settle(&mut h, 0.2);
            save(&mut h, &dir, &format!("nav-{alt}-{tilt}"));
        }
    }
    // The arc's sky end, and how far it reaches where the view levels out.
    for (alt, lean) in [(2.0, 150.0), (10_000.0, 175.0)] {
        let _ = h.state_mut().run(
            "camera.set",
            json!({"lat": 30.0, "lon": 105.0, "altitude_km": alt, "heading_deg": 15.0, "tilt_deg": 0.0}),
        );
        h.state_mut().cam.tilt_view(lean);
        settle(&mut h, 0.2);
        save(&mut h, &dir, &format!("nav-lean-{alt}-{lean}"));
    }
}

/// Settings → Network at the window's minimum size, in both languages, with a
/// remote user layer and an assistant provider so the dynamic groups are shown.
/// Manual because it renders offscreen with the project's screenshot renderer.
#[test]
#[ignore = "manual: renders the Settings Network page offscreen at minimum size"]
fn network_settings_screenshots() {
    use egui_kittest::kittest::Queryable;
    use globecraft_data::user_data::{FeatureFormat, UserLayer, UserSource};

    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let mut settings = Settings::default();
    settings.appearance.theme = ThemePreference::Light;
    settings.user_layers.push(UserLayer {
        id: 9_999,
        name: "City feed".into(),
        source: UserSource::Features {
            location: "https://data.example.org/cities.geojson".into(),
            format: FeatureFormat::GeoJson,
            refresh_minutes: 15,
            value_field: None,
        },
        visible: true,
        opacity: 1.0,
        attribution: "Example data".into(),
        color: "#ffb000".into(),
        preset: None,
    });
    settings.assistant.provider = "ollama".into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .with_pixels_per_point(1.5)
        .with_step_dt(0.1)
        .renderer(WgpuTestRenderer::new())
        .build_eframe(move |cc| App::new(cc, settings, Arc::new(RestartRequest::default())));
    settle(&mut h, 2.0);
    let _ = h.state_mut().run("ui.settings", Value::Null);
    settle(&mut h, 0.5);

    // Drag the bottom-right corner to the Settings window's 600×380 point minimum.
    let before = settings_window_rect(&h);
    let start = before.max - egui::vec2(4.0, 4.0);
    let delta = egui::vec2(600.0, 380.0) - before.size();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(start));
    h.step();
    h.event(button(start, true));
    h.step();
    for step in 1..=5 {
        h.event(egui::Event::PointerMoved(
            start + delta * (step as f32 / 5.0),
        ));
        h.step();
    }
    h.event(button(start + delta, false));
    settle(&mut h, 0.5);
    let rect = settings_window_rect(&h);
    println!("settings window after resize: {rect:?}");
    assert!(
        (rect.width() - 600.0).abs() < 3.0 && (rect.height() - 380.0).abs() < 4.0,
        "window did not reach its minimum: {rect:?}"
    );

    h.get_by_label("Data & network").click();
    settle(&mut h, 0.5);
    assert!(
        h.query_by_label_contains("server.arcgisonline.com")
            .is_some(),
        "no basemap host row"
    );
    save(&mut h, &dir, "network-en-top");
    scroll_settings(&mut h, rect.center(), true, 4);
    save(&mut h, &dir, "network-en-middle");
    scroll_settings(&mut h, rect.center(), true, 6);
    save(&mut h, &dir, "network-en-bottom");
    scroll_settings(&mut h, rect.center(), false, 10);
    // The keys section (adapter D), lower on the same page: one field per keyed service.
    settle(&mut h, 0.5);
    assert!(
        h.query_by_label_contains("TomTom").is_some(),
        "no TomTom key row"
    );
    save(&mut h, &dir, "keys-en");

    let _ = h
        .state_mut()
        .run("settings.language", json!({"language": "pt-BR"}));
    settle(&mut h, 0.5);
    save(&mut h, &dir, "network-pt-top");
    scroll_settings(&mut h, rect.center(), true, 4);
    save(&mut h, &dir, "network-pt-middle");
    scroll_settings(&mut h, rect.center(), true, 6);
    save(&mut h, &dir, "network-pt-bottom");
    assert!(
        h.query_by_label_contains("data.example.org").is_some(),
        "no user-layer host row"
    );
    assert!(
        h.query_by_label_contains("localhost:11434").is_some(),
        "no assistant host row"
    );
    // The language choice is saved for the next start: leave it as it was.
    let _ = h
        .state_mut()
        .run("settings.language", json!({"language": "en"}));
    settle(&mut h, 0.5);
}

/// The About window with the logo, light and dark (`about-<theme>`).
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn about_window() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, name) in [
        (ThemePreference::Light, "about-light"),
        (ThemePreference::Dark, "about-dark"),
    ] {
        let mut h = harness(theme);
        settle(&mut h, 0.5);
        h.state_mut().run("ui.about", json!({})).expect("ui.about");
        settle(&mut h, 0.3);
        save(&mut h, &dir, name);
    }
}

/// Every ribbon tab in Advanced mode, every Settings page and the left panes: the
/// inventory for the layout redesign (`ribbon-tab-<code>`, `settings-<n>`).
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn layout_inventory_screenshots() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Light);
    settle(&mut h, 0.5);
    h.state_mut()
        .run("settings.experience", json!({"level": "advanced"}))
        .expect("advanced");
    // GLOBECRAFT_SHOT_LANG=pt-BR checks that the tabs fit in Portuguese too.
    let language = std::env::var("GLOBECRAFT_SHOT_LANG").ok();
    if let Some(code) = &language {
        h.state_mut()
            .run("settings.language", json!({"language": code}))
            .expect("language");
    }
    for tab in Tab::RIBBON {
        h.state_mut().settings.workspace.ribbon_tab = tab;
        settle(&mut h, 0.3);
        save(&mut h, &dir, &format!("ribbon-tab-{}", tab.code()));
    }
    // The status bar's menus: the coordinate format and the units (right-click).
    {
        use egui_kittest::kittest::Queryable;
        h.get_by_label_contains("Center:").click_secondary();
        settle(&mut h, 0.3);
        save(&mut h, &dir, "status-coordinate-format");
        h.key_press(egui::Key::Escape);
        settle(&mut h, 0.2);
        h.get_by_label_contains("Camera altitude").click_secondary();
        settle(&mut h, 0.3);
        save(&mut h, &dir, "status-units");
        h.key_press(egui::Key::Escape);
        settle(&mut h, 0.2);
    }
    // Search results as a finished search leaves them, listed at the top of Places.
    h.state_mut().search.results = ["Lisbon, Portugal", "Lisbon, Maine, United States"]
        .iter()
        .map(|name| globecraft_data::search::GeoResult {
            name: name.to_string(),
            lat: 38.7,
            lon: -9.1,
            alt: 30.0,
        })
        .collect();
    for pane in ["ui.pane.layers", "ui.pane.places"] {
        h.state_mut().run(pane, json!({})).expect(pane);
        settle(&mut h, 0.5);
        save(&mut h, &dir, &format!("pane-{pane}"));
    }
    h.state_mut()
        .run("ui.settings", json!({}))
        .expect("settings");
    settle(&mut h, 0.5);
    save(&mut h, &dir, "settings-window");
    {
        use egui_kittest::kittest::Queryable;
        let pages = [
            "Appearance",
            "Map style",
            "Navigation",
            "Globe & graphics",
            "Data & network",
            "Files & shortcuts",
        ];
        if language.is_none() {
            for (n, page) in pages.iter().enumerate() {
                h.get_by_label(page).click();
                settle(&mut h, 0.3);
                save(&mut h, &dir, &format!("settings-page-{}", n + 2));
            }
        }
    }
    if language.is_some() {
        // The language is saved: put English back for the next tests.
        h.state_mut()
            .run("settings.language", json!({"language": "en"}))
            .expect("language");
        settle(&mut h, 0.3);
    }
}

/// Every dialog, card and pane that replaced a floating window (layout redesign
/// step 4), in light and dark.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn surfaces_screenshots() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        let mut h = harness(theme);
        settle(&mut h, 0.5);
        let steps: [(&str, Value, &str); 6] = [
            ("ui.about", json!({}), "about"),
            ("ui.shortcuts", json!({}), "shortcuts"),
            ("ui.addData", json!({}), "add-data"),
            ("ui.here", json!({"lat": 38.72, "lon": -9.14}), "here-card"),
            ("ui.explorer", json!({}), "explorer-pane"),
            ("ruler.editPoints", json!({}), "ruler-points"),
        ];
        for (id, params, shot) in steps {
            h.state_mut().run(id, params).expect(id);
            settle(&mut h, 0.5);
            save(&mut h, &dir, &format!("surface-{shot}-{name}"));
            h.key_press(egui::Key::Escape);
            settle(&mut h, 0.2);
        }
        h.state_mut()
            .run("ui.legend", json!({"on": true}))
            .expect("legend");
        h.state_mut()
            .run("layer.show", json!({"layer": "flights", "on": true}))
            .expect("flights");
        settle(&mut h, 0.5);
        save(&mut h, &dir, &format!("surface-legend-pane-{name}"));
    }
}

/// The keyboard: F6 reaches the ribbon's tabs, the arrows move between them, Enter
/// opens one, F6 goes on to the side pane and the globe, and the focused control has a
/// ring (layout redesign step 5).
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn keyboard_navigation_screenshot() {
    use super::super::ui::focus::{pane_tab_id, ribbon_tab_id};
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Light);
    settle(&mut h, 0.5);
    let focused = |h: &Harness<'static, App>| h.ctx.memory(|m| m.focused());
    h.key_press(egui::Key::F6);
    settle(&mut h, 0.2);
    assert_eq!(
        focused(&h),
        Some(ribbon_tab_id(Tab::Explore)),
        "F6 reaches the ribbon"
    );
    h.key_press(egui::Key::ArrowRight);
    settle(&mut h, 0.2);
    assert_eq!(
        focused(&h),
        Some(ribbon_tab_id(Tab::Map)),
        "→ moves to the next tab"
    );
    h.key_press(egui::Key::Enter);
    settle(&mut h, 0.3);
    assert_eq!(
        h.state().settings.workspace.ribbon_tab,
        Tab::Map,
        "Enter opens it"
    );
    save(&mut h, &dir, "keyboard-ribbon-tab");
    h.key_press(egui::Key::F6);
    settle(&mut h, 0.2);
    let pane = h
        .state()
        .settings
        .workspace
        .right_pane
        .expect("a tab in view");
    assert_eq!(
        focused(&h),
        Some(pane_tab_id(pane)),
        "F6 goes on to the side pane"
    );
    save(&mut h, &dir, "keyboard-side-pane");
    h.key_press(egui::Key::F6);
    settle(&mut h, 0.2);
    assert_eq!(
        focused(&h),
        None,
        "then the globe, whose arrows move the view"
    );
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F6);
    settle(&mut h, 0.2);
    assert_eq!(focused(&h), Some(pane_tab_id(pane)), "Shift+F6 goes back");
}

/// Side menus in the group drop-downs: Nature ▾ › Exaggeration ▸, My data ▾ ›
/// Ocean ▸ (the sources that were the Data pane), View › Window ▾ › Theme ▸.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn side_menus_screenshot() {
    use egui_kittest::kittest::Queryable;
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Light);
    settle(&mut h, 0.5);
    for (tab, group, side, name) in [
        (Tab::Map, "Nature", "Exaggeration", "side-exaggeration"),
        (Tab::Map, "My data", "Ocean", "side-library-ocean"),
        (Tab::View, "Window", "Theme", "side-theme"),
    ] {
        h.state_mut().settings.workspace.ribbon_tab = tab;
        settle(&mut h, 0.3);
        h.get_by_label(group).click();
        settle(&mut h, 0.3);
        h.get_by_label_contains(side).hover();
        settle(&mut h, 0.5);
        save(&mut h, &dir, name);
        h.key_press(egui::Key::Escape);
        h.key_press(egui::Key::Escape);
        settle(&mut h, 0.3);
    }
}

/// A ribbon group's menu, a tool's ⋮ menu, a tool pinned to the ribbon and to the
/// quick-access row, and the keyboard shortcut dialog.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn ribbon_menus_screenshot() {
    use egui_kittest::kittest::Queryable;
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, name) in [
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ] {
        let mut h = harness(theme);
        settle(&mut h, 0.5);
        save(&mut h, &dir, &format!("ribbon-{name}"));
        h.get_by_label("Navigate").click();
        settle(&mut h, 0.3);
        assert!(
            h.query_by_label("Auto-Rotate").is_some(),
            "the menu is open"
        );
        save(&mut h, &dir, &format!("ribbon-menu-{name}"));
        h.get_all_by_label("⋮").next().expect("a ⋮ button").hover();
        settle(&mut h, 0.2);
        h.get_all_by_label("⋮").next().expect("a ⋮ button").click();
        settle(&mut h, 0.3);
        save(&mut h, &dir, &format!("ribbon-menu-more-{name}"));
        h.key_press(egui::Key::Escape);
        h.key_press(egui::Key::Escape);
        settle(&mut h, 0.2);
        let app = h.state_mut();
        app.run(
            "ui.ribbon.pin",
            json!({"group": "explore.navigate", "tool": "camera.zoomIn"}),
        )
        .expect("pin");
        assert!(app.tool_pinned("explore.navigate", "camera.zoomIn"));
        app.run(
            "ui.ribbon.pin",
            json!({"group": "explore.navigate", "tool": "camera.resetNorth", "on": false}),
        )
        .expect("unpin");
        app.run(
            "ui.quickTools.pin",
            json!({"tool": r#"layer.show {"layer":"radar"}"#}),
        )
        .expect("quick tool");
        app.run(
            "ui.shortcut.set",
            json!({"tool": "ui.legend", "keys": "command+shift+E"}),
        )
        .expect("shortcut");
        // The keys were the screenshot's: it has none now.
        assert_eq!(crate::app::commands::shortcut("file.screenshot"), None);
        assert!(crate::app::commands::shortcut("ui.legend").is_some());
        settle(&mut h, 0.3);
        save(&mut h, &dir, &format!("ribbon-pinned-{name}"));
        h.state_mut()
            .run("ui.shortcut.edit", json!({"tool": "ui.legend"}))
            .expect("edit");
        settle(&mut h, 0.3);
        save(&mut h, &dir, &format!("shortcut-dialog-{name}"));
        h.event(egui::Event::Key {
            key: egui::Key::L,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        });
        settle(&mut h, 0.2);
        let app = h.state_mut();
        assert!(
            app.chrome.ribbon_tools.capture.is_none(),
            "the dialog closed"
        );
        assert_eq!(
            app.settings.workspace.shortcuts["ui.legend"],
            "command+alt+L"
        );
        app.run("ui.ribbon.resetTools", json!({})).expect("reset");
        assert!(crate::app::commands::shortcut("file.screenshot").is_some());
        assert!(!app.tool_pinned("explore.navigate", "camera.zoomIn"));
    }
}

/// Every icon the ribbon, the Layers pane and the timeline show, with drafts beside
/// each (`name~a`, `~b`, `~c` when drawn). `GLOBECRAFT_ICON_NAMES` lists the names,
/// `GLOBECRAFT_ICON_DRAFTS=1` adds the drafts. Manual.
#[test]
#[ignore = "manual: renders an icon sheet"]
fn icon_sheet() {
    use egui_kittest::Harness;
    let dir = std::path::PathBuf::from(
        std::env::var("GLOBECRAFT_SHOTS").unwrap_or_else(|_| "target/screenshots".into()),
    );
    std::fs::create_dir_all(&dir).expect("screenshot folder");
    let names: Vec<String> = std::env::var("GLOBECRAFT_ICON_NAMES")
        .expect("GLOBECRAFT_ICON_NAMES")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let drafts = std::env::var("GLOBECRAFT_ICON_DRAFTS").is_ok();
    let variants: Vec<&str> = if drafts {
        vec!["", "~a", "~b", "~c"]
    } else {
        vec![""]
    };
    let per_row = if drafts { 3 } else { 8 };
    let cell_w = 70.0 * variants.len() as f32 + 20.0;
    let cell_h = 112.0;
    let rows = names.len().div_ceil(per_row);
    let size = egui::Vec2::new(cell_w * per_row as f32 + 20.0, cell_h * rows as f32 + 20.0);
    for (theme, dark) in [("dark", true), ("light", false)] {
        let (names, variants) = (names.clone(), variants.clone());
        let mut fonts_set = false;
        let mut h = Harness::builder()
            .with_size(size)
            .with_pixels_per_point(1.5)
            .renderer(WgpuTestRenderer::new())
            .build_ui(move |ui| {
                // The interface's fonts arrive with the next frame; draw from then.
                if !fonts_set {
                    ui.ctx().set_fonts(super::super::fonts::base_definitions());
                    fonts_set = true;
                    ui.ctx().request_repaint();
                    return;
                }
                let (bg, card, ink, text) = if dark {
                    (
                        egui::Color32::from_rgb(24, 26, 30),
                        egui::Color32::from_rgb(36, 39, 44),
                        egui::Color32::from_rgb(220, 224, 230),
                        egui::Color32::from_rgb(200, 204, 210),
                    )
                } else {
                    (
                        egui::Color32::from_rgb(232, 234, 238),
                        egui::Color32::from_rgb(250, 251, 252),
                        egui::Color32::from_rgb(40, 44, 52),
                        egui::Color32::from_rgb(60, 64, 72),
                    )
                };
                let accent = egui::Color32::from_rgb(0x2B, 0x7B, 0xE4);
                let painter = ui.painter();
                painter.rect_filled(ui.max_rect(), 0.0, bg);
                for (n, name) in names.iter().enumerate() {
                    let origin = egui::pos2(
                        10.0 + (n % per_row) as f32 * cell_w,
                        10.0 + (n / per_row) as f32 * cell_h,
                    );
                    painter.rect_filled(
                        egui::Rect::from_min_size(origin, egui::vec2(cell_w - 8.0, cell_h - 8.0)),
                        8.0,
                        card,
                    );
                    painter.text(
                        origin + egui::vec2(10.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        format!("{} {name}", n + 1),
                        egui::FontId::proportional(12.0),
                        text,
                    );
                    for (v, variant) in variants.iter().enumerate() {
                        let x = origin.x + 10.0 + v as f32 * 70.0;
                        let big = egui::Rect::from_min_size(
                            egui::pos2(x, origin.y + 26.0),
                            egui::Vec2::splat(44.0),
                        );
                        let full = format!("{name}{variant}");
                        super::super::ui::icons::paint(painter, big, &full, ink, accent);
                        let small = egui::Rect::from_min_size(
                            egui::pos2(x + 48.0, origin.y + 46.0),
                            egui::Vec2::splat(20.0),
                        );
                        super::super::ui::icons::paint(painter, small, &full, ink, accent);
                        let caption = match *variant {
                            "" => "now",
                            "~a" => "A",
                            "~b" => "B",
                            _ => "C",
                        };
                        painter.text(
                            egui::pos2(x + 22.0, origin.y + 84.0),
                            egui::Align2::CENTER_CENTER,
                            caption,
                            egui::FontId::proportional(12.0),
                            text,
                        );
                    }
                }
            });
        h.run();
        h.run();
        let image = h.render().expect("render");
        let suffix = if drafts { "-drafts" } else { "" };
        image
            .save(dir.join(format!("icons{suffix}-{theme}.png")))
            .expect("save");
    }
}

/// The legend opens when a coded layer appears and closes when the last one goes;
/// opened by hand with nothing on the globe, it stays (user, 2026-11-18). No rendering.
#[test]
fn the_legend_closes_when_its_last_key_goes() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .build_eframe(|cc| App::new(cc, Settings::default(), Arc::new(RestartRequest::default())));
    let app = h.state_mut();
    let show = |app: &mut App, visible: bool| {
        app.run("layer.show", json!({"layer": "aurora", "visible": visible}))
            .expect("aurora");
        app.legend_watch();
    };
    show(app, true);
    assert!(app.legend_open());
    show(app, false);
    assert!(!app.legend_open());
    app.run("ui.legend", json!({"on": true})).expect("legend");
    app.legend_watch();
    assert!(app.legend_open());
}

/// `camera.lookAround` turns the view about the eye; raised past the ground it looks
/// up at the sky (roadmap C8), and lowered it comes back. No rendering.
#[test]
fn looking_around_turns_the_view_about_the_eye() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .build_eframe(|cc| App::new(cc, Settings::default(), Arc::new(RestartRequest::default())));
    let app = h.state_mut();
    app.run(
        "camera.set",
        json!({"lat": 40.0, "lon": -74.0, "altitude_km": 2.0, "heading_deg": 0.0, "tilt_deg": 60.0}),
    )
    .expect("camera");
    let eye = |app: &App| app.view_basis(1.0).c;
    let before = eye(app);
    let turned = app
        .run("camera.lookAround", json!({"turn_deg": 30.0}))
        .expect("look");
    let heading = turned["heading_deg"].as_f64().unwrap();
    assert!((heading - 30.0).abs() < 1.0, "{heading}");
    let moved = eye(app).distance(before) * globecraft_core::EARTH_RADIUS_KM;
    assert!(moved < 1e-6, "{moved} km");
    let raised = app
        .run("camera.lookAround", json!({"raise_deg": 60.0}))
        .expect("look up");
    assert!(raised["look_up_deg"].as_f64().unwrap() > 0.0, "{raised}");
    let lowered = app
        .run("camera.lookAround", json!({"raise_deg": -60.0}))
        .expect("look down");
    assert_eq!(lowered["look_up_deg"].as_f64(), Some(0.0));
    let moved = eye(app).distance(before) * globecraft_core::EARTH_RADIUS_KM;
    assert!(moved < 1e-6, "{moved} km");
    // Without angles it switches look-around dragging on, and off again.
    let mode = app.run("camera.lookAround", json!({})).expect("mode");
    assert_eq!(mode["look_mode"], true);
    let mode = app.run("camera.lookAround", json!({})).expect("mode");
    assert_eq!(mode["look_mode"], false);
}

/// The status bar's camera altitude is the camera's height above sea level, and
/// entering one puts the camera there along its line of sight. No rendering.
#[test]
fn the_camera_altitude_is_its_height() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .build_eframe(|cc| App::new(cc, Settings::default(), Arc::new(RestartRequest::default())));
    let app = h.state_mut();
    app.run(
        "camera.set",
        json!({"lat": 40.0, "lon": -74.0, "altitude_km": 10.0, "heading_deg": 0.0, "tilt_deg": 60.0}),
    )
    .expect("camera");
    // Ten km from the point it looks at, leaning 60°: about 5 km up.
    let height = app.eye_height_km();
    assert!((height - 5.0).abs() < 0.1, "{height}");
    let alt = app.alt_for_eye_height(3.0);
    app.cam.alt = alt;
    assert!(
        (app.eye_height_km() - 3.0).abs() < 1e-3,
        "{}",
        app.eye_height_km()
    );
    assert!((alt - 6.0).abs() < 0.1, "{alt}");
}

/// The side pane (one since 2026-11-19): its tabs, What's here as a tab, docked on
/// the left, and its content at the narrowest and a wide width.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn side_pane_screenshots() {
    use globecraft_data::settings::workspace::{
        MAX_RIGHT_PANE_WIDTH_POINTS, MIN_RIGHT_PANE_WIDTH_POINTS, PaneSide,
    };
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    settle(&mut h, 1.0);
    save(&mut h, &dir, "pane-default");
    let _ = h
        .state_mut()
        .run("ui.here", json!({"lat": 46.3, "lon": 8.3}));
    settle(&mut h, 1.0);
    assert_eq!(
        h.state().settings.workspace.right_pane,
        Some(PaneTab::Here),
        "What's here opens as a tab"
    );
    save(&mut h, &dir, "pane-here");
    let _ = h
        .state_mut()
        .run("ui.pane.moveTab", json!({"tab": "here", "to": 0}));
    let _ = h.state_mut().run("ui.pane.side", Value::Null);
    settle(&mut h, 1.0);
    assert_eq!(h.state().settings.workspace.pane_side, PaneSide::Left);
    assert_eq!(h.state().settings.workspace.right_open[0], PaneTab::Here);
    save(&mut h, &dir, "pane-left");
    for (width, name) in [
        (MIN_RIGHT_PANE_WIDTH_POINTS, "pane-narrow"),
        (MAX_RIGHT_PANE_WIDTH_POINTS, "pane-wide"),
    ] {
        let mut settings = Settings::default();
        settings.appearance.theme = ThemePreference::Dark;
        settings.workspace.right_width_points = width;
        settings.workspace.open_right(PaneTab::Measure);
        settings.workspace.open_right(PaneTab::Legend);
        settings.workspace.open_right(PaneTab::Layers);
        let mut h = harness_with(settings);
        settle(&mut h, 1.0);
        save(&mut h, &dir, name);
    }
    // Closing the tab clears what it showed.
    let mut h = harness(ThemePreference::Dark);
    let _ = h
        .state_mut()
        .run("ui.here", json!({"lat": 46.3, "lon": 8.3}));
    settle(&mut h, 0.5);
    h.state_mut().pointer.here_card = None;
    settle(&mut h, 0.5);
    assert!(
        !h.state().settings.workspace.right_is_open(PaneTab::Here),
        "the tab goes with its selection"
    );
}

/// The View tab's Worlds group (its lone button in the group's middle) and the
/// Settings window's page clear of its sidebar line (user, 2026-11-20).
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn view_ribbon_and_settings_screenshots() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    let app = h.state_mut();
    app.settings.workspace.ribbon_tab = globecraft_core::commands::Tab::View;
    app.settings.experience = globecraft_data::settings::experience::ExperienceLevel::Advanced;
    settle(&mut h, 0.5);
    // Twice: the group centres its tools from their width the frame before.
    settle(&mut h, 0.2);
    save(&mut h, &dir, "ribbon-view");
    let _ = h.state_mut().run("ui.settings", json!({}));
    settle(&mut h, 0.5);
    save(&mut h, &dir, "settings-general");
}

/// The side pane mid-slide, closing and opening (user, 2026-11-20): the globe and
/// its overlays follow the pane's edge, the pane's text keeps its wrap.
#[test]
#[ignore = "writes screenshots; run with GLOBECRAFT_SHOTS=<dir>"]
fn side_pane_slide_screenshots() {
    let Some(dir) = std::env::var_os("GLOBECRAFT_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut h = harness(ThemePreference::Dark);
    // PROBE_CITY: over a city, where black bars beside a frame would show (user's
    // recording, 2026-11-21 08:39).
    let city = std::env::var_os("PROBE_CITY").is_some();
    if city {
        h.state_mut()
            .run(
                "camera.set",
                json!({"lat": 53.0758, "lon": 8.8071, "altitude_km": 2.0}),
            )
            .expect("camera");
    }
    settle(&mut h, if city { 30.0 } else { 3.0 });
    save(&mut h, &dir, "pane-open");
    // A slow slide, so a quarter-second step lands mid-way.
    h.ctx.global_style_mut(|style| style.animation_time = 1.0);
    for name in ["pane-closing", "pane-closed", "pane-opening", "pane-opened"] {
        if matches!(name, "pane-closing" | "pane-opening") {
            let _ = h.state_mut().run("ui.leftPane", json!({}));
            // Mid-way, with time for a frame to finish.
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
        h.step();
        if matches!(name, "pane-closed" | "pane-opened") {
            settle(&mut h, 1.0);
        }
        save(&mut h, &dir, name);
    }
    // The window made larger (user's recording, 2026-11-21 08:44): the frame before
    // grows with it until the new one comes.
    let size = h.ctx.content_rect().size();
    h.set_size(size + egui::vec2(240.0, 160.0));
    h.step();
    save(&mut h, &dir, "window-grown-first");
    settle(&mut h, 2.0);
    save(&mut h, &dir, "window-grown");
}
