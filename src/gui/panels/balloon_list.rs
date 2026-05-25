use egui::{Context, ScrollArea, Ui};
use crate::gui::app::BalloonEditorApp;

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    ui.strong("バルーン");
    ui.separator();

    // バルーンリスト
    let balloons = app.state.preview_balloons.clone();
    ScrollArea::vertical().show(ui, |ui| {
        for name in &balloons {
            let label = name.trim_end_matches(".png");
            let selected = app.state.selected_balloon == *name;
            if ui.selectable_label(selected, label).clicked() && !selected {
                app.state.selected_balloon = name.clone();
                app.refresh_preview_texture_silent(ctx);
            }
        }
    });
}
