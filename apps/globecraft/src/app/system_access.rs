//! The system's display accessibility settings: Reduce Motion and Increase Contrast on
//! macOS. egui draws its own controls, so nothing follows these unless the app reads them.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SystemAccess {
    pub(super) reduce_motion: bool,
    pub(super) increase_contrast: bool,
}

/// The settings now. Two property reads, cheap enough for every frame, so a change in
/// System Settings shows at once.
pub(super) fn read() -> SystemAccess {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        SystemAccess {
            reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    SystemAccess::default()
}
