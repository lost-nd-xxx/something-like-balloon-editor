use egui::{Context, Ui};
use crate::gui::app::BalloonEditorApp;
use crate::gui::state::{PreviewTextMode, ThemeMode};

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    egui::menu::bar(ui, |ui| {
        // ファイルメニュー
        ui.menu_button("ファイル", |ui| {
            // 素材フォルダ選択
            {
                if ui.button("素材フォルダを選択…").clicked() {
                    ui.close_menu();
                    app.select_asset_folder_dialog(ctx);
                }
                if ui.button("素材フォルダをエクスプローラで開く").clicked() {
                    ui.close_menu();
                    if let Some(path) = app.state.asset_dir() {
                        let _ = open::that(&path);
                    }
                }
            }
            ui.separator();
            if ui.button("プロファイルを保存…").clicked() {
                ui.close_menu();
                app.save_profile_dialog();
            }
            if ui.button("プロファイルを読み込み…").clicked() {
                ui.close_menu();
                app.load_profile_dialog(ctx);
            }
            ui.separator();
            if ui.button("出力  (Ctrl+E)").clicked() {
                ui.close_menu();
                app.export();
            }
        });

        // 編集メニュー
        ui.menu_button("編集", |ui| {
            if ui.button("元に戻す  (Ctrl+Z)").clicked() {
                ui.close_menu();
                if app.state.undo() {
                    app.rebuild_and_refresh(ctx);
                } else {
                    app.refresh_preview_texture(ctx);
                }
            }
            if ui.button("やり直し  (Ctrl+Y)").clicked() {
                ui.close_menu();
                if app.state.redo() {
                    app.rebuild_and_refresh(ctx);
                } else {
                    app.refresh_preview_texture(ctx);
                }
            }
            ui.separator();
            if ui.button("バルーン設定を初期値に戻す").clicked() {
                ui.close_menu();
                app.state.push_undo();
                use crate::gui::state::LAYER_DEFS;
                for &(k, _, c) in LAYER_DEFS {
                    app.state.layer_colors.insert(k.to_string(), c);
                }
                let default_parts = app.state.layer_colors.get("parts").copied()
                    .unwrap_or(crate::core::color::Rgb(29, 106, 184));
                app.state.parts_colors.clear();
                app.state.parts_colors.insert("all".to_string(), default_parts);
                app.state.basic_info.clear();
                app.reload_asset_folder(ctx);
            }
            ui.separator();
            let direct = app.state.direct_image_mode;
            // 画像編集なしモードでは強制的に no_balloon_color = true、チェックボックスはグレーアウト
            let mut no_color = if direct { true } else { app.state.no_balloon_color };
            ui.add_enabled_ui(!direct, |ui| {
                if ui.checkbox(&mut no_color, "バルーン画像色を変更しない").changed() {
                    app.state.no_balloon_color = no_color;
                    ui.close_menu();
                    app.rebuild_and_refresh(ctx);
                }
            });
        });

        // 表示メニュー
        ui.menu_button("表示", |ui| {
            if ui.button("プレビュー更新  (F5)").clicked() {
                ui.close_menu();
                app.rebuild_and_refresh(ctx);
            }
            ui.separator();
            if ui.button("ウィンドウレイアウトをリセット").clicked() {
                ui.close_menu();
                app.state.panel_left_width  = 150.0;
                app.state.panel_right_width = 320.0;
                app.state.window_size       = [1400.0, 720.0];
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                    egui::vec2(1400.0, 720.0)
                ));
            }
            ui.separator();

            // テーマ
            ui.label("テーマ:");
            for (mode, label) in [
                (ThemeMode::Light, "ライト"),
                (ThemeMode::Dark,  "ダーク"),
            ] {
                if ui.radio(app.state.theme == mode, label).clicked() {
                    app.state.theme = mode;
                    crate::gui::app::apply_theme(ctx, mode);
                    ui.close_menu();
                }
            }
            ui.separator();

            // テキスト域オーバーレイ
            let mut overlay_on = app.state.overlay_mode == "layout";
            if ui.checkbox(&mut overlay_on, "テキスト域オーバーレイ表示").changed() {
                app.state.overlay_mode = if overlay_on { "layout".to_string() } else { String::new() };
                app.refresh_preview_texture(ctx);
            }
            ui.separator();

            // プレビューテキストモード
            ui.label("プレビューテキスト:");
            if ui.radio(app.state.preview_text_mode == PreviewTextMode::A, "A: 通常サンプル").clicked() {
                app.state.preview_text_mode = PreviewTextMode::A;
                app.refresh_preview_texture(ctx);
            }
            if ui.radio(app.state.preview_text_mode == PreviewTextMode::B, "B: 折り返し幅確認").clicked() {
                app.state.preview_text_mode = PreviewTextMode::B;
                app.refresh_preview_texture(ctx);
            }
            ui.separator();

            // キャンバス背景
            ui.label("背景:");
            if ui.radio(matches!(app.state.canvas_bg, crate::gui::state::CanvasBg::Checker), "チェッカー").clicked() {
                app.state.canvas_bg = crate::gui::state::CanvasBg::Checker;
                app.refresh_preview_texture(ctx);
            }
            let is_solid = matches!(app.state.canvas_bg, crate::gui::state::CanvasBg::Solid(..));
            // 現在の単色背景色を取得
            let (sr, sg, sb) = if let crate::gui::state::CanvasBg::Solid(r,g,b) = app.state.canvas_bg {
                (r, g, b)
            } else { (255, 255, 255) };
            ui.horizontal(|ui| {
                if ui.radio(is_solid, "単色背景").clicked() {
                    app.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                    app.refresh_preview_texture(ctx);
                    ui.close_menu();
                }
                let swatch = egui::Color32::from_rgb(sr, sg, sb);
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(20.0, 20.0),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(rect, 2.0, swatch);
                ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color));
                if ui.interact(rect, ui.id().with("bg_swatch"), egui::Sense::click()).clicked() {
                    app.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                    app.state.show_bg_color_window = true;
                    ui.close_menu();
                }
            });
        });

        // ヘルプメニュー
        ui.menu_button("ヘルプ", |ui| {
            if ui.button("オンラインマニュアル").clicked() {
                ui.close_menu();
                let _ = open::that("https://github.com/lost-nd-xxx/something_like_balloon_editor/blob/main/MANUAL.md");
            }
            if ui.button("リポジトリ").clicked() {
                ui.close_menu();
                let _ = open::that(crate::gui::state::APP_REPOSITORY_URL);
            }
            ui.separator();
            if ui.button("バージョン情報").clicked() {
                ui.close_menu();
                app.dialog = Some(("バージョン情報".into(), format!(
                    "バルーンエディタ  v{}\n\nライセンス: MIT License\n{}",
                    crate::gui::state::APP_VERSION,
                    crate::gui::state::APP_REPOSITORY_URL,
                )));
            }
        });
    });
}
