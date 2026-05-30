use egui::{Context, Ui};
use crate::gui::app::BalloonEditorApp;

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    let available_height = ui.available_height();

    // 開いているセクション数で高さを按分（片方を畳めば残り片方が最大化）
    let open_count = (app.state.balloon_list_open as u8 + app.state.png_list_open as u8).max(1);
    // セパレータ・ヘッダ分の余白を控えめに差し引く
    let section_h = ((available_height - 40.0) / open_count as f32).max(60.0);

    // ── 上部：バルーンリスト（折りたたみ可能）──────────────
    let balloons = app.state.preview_balloons.clone();
    let bh = egui::CollapsingHeader::new(egui::RichText::new("バルーン").strong())
        .id_salt("balloon_header")
        .open(Some(app.state.balloon_list_open))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("balloon_scroll")
                .max_height(section_h)
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
        });
    if bh.header_response.clicked() {
        app.state.balloon_list_open = !app.state.balloon_list_open;
    }

    ui.separator();

    // ── 下部：プロジェクト内 PNG 一覧（折りたたみ可能）──────
    // プロジェクトフォルダ内の全 PNG を収集（ソート済み）
    let png_list = collect_project_pngs(app);

    let ph = egui::CollapsingHeader::new(egui::RichText::new("画像一覧").strong())
        .id_salt("png_header")
        .open(Some(app.state.png_list_open))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("png_scroll")
                .max_height(section_h)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if png_list.is_empty() {
                        ui.label("（PNGファイルなし）");
                    }
                    for (i, name) in png_list.iter().enumerate() {
                        // .png は拡張子を落として表示。.pnr 等はそのまま表示して区別する
                        let label = if let Some(s) = name.strip_suffix(".png") { s } else { name.as_str() };
                        let stem = label;
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
        });
    if ph.header_response.clicked() {
        app.state.png_list_open = !app.state.png_list_open;
    }
}

/// バルーンリストの右クリックメニュー
fn balloon_context_menu(ui: &mut Ui, app: &mut BalloonEditorApp, name: &str, _ctx: &Context) {
    if ui.button("名前変更…").clicked() {
        let stem = name.trim_end_matches(".png");
        app.state.rename_target = name.to_string();
        app.state.rename_warning = String::new();
        app.state.rename_is_balloon = true;
        // stem からスコープ番号・ID番号・c系フラグを逆算してプリセット
        if stem.starts_with("balloonc") {
            app.state.rename_balloon_is_c = true;
            app.state.rename_id_num = stem.strip_prefix("balloonc")
                .and_then(|s| s.parse().ok()).unwrap_or(0);
            app.state.rename_scope_num = 0;
        } else {
            app.state.rename_balloon_is_c = false;
            if stem.starts_with("balloons") {
                app.state.rename_scope_num = 0;
                app.state.rename_id_num = stem.strip_prefix("balloons")
                    .and_then(|s| s.parse().ok()).unwrap_or(0);
            } else if stem.starts_with("balloonk") {
                app.state.rename_scope_num = 1;
                app.state.rename_id_num = stem.strip_prefix("balloonk")
                    .and_then(|s| s.parse().ok()).unwrap_or(0);
            } else if stem.starts_with("balloonp") && stem.contains("def") {
                let parts: Vec<&str> = stem.splitn(2, "def").collect();
                app.state.rename_scope_num = parts[0].strip_prefix("balloonp")
                    .and_then(|s| s.parse().ok()).unwrap_or(2);
                app.state.rename_id_num = parts.get(1)
                    .and_then(|s| s.parse().ok()).unwrap_or(0);
            } else {
                app.state.rename_scope_num = 0;
                app.state.rename_id_num = 0;
            }
        }
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
        // 拡張子（.png/.pnr 等）を除いた stem を初期値にする
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name)
            .to_string();
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
    // png / pnr（左上1ドット透過版）を画像一覧に含める
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str())?.to_lowercase();
            if ext == "png" || ext == "pnr" {
                path.file_name()?.to_str().map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}
