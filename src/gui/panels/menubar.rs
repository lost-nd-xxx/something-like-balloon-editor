use egui::{Context, Ui};
use crate::gui::app::BalloonEditorApp;
use crate::gui::state::{PreviewTextMode, ThemeMode};

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    egui::menu::bar(ui, |ui| {
        // ファイルメニュー
        ui.menu_button("ファイル", |ui| {
            if ui.button("新規プロジェクト...").clicked() {
                ui.close_menu();
                app.state.show_new_project_window = true;
            }
            if ui.button("プロジェクトを開く...").clicked() {
                ui.close_menu();
                app.state.open_project_list = crate::core::project::list_projects();
                app.state.open_project_selected = None;
                app.state.show_open_project_window = true;
            }
            if ui.button("フォルダからプロジェクトを作成...").clicked() {
                ui.close_menu();
                // update 内で rfd を同期呼び出しするとそのフレームが完結しないため、
                // 要求だけ立てて別スレッドで開く
                app.state.request_pick_import_folder = true;
            }
            ui.separator();
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("上書き保存  (Ctrl+S)").clicked() {
                    ui.close_menu();
                    app.save_project();
                }
            });
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("別名で保存...").clicked() {
                    ui.close_menu();
                    let current_name = app.state.asset_dir()
                        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_default();
                    app.state.save_as_project_name = current_name;
                    app.state.save_as_project_warning = String::new();
                    app.state.show_save_as_project_window = true;
                }
            });
            ui.separator();
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("画像をプロジェクトに追加...").clicked() {
                    ui.close_menu();
                    // 同上（update 内での rfd 同期呼び出しを避ける）
                    app.state.request_pick_import_images = true;
                }
            });
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("プロジェクトをエクスプローラで開く").clicked() {
                    ui.close_menu();
                    if let Some(path) = app.state.asset_dir() {
                        let _ = open::that(&path);
                    }
                }
            });
            ui.separator();
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("バルーンとして出力  (Ctrl+E)").clicked() {
                    ui.close_menu();
                    app.pending_export = true;
                }
            });
        });

        // 編集メニュー
        ui.menu_button("編集", |ui| {
            let no_project = app.state.asset_dir().is_none();
            ui.add_enabled_ui(!no_project, |ui| {
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
            });
            ui.separator();
            ui.add_enabled_ui(!no_project, |ui| {
            if ui.button("バルーン設定を初期値に戻す").clicked() {
                ui.close_menu();
                app.state.push_undo();
                use crate::gui::state::LAYER_DEFS;
                // 編集中バッファを破棄（残っていると再読み込み後に書き戻されるため）
                app.state.editing_buf = None;
                app.state.basic_info.clear();
                // 共通テキストをディスクの保存済み内容へ戻す
                // （reload は slbe_profile.json の色や {stem}s.txt を復活させるので、
                //  この後でメモリ上の色・個別設定を初期値へ上書きし直す）
                app.reload_asset_folder(ctx);
                // レイヤー色を初期値へ
                for &(k, _, c) in LAYER_DEFS {
                    app.state.layer_colors.insert(k.to_string(), c);
                }
                let default_parts = app.state.layer_colors.get("parts").copied()
                    .unwrap_or(crate::core::color::Rgb(29, 106, 184));
                app.state.parts_colors.clear();
                app.state.parts_colors.insert("all".to_string(), default_parts);
                // reload で {stem}s.txt から復活した個別設定をメモリから消す。
                // （ディスク上のファイルは次回保存時に削除される）
                app.state.individual_colors.clear();
                app.state.individual_texts.clear();
                app.rebuild_and_refresh(ctx);
            }
            }); // add_enabled_ui(!no_project) バルーン設定を初期値に戻す
            ui.separator();
            ui.add_enabled_ui(app.state.is_project_dir(), |ui| {
                if ui.button("レイアウト定義を編集...").clicked() {
                    ui.close_menu();
                    app.state.show_files_editor_window = true;
                }
            });
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
            let mut auto_flip = app.state.auto_flip;
            if ui.checkbox(&mut auto_flip, "奇数・偶数番バルーンを自動補完").changed() {
                app.state.auto_flip = auto_flip;
                ui.close_menu();
                // reload_asset_folder は slbe_profile.json から no_balloon_color を復元してしまうため、
                // reload 後に現在の値で上書きして保持する
                let saved_no_color = app.state.no_balloon_color;
                app.reload_asset_folder(ctx);
                app.state.no_balloon_color = saved_no_color;
            }
        });

        // 表示メニュー
        ui.menu_button("表示", |ui| {
            let no_project = app.state.asset_dir().is_none();
            ui.add_enabled_ui(!no_project, |ui| {
                if ui.button("プレビュー更新  (F5)").clicked() {
                    ui.close_menu();
                    app.reload_asset_folder_keep_texts(ctx);
                }
            });
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

            // テキスト域オーバーレイ
            let mut overlay_on = app.state.overlay_mode == "layout";
            if ui.checkbox(&mut overlay_on, "テキスト域オーバーレイ表示").changed() {
                app.state.overlay_mode = if overlay_on { "layout".to_string() } else { String::new() };
                app.refresh_preview_texture(ctx);
            }
            ui.separator();

            // アプリテーマ（サブメニュー）
            ui.menu_button("アプリテーマ", |ui| {
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
            });

            // プレビューテキスト（サブメニュー）
            ui.menu_button("プレビューテキスト", |ui| {
                if ui.radio(app.state.preview_text_mode == PreviewTextMode::A, "A: 通常サンプル").clicked() {
                    app.state.preview_text_mode = PreviewTextMode::A;
                    app.refresh_preview_texture(ctx);
                    ui.close_menu();
                }
                if ui.radio(app.state.preview_text_mode == PreviewTextMode::B, "B: 折り返し幅確認").clicked() {
                    app.state.preview_text_mode = PreviewTextMode::B;
                    app.refresh_preview_texture(ctx);
                    ui.close_menu();
                }
            });

            // プレビュー背景（サブメニュー）
            let is_solid = matches!(app.state.canvas_bg, crate::gui::state::CanvasBg::Solid(..));
            // 単色色は canvas_bg_solid_color から取得（チェッカー中も保持）
            let (sr, sg, sb) = app.state.canvas_bg_solid_color;
            ui.menu_button("プレビュー背景", |ui| {
                if ui.radio(matches!(app.state.canvas_bg, crate::gui::state::CanvasBg::Checker), "チェッカー").clicked() {
                    app.state.canvas_bg = crate::gui::state::CanvasBg::Checker;
                    app.refresh_preview_texture(ctx);
                    ui.close_menu();
                }
                ui.horizontal(|ui| {
                    if ui.radio(is_solid, "単色背景").clicked() {
                        app.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                        app.refresh_preview_texture(ctx);
                        ui.close_menu();
                    }
                    let swatch = egui::Color32::from_rgb(sr, sg, sb);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 2.0, swatch);
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.fg_stroke.color));
                    if ui.interact(rect, ui.id().with("bg_swatch"), egui::Sense::click()).clicked() {
                        app.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                        app.state.show_bg_color_window = true;
                        ui.close_menu();
                    }
                });
            });
        });

        // ヘルプメニュー
        ui.menu_button("ヘルプ", |ui| {
            if ui.button("オンラインマニュアル").clicked() {
                ui.close_menu();
                let _ = open::that("https://github.com/lost-nd-xxx/something-like-balloon-editor/blob/main/MANUAL.md");
            }
            if ui.button("リポジトリ").clicked() {
                ui.close_menu();
                let _ = open::that(crate::gui::state::APP_REPOSITORY_URL);
            }
            ui.separator();
            if ui.button("バージョン情報").clicked() {
                ui.close_menu();
                app.dialog = Some(("バージョン情報".into(), format!(
                    "{}  v{}\n\nライセンス: MIT License\n{}",
                    crate::gui::state::APP_NAME,
                    crate::gui::state::APP_VERSION,
                    crate::gui::state::APP_REPOSITORY_URL,
                )));
            }
        });
    });
}
