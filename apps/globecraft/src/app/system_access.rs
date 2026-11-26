//! The system's display settings: Reduce Motion, Increase Contrast and the accent colour
//! on macOS. egui draws its own controls, so nothing follows these unless the app reads
//! them.
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SystemAccess {
    pub(super) reduce_motion: bool,
    pub(super) increase_contrast: bool,
    /// The accent colour chosen in System Settings, for selections and switches.
    pub(super) accent: Option<Color32>,
}

/// The settings now. A few property reads, cheap enough for every frame, so a change in
/// System Settings shows at once.
pub(super) fn read() -> SystemAccess {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSColor, NSColorSpace, NSWorkspace};
        let workspace = NSWorkspace::sharedWorkspace();
        let accent = NSColor::controlAccentColor()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .map(|c| {
                let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                Color32::from_rgb(
                    byte(c.redComponent()),
                    byte(c.greenComponent()),
                    byte(c.blueComponent()),
                )
            });
        SystemAccess {
            reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
            accent,
        }
    }
    #[cfg(not(target_os = "macos"))]
    SystemAccess::default()
}
