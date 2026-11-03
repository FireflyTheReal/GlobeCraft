//! Native UI presentation configuration; globe computation stays in the renderer.
use egui_wgpu::{WgpuConfiguration, WgpuSetup};
use wgpu::{Backends, PresentMode};

/// The graphics APIs the interface may draw with, unless `WGPU_BACKEND` names others.
/// On Linux and the BSDs this is Vulkan alone: wgpu starts every backend it is given,
/// and its OpenGL backend opens EGL at once, which panics inside `khronos-egl` when a
/// driver returns no display without an error (issue #1, an AppImage whose bundled
/// Wayland library did not match the system's Mesa). Without Vulkan, wgpu finds no
/// adapter and `main` falls back to the glow (OpenGL) renderer, which reports EGL
/// failures as errors. Elsewhere, egui's default (the platform API and OpenGL).
fn interface_backends(default: Backends) -> Backends {
    if cfg!(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )) {
        Backends::from_env().unwrap_or(Backends::VULKAN)
    } else {
        default
    }
}

pub(crate) fn configuration(uncap_fps: bool) -> WgpuConfiguration {
    let mut configuration = eframe::NativeOptions::default().wgpu_options;
    if uncap_fps {
        configuration.surface.present_mode = PresentMode::AutoNoVsync;
    }
    if let WgpuSetup::CreateNew(create_new) = &mut configuration.wgpu_setup {
        let descriptor = &mut create_new.instance_descriptor;
        descriptor.backends = interface_backends(descriptor.backends);
    }
    configuration
}

/// Whether a failed start with `renderer` should be retried with glow: wgpu found
/// no usable adapter or surface, which glow's OpenGL may still provide.
pub(crate) fn retry_with_glow(renderer: eframe::Renderer, error: &eframe::Error) -> bool {
    renderer == eframe::Renderer::Wgpu && matches!(error, eframe::Error::Wgpu(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_mode_preserves_the_saved_vsync_preference() {
        let defaults = eframe::NativeOptions::default();
        assert_eq!(defaults.renderer, eframe::Renderer::Wgpu);
        let capped = configuration(false);
        let uncapped = configuration(true);
        assert_eq!(
            capped.surface.present_mode,
            defaults.wgpu_options.surface.present_mode
        );
        assert_eq!(uncapped.surface.present_mode, PresentMode::AutoNoVsync);
        for configuration in [capped, uncapped] {
            let WgpuSetup::CreateNew(create_new) = &configuration.wgpu_setup else {
                panic!("the interface creates its own wgpu instance");
            };
            let backends = create_new.instance_descriptor.backends;
            if cfg!(target_os = "linux") && std::env::var_os("WGPU_BACKEND").is_none() {
                assert_eq!(backends, Backends::VULKAN);
            } else if !cfg!(target_os = "linux") {
                let WgpuSetup::CreateNew(default) = &defaults.wgpu_options.wgpu_setup else {
                    panic!("egui creates its own wgpu instance by default");
                };
                assert_eq!(backends, default.instance_descriptor.backends);
            }
            assert_eq!(
                configuration.surface.desired_maximum_frame_latency,
                defaults.wgpu_options.surface.desired_maximum_frame_latency
            );
        }
    }

    #[test]
    fn only_a_failed_wgpu_start_falls_back_to_glow() {
        let no_adapter = || {
            eframe::Error::Wgpu(egui_wgpu::WgpuError::CustomNativeAdapterSelectionError(
                "no adapter".into(),
            ))
        };
        assert!(retry_with_glow(eframe::Renderer::Wgpu, &no_adapter()));
        assert!(!retry_with_glow(eframe::Renderer::Glow, &no_adapter()));
        let other = eframe::Error::AppCreation("app".into());
        assert!(!retry_with_glow(eframe::Renderer::Wgpu, &other));
    }
}
