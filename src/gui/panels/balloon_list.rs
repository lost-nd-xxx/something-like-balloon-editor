use egui::{Context, Ui};
use crate::gui::app::BalloonEditorApp;

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    let available_height = ui.available_height();
    let half = (available_height / 2.0).max(60.0);

    // ── 上部：バルーンリスト ──────────────────────────────
    ui.strong("バルーン");
    ui.separator();

    let balloons = app.state.preview_balloons.clone();
    egui::ScrollArea::vertical()
        .id_salt("balloon_scroll")
        .max_height(half - 30.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            for name in &balloons {
                let label = name.trim_end_matches(".png");
                let selected = app.state.selected_balloon == *name;
                let resp = ui.selectable_label(selected, label);
                if resp.clicked() && !selected {
                    app.state.selected_balloon = name.clone();
                    app.state.png_preview_name = None;
                    app.refresh_preview_texture_silent(ctx);
                }
                resp.context_menu(|ui| {
                    balloon_context_menu(ui, app, name, ctx);
                });
            }
        });

    ui.separator();

    // ── 下部：プロジェクト内 PNG 一覧 ────────────────────
    ui.strong("PNG一覧");
    ui.separator();

    // プロジェクトフォルダ内の全 PNG を収集（ソート済み）
    let png_list = collect_project_pngs(app);

    egui::ScrollArea::vertical()
        .id_salt("png_scroll")
        .max_height(half - 30.0)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if png_list.is_empty() {
                ui.label("（PNGファイルなし）");
            }
            for (i, name) in png_list.iter().enumerate() {
                let stem = name.trim_end_matches(".png");
                let selected = app.state.png_preview_name.as_deref() == Some(name.as_str());
                let name_owned = name.clone();
                ui.push_id(("png_item", i), |ui| {
                    let resp = ui.selectable_label(selected, stem);
                    if resp.clicked() {
                        app.preview_single_png(&name_owned, ctx);
                    }
                    resp.context_menu(|ui| {
                        png_context_menu(ui, app, &name_owned, ctx);
                    });
                });
            }
        });
}

/// バルーンリストの右クリックメニュー
fn balloon_context_menu(ui: &mut Ui, app: &mut BalloonEditorApp, name: &str, _ctx: &Context) {
    if ui.button("名前変更…").clicked() {
        let stem = name.trim_end_matches(".png").to_string();
        app.state.rename_target = name.to_string();
        app.state.rename_new_name = stem;
        app.state.rename_warning = String::new();
        app.state.show_rename_window = true;
        ui.close_menu();
    }
    if ui.button("削除…").clicked() {
        app.request_delete_png(name);
        ui.close_menu();
    }
}

/// PNG一覧の右クリックメニュー
fn png_context_menu(ui: &mut Ui, app: &mut BalloonEditorApp, name: &str, _ctx: &Context) {
    if ui.button("名前変更…").clicked() {
        let stem = name.trim_end_matches(".png").to_string();
        app.state.rename_target = name.to_string();
        app.state.rename_new_name = stem;
        app.state.rename_warning = String::new();
        app.state.show_rename_window = true;
        ui.close_menu();
    }
    if ui.button("削除…").clicked() {
        app.request_delete_png(name);
        ui.close_menu();
    }
}

/// プロジェクトフォルダ内の全 PNG ファイル名をソートして返す
fn collect_project_pngs(app: &BalloonEditorApp) -> Vec<String> {
    let Some(asset_dir) = app.state.asset_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&asset_dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) == Some("png") {
                path.file_name()?.to_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}
