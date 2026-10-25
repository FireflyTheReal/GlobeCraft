//! Desktop compass widgets; geographic bearings live in core.
use eframe::egui::{self, Sense};
use globecraft_core::{compass::heading_text, language::Lang};

pub(super) fn heading_readout(ui: &mut egui::Ui, heading: f64, lang: Lang) -> bool {
    // Match the status bar's horizontal row height and baseline.
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(heading_text(heading, lang)).monospace())
                .sense(Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(lang.text("compass-heading-tooltip"))
        .clicked()
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heading_readout_resets_only_when_clicked_and_matches_status_baseline() {
        let ctx = egui::Context::default();
        let frame = |events| {
            let mut clicked = false;
            let output = crate::app::test_ui::run(
                &ctx,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let origin = ui.next_widget_position();
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                                origin,
                                egui::vec2(180.0, 30.0),
                            )),
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.monospace("Center");
                                });
                            },
                        );
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                                origin + egui::vec2(200.0, 0.0),
                                egui::vec2(180.0, 30.0),
                            )),
                            |ui| {
                                clicked = heading_readout(ui, 0.0, Lang::En);
                            },
                        );
                    });
                },
            );
            (clicked, output)
        };
        let (_, output) = frame(vec![]);
        let text = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "0° N" => Some(t),
                _ => None,
            })
            .unwrap();
        let center_y = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == "Center" => Some(t.pos.y),
                _ => None,
            })
            .unwrap();
        assert_eq!(text.pos.y, center_y);
        let pos = text.pos + text.galley.size() * 0.5;
        assert!(!frame(vec![egui::Event::PointerMoved(pos)]).0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(!frame(vec![button(true)]).0);
        assert!(frame(vec![button(false)]).0);
    }
}
