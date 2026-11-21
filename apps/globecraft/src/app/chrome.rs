//! The header and ribbon's state (roadmap 10.2: one of the feature states split out of `App`).
use super::ui;
use eframe::egui::Rect;

/// Used when macOS cannot report the native traffic-light bounds. AppKit points.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHTS_WIDTH_FALLBACK_POINTS: f32 = 78.0;

/// What the window's header and the ribbon keep between frames.
pub(super) struct ChromeState {
    /// The UI scale the zoom was last set for.
    pub(super) styled_scale: Option<f32>,
    /// The system's Reduce Motion and Increase Contrast, read each frame.
    pub(super) access: super::system_access::SystemAccess,
    /// The (increase contrast, reduce motion) the style was last built for.
    pub(super) styled_look: Option<(bool, bool)>,
    /// egui's font atlas fill at the start of the last frame (see `ui`).
    pub(super) font_atlas_fill: f32,
    /// The ribbon group being drawn, and the shortcut dialog.
    pub(super) ribbon_tools: ui::ribbon_tools::RibbonTools,
    /// The ribbon shown over the globe while collapsed (a tab was clicked).
    pub(super) ribbon_peek: bool,
    pub(super) tab_strip_rect: Option<Rect>,
    /// The native macOS traffic-light width, in AppKit points. Divide by egui's
    /// zoom factor to reserve the same room in egui points.
    #[cfg(target_os = "macos")]
    pub(super) traffic_lights_width_native: f32,
    #[cfg(target_os = "macos")]
    pub(super) native_menu: Option<super::menu::NativeMenu>,
}

impl ChromeState {
    #[cfg_attr(not(target_os = "macos"), expect(unused_variables))]
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            styled_scale: None,
            access: Default::default(),
            styled_look: None,
            font_atlas_fill: 0.0,
            ribbon_tools: Default::default(),
            ribbon_peek: false,
            tab_strip_rect: None,
            #[cfg(target_os = "macos")]
            traffic_lights_width_native: {
                use raw_window_handle::HasWindowHandle as _;
                cc.window_handle()
                    .ok()
                    .and_then(|handle| {
                        eframe::WindowChromeMetrics::from_window_handle(&handle.as_raw())
                    })
                    .map(|metrics| metrics.traffic_lights_size.x)
                    .filter(|width| *width > 0.0)
                    .unwrap_or(TRAFFIC_LIGHTS_WIDTH_FALLBACK_POINTS)
            },
            #[cfg(target_os = "macos")]
            native_menu: None,
        }
    }
}
