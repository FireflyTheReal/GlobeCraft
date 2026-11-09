//! Headless egui passes intentionally discard texture uploads instead of rendering.
use eframe::egui;

pub(crate) fn run(
    ctx: &egui::Context,
    input: egui::RawInput,
    ui: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, ui);
    output.textures_delta.clear();
    output
}

/// A context with the application's fonts (the ribbon and dialogs use its named families).
pub(crate) fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(super::fonts::base_definitions());
    ctx
}
