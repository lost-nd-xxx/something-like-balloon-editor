/// メインウィンドウ（eframe::App の実装）
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::RgbaImage;
use crate::gui::{
    loader::{load_asset_folder, load_asset_folder_keep_texts,
             rebuild_balloon_cache, rebuild_selected_balloon, ensure_balloon_cached,
             rebuild_dynamic_defaults},
    panels::{self},
    state::AppState,
};

/// eframe アプリ本体
pub struct BalloonEditorApp {
    pub state: AppState,
    /// 実行ファイルと同じディレクトリ（アセット・出力の基点）
    pub root:  PathBuf,
    /// プレビュー用テクスチャ（現在選択中バルーンの描画済み画像）
    pub preview_texture: Option<TextureHandle>,
    /// ダイアログ表示用メッセージ (title, body)
    pub dialog: Option<(String, String)>,
    /// バックグラウンドスレッドからのプレビュー結果受け取り用
    preview_result: Arc<Mutex<Option<RgbaImage>>>,
}

impl BalloonEditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 日本語フォントを設定する
        setup_japanese_font(&cc.egui_ctx);

        let root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let mut state = AppState::new();

        // 起動時の状態復元
        let state_path = root.join("state.json");
        let has_profile = if let Some(profile) = crate::core::profile::load_state(&state_path) {
            apply_profile_to_state(&mut state, &profile);
            true
        } else {
            false
        };

        // 起動時テーマを適用
        apply_theme(&cc.egui_ctx, state.theme);

        let mut app = Self {
            state,
            root,
            preview_texture: None,
            dialog: None,
            preview_result: Arc::new(Mutex::new(None)),
        };

        // 素材フォルダを初期読み込み
        // プロファイルがある場合はテキストを上書きしない（編集済み内容を保持）
        let load_result = if has_profile {
            load_asset_folder_keep_texts(&mut app.state, &app.root)
        } else {
            load_asset_folder(&mut app.state, &app.root)
        };
        if let Err(e) = load_result {
            app.dialog = Some(("エラー".into(), format!("素材フォルダの読み込みに失敗しました:\n\n{}", e)));
        }

        app
    }

    /// 現在選択中バルーンの合成済み画像を返す
    fn current_balloon_image(&self) -> Option<&RgbaImage> {
        self.state.balloon_cache.get(&self.state.selected_balloon)
    }

    /// プレビューテクスチャを更新する（draw_preview をバックグラウンドスレッドで実行）
    /// show_generating: true のとき「プレビュー生成中」オーバーレイを表示する
    pub fn refresh_preview_texture(&mut self, ctx: &Context) {
        self.refresh_preview_texture_inner(ctx, true);
    }

    /// バルーン切り替え専用：既存テクスチャを維持しつつバックグラウンド更新
    pub fn refresh_preview_texture_silent(&mut self, ctx: &Context) {
        self.refresh_preview_texture_inner(ctx, false);
    }

    fn refresh_preview_texture_inner(&mut self, ctx: &Context, show_generating: bool) {
        // 選択バルーンがキャッシュに無ければ遅延合成する（メインスレッドで実行）
        if !self.state.balloon_cache.contains_key(&self.state.selected_balloon) {
            if let Some(asset_dir) = self.state.asset_dir() {
                if let Err(e) = ensure_balloon_cached(&mut self.state, &asset_dir) {
                    self.err(format!("画像の再構築に失敗しました:\n\n{}", e));
                }
            }
        }

        use crate::core::preview::{draw_preview, DrawOptions, PreviewMode};

        let base = match self.current_balloon_image() {
            Some(i) => i.clone(),
            None => {
                self.preview_texture = None;
                return;
            }
        };

        let balloon_name = self.state.selected_balloon.trim_end_matches(".png").to_string();
        let cfg_key = format!("{}s.txt", balloon_name);
        let descript_text = self.state.individual_texts
            .get(&cfg_key)
            .cloned()
            .unwrap_or_else(|| self.state.descript_text.clone());

        let empty_map = std::collections::HashMap::new();
        let parts = self.state.parts_cache
            .get(&self.state.selected_balloon)
            .unwrap_or(&empty_map)
            .clone();

        let opts = DrawOptions {
            mode: match self.state.preview_text_mode {
                crate::gui::state::PreviewTextMode::A => PreviewMode::A,
                crate::gui::state::PreviewTextMode::B => PreviewMode::B,
            },
            show_overlay: self.state.overlay_mode == "layout",
            is_balloonc:  self.state.is_balloonc(),
            balloon_name: self.state.selected_balloon.clone(),
            asset_dir:    self.state.asset_dir(),
        };

        // バックグラウンドスレッドで draw_preview を実行
        if show_generating {
            self.state.preview_generating = true;
        }
        let result_slot = Arc::clone(&self.preview_result);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let rendered = draw_preview(&base, &descript_text, &parts, &opts);
            if let Ok(mut slot) = result_slot.lock() {
                *slot = Some(rendered);
            }
            ctx_clone.request_repaint();
        });
    }

    /// バックグラウンドスレッドの完了結果をポーリングしてテクスチャを更新する
    /// update() の先頭で毎フレーム呼ぶ
    pub fn poll_preview_result(&mut self, ctx: &Context) {
        if let Ok(mut slot) = self.preview_result.try_lock() {
            if let Some(rendered) = slot.take() {
                let (w, h) = rendered.dimensions();
                let pixels: Vec<u8> = rendered.pixels().flat_map(|p| p.0).collect();
                let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                self.preview_texture = Some(ctx.load_texture(
                    "preview",
                    color_image,
                    TextureOptions::default(),
                ));
                self.state.preview_generating = false;
            }
        }
    }

    pub fn err(&mut self, msg: impl Into<String>) {
        self.dialog = Some(("エラー".into(), msg.into()));
    }

    /// 素材フォルダをダイアログで選択して読み込む
    pub fn select_asset_folder_dialog(&mut self, ctx: &Context) {
        let start_dir = self.state.selected_asset_dir.clone()
            .unwrap_or_else(|| self.root.clone());
        let picked = rfd::FileDialog::new()
            .set_title("素材フォルダを選択")
            .set_directory(&start_dir)
            .pick_folder();
        if let Some(path) = picked {
            self.state.selected_asset_dir = Some(path);
            self.reload_asset_folder(ctx);
        }
    }

    pub fn reload_asset_folder(&mut self, ctx: &Context) {
        // 素材フォルダを切り替えたとき基本情報もリセットする
        self.state.basic_info.clear();
        match load_asset_folder(&mut self.state, &self.root.clone()) {
            Ok(_) => {
                // descript から基本情報を再構築
                use crate::core::descript::parse_descript;
                let parsed_d = parse_descript(&self.state.descript_text);
                let parsed_i = parse_descript(&self.state.install_text);
                for key in &["name","craftman","craftmanw","craftmanurl","homeurl"] {
                    if let Some(v) = parsed_d.get(*key) {
                        self.state.basic_info.insert(key.to_string(), v.clone());
                    }
                }
                if let Some(v) = parsed_i.get("directory") {
                    self.state.basic_info.insert("directory".to_string(), v.clone());
                }
                self.refresh_preview_texture(ctx);
            }
            Err(e) => self.err(format!("読み込みエラー:\n\n{}", e)),
        }
    }

    /// プロファイル読み込み後に使用: テキスト類を上書きせずにフォルダを再読み込みする
    pub fn reload_asset_folder_keep_texts(&mut self, ctx: &Context) {
        match load_asset_folder_keep_texts(&mut self.state, &self.root.clone()) {
            Ok(_) => {
                // プロファイルのテキストから基本情報を再構築
                use crate::core::descript::parse_descript;
                let parsed_d = parse_descript(&self.state.descript_text);
                let parsed_i = parse_descript(&self.state.install_text);
                self.state.basic_info.clear();
                for key in &["name","craftman","craftmanw","craftmanurl","homeurl"] {
                    if let Some(v) = parsed_d.get(*key) {
                        self.state.basic_info.insert(key.to_string(), v.clone());
                    }
                }
                if let Some(v) = parsed_i.get("directory") {
                    self.state.basic_info.insert("directory".to_string(), v.clone());
                }
                self.refresh_preview_texture(ctx);
            }
            Err(e) => self.err(format!("読み込みエラー:\n\n{}", e)),
        }
    }

    /// バルーンキャッシュを全件再構築してプレビューを更新する（F5・初期値に戻す等で使用）
    pub fn rebuild_and_refresh(&mut self, ctx: &Context) {
        if let Some(asset_dir) = self.state.asset_dir() {
            if let Err(e) = rebuild_balloon_cache(&mut self.state, &asset_dir) {
                self.err(format!("画像の再構築に失敗しました:\n\n{}", e));
            }
        }
        rebuild_dynamic_defaults(&mut self.state);
        self.refresh_preview_texture(ctx);
    }

    /// 選択バルーンのみ再合成してプレビューを更新する（Undo/Redo 後の高速プレビュー用）
    pub fn rebuild_selected_and_refresh(&mut self, ctx: &Context) {
        if let Some(asset_dir) = self.state.asset_dir() {
            if let Err(e) = rebuild_selected_balloon(&mut self.state, &asset_dir) {
                self.err(format!("画像の再構築に失敗しました:\n\n{}", e));
            }
        }
        self.refresh_preview_texture(ctx);
    }

    /// プロファイルをファイルダイアログで保存する
    pub fn save_profile_dialog(&mut self) {
        let path = rfd::FileDialog::new()
            .set_title("プロファイルを保存")
            .add_filter("JSONファイル", &["json"])
            .save_file();
        if let Some(path) = path {
            let profile = build_profile_from_state(&self.state);
            if let Err(e) = crate::core::profile::save_profile(&path, &profile) {
                self.err(format!("プロファイルの保存に失敗しました:\n{}", e));
            }
        }
    }

    /// プロファイルをファイルダイアログで読み込む
    pub fn load_profile_dialog(&mut self, ctx: &Context) {
        let path = rfd::FileDialog::new()
            .set_title("プロファイルを読み込み")
            .add_filter("JSONファイル", &["json"])
            .pick_file();
        if let Some(path) = path {
            match crate::core::profile::load_profile(&path) {
                Ok(profile) => {
                    apply_profile_to_state(&mut self.state, &profile);
                    self.reload_asset_folder_keep_texts(ctx);
                }
                Err(e) => self.err(format!("プロファイルの読み込みに失敗しました:\n{}", e)),
            }
        }
    }

    /// 出力処理
    pub fn export(&mut self) {
        use crate::core::descript::parse_descript;

        let Some(_ad) = self.state.asset_dir() else { return; };

        // 必須項目チェック
        let balloon_name = self.state.basic_info.get("name").map(|s| s.trim().to_string()).unwrap_or_default();
        let directory    = self.state.basic_info.get("directory").map(|s| s.trim().to_string()).unwrap_or_default();
        let mut missing = Vec::new();
        if balloon_name.is_empty()  { missing.push("バルーン名"); }
        if directory.is_empty()     { missing.push("インストール先フォルダ名"); }
        if !missing.is_empty() {
            self.err(format!(
                "以下の項目が入力されていないため出力できません:\n\n{}",
                missing.iter().map(|s| format!("・{}", s)).collect::<Vec<_>>().join("\n")
            ));
            return;
        }

        // インストール先フォルダ名を install.txt の directory キーから取得
        let parsed_i = parse_descript(&self.state.install_text);
        let parsed_d = parse_descript(&self.state.descript_text);
        let dir_name_raw = parsed_i.get("directory")
            .or_else(|| parsed_d.get("directory"))
            .cloned()
            .unwrap_or_else(|| self.state.asset_dir_name().unwrap_or_default());
        // パストラバーサル対策: ファイル名部分のみ使用し "../" などを除去
        let dir_name = std::path::Path::new(&dir_name_raw)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("output")
            .to_string();
        let output_dir = self.root.join("output").join(&dir_name);

        // 出力先が既に存在する場合は確認してから削除
        if output_dir.exists() {
            let confirmed = rfd::MessageDialog::new()
                .set_title("確認")
                .set_description(format!(
                    "出力先フォルダが既に存在します。\n上書きしますか？\n\n{}",
                    output_dir.display()
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show() == rfd::MessageDialogResult::Yes;
            if !confirmed {
                return;
            }
            if let Err(e) = std::fs::remove_dir_all(&output_dir) {
                self.err(format!("出力フォルダの削除に失敗しました:\n{}", e));
                return;
            }
        }
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            self.err(format!("出力フォルダの作成に失敗しました:\n{}", e));
            return;
        }

        // バルーン画像を出力
        for (name, img) in &self.state.balloon_cache {
            let path = output_dir.join(name);
            if let Err(e) = img.save(&path) {
                self.err(format!("{} の保存に失敗しました:\n{}", name, e));
                return;
            }
        }

        // パーツ画像を出力（先頭バルーンのキャッシュを代表として使う）
        if let Some(parts) = self.state.parts_cache.values().next() {
            for (name, img) in parts {
                let path = output_dir.join(name);
                if let Err(e) = img.save(&path) {
                    self.err(format!("{} の保存に失敗しました:\n{}", name, e));
                    return;
                }
            }
        }

        // descript.txt に必須キーを強制書き込み
        let mut descript_out = self.state.descript_text.clone();
        for key in &["use_self_alpha", "use_input_alpha", "overlay_outside_balloon"] {
            descript_out = crate::core::descript::set_descript_value(&descript_out, key, "1");
        }

        // テキストファイルを出力
        let texts = [
            ("descript.txt", descript_out),
            ("install.txt",  self.state.install_text.clone()),
            ("readme.txt",   self.state.readme_text.clone()),
        ];
        for (fname, text) in &texts {
            if !text.is_empty() {
                let path = output_dir.join(fname);
                if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                    self.err(format!("{} の書き込みに失敗しました:\n{}", fname, e));
                    return;
                }
            }
        }

        // 個別設定ファイルを出力
        for (cfg, text) in &self.state.individual_texts.clone() {
            let path = output_dir.join(cfg);
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                self.err(format!("{} の書き込みに失敗しました:\n{}", cfg, e));
                return;
            }
        }

        let out_path = output_dir.clone();
        self.dialog = Some(("出力完了".into(), format!(
            "出力が完了しました。\n\n出力先:\n{}",
            out_path.display()
        )));
        // 出力フォルダをエクスプローラーで開く
        let _ = open::that(&output_dir);
    }
}

impl eframe::App for BalloonEditorApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // バックグラウンドのプレビュー生成結果を毎フレームチェック
        self.poll_preview_result(ctx);

        // ウィンドウタイトルを素材フォルダ名に合わせて更新
        {
            let title = match self.state.asset_dir_name() {
                Some(name) => format!("バルーンエディタ - {}", name),
                None => "バルーンエディタ".to_string(),
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // ダイアログ
        if let Some((title, msg)) = self.dialog.clone() {
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(&msg);
                    if ui.button("OK").clicked() {
                        self.dialog = None;
                    }
                });
        }

        // ウィンドウサイズを毎フレーム記録
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            self.state.window_size = [rect.width(), rect.height()];
        }

        // 単色背景カラーピッカーウィンドウ
        if self.state.show_bg_color_window {
            let (mut sr, mut sg, mut sb) = if let crate::gui::state::CanvasBg::Solid(r,g,b) = self.state.canvas_bg {
                (r, g, b)
            } else { (255, 255, 255) };
            let mut open = true;
            egui::Window::new("背景色")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    let mut col = egui::Color32::from_rgb(sr, sg, sb);
                    if egui::color_picker::color_picker_color32(
                        ui, &mut col, egui::color_picker::Alpha::Opaque,
                    ) {
                        sr = col.r(); sg = col.g(); sb = col.b();
                        self.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                        self.refresh_preview_texture(ctx);
                    }
                });
            if !open { self.state.show_bg_color_window = false; }
        }

        // キーボードショートカット（ctx.input のクロージャ外で ctx を使う処理を行う）
        #[derive(Default)]
        struct Keys { export: bool, undo: bool, redo: bool, refresh: bool }
        let keys = ctx.input(|i| Keys {
            export:  i.key_pressed(egui::Key::E) && i.modifiers.ctrl,
            undo:    i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && !i.modifiers.shift,
            redo:    (i.key_pressed(egui::Key::Y) && i.modifiers.ctrl)
                  || (i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && i.modifiers.shift),
            refresh: i.key_pressed(egui::Key::F5),
        });
        if keys.export  { self.export(); }
        if keys.undo {
            if self.state.undo() { self.rebuild_selected_and_refresh(ctx); }
            else                 { self.refresh_preview_texture(ctx); }
            // TextEdit の内部バッファをリセットして表示を最新の descript に合わせる
            ctx.memory_mut(|m| m.reset_areas());
        }
        if keys.redo {
            if self.state.redo() { self.rebuild_selected_and_refresh(ctx); }
            else                 { self.refresh_preview_texture(ctx); }
            ctx.memory_mut(|m| m.reset_areas());
        }
        if keys.refresh { self.rebuild_and_refresh(ctx); }

        // メニューバー
        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            panels::menubar::show(ui, self, ctx);
        });

        // ツールバー（4色ピッカー＋出力ボタン）
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            panels::toolbar::show(ui, self, ctx);
        });

        // 左ペイン：バルーンリスト
        let left_resp = egui::SidePanel::left("balloon_list")
            .resizable(true)
            .min_width(200.0)
            .default_width(self.state.panel_left_width)
            .show(ctx, |ui| {
                panels::balloon_list::show(ui, self, ctx);
            });
        self.state.panel_left_width = left_resp.response.rect.width();

        // 右ペイン：設定パネル
        let right_resp = egui::SidePanel::right("settings_panel")
            .resizable(true)
            .default_width(self.state.panel_right_width)
            .show(ctx, |ui| {
                panels::settings::show(ui, self, ctx);
            });
        self.state.panel_right_width = right_resp.response.rect.width();

        // 中央：プレビュー
        egui::CentralPanel::default().show(ctx, |ui| {
            panels::preview::show(ui, self, ctx);
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 終了時に状態を保存
        let state_path = self.root.join("state.json");
        let profile = build_profile_from_state(&self.state);
        let _ = crate::core::profile::save_profile(&state_path, &profile);
    }
}

/// Profile → AppState に適用する
fn apply_profile_to_state(state: &mut AppState, profile: &crate::core::profile::Profile) {
    use crate::gui::state::LAYER_DEFS;

    if !profile.asset_dir.is_empty() {
        let p = std::path::PathBuf::from(&profile.asset_dir);
        if p.is_dir() {
            state.selected_asset_dir = Some(p);
        }
    }

    for &(key, _, default) in LAYER_DEFS {
        let color = profile.layer_color(key, default);
        state.layer_colors.insert(key.to_string(), color);
    }

    let default_parts = state.layer_colors.get("parts").copied()
        .unwrap_or(crate::core::color::Rgb(29, 106, 184));
    state.parts_colors = if profile.parts_colors.is_empty() {
        let mut m = std::collections::HashMap::new();
        m.insert("all".to_string(), default_parts);
        m
    } else {
        profile.parts_colors
            .iter()
            .map(|(k, &[r, g, b])| (k.clone(), crate::core::color::Rgb(r, g, b)))
            .collect()
    };

    state.no_balloon_color = profile.no_balloon_color;
    state.bulk_color_mode  = profile.bulk_color_mode;

    if !profile.descript_text.is_empty() { state.descript_text = profile.descript_text.clone(); }
    if !profile.install_text.is_empty()  { state.install_text  = profile.install_text.clone(); }
    state.individual_texts = profile.individual_texts.clone();
    state.basic_info       = profile.basic_info.clone();

    if !profile.selected_balloon.is_empty() {
        state.selected_balloon = profile.selected_balloon.clone();
    }

    state.theme = match profile.theme.as_str() {
        "light"  => crate::gui::state::ThemeMode::Light,
        "dark"   => crate::gui::state::ThemeMode::Dark,
        _        => crate::gui::state::ThemeMode::Light,
    };

    if profile.panel_left_width  > 0.0 { state.panel_left_width  = profile.panel_left_width; }
    if profile.panel_right_width > 0.0 { state.panel_right_width = profile.panel_right_width; }
    if profile.window_size[0]    > 0.0 { state.window_size       = profile.window_size; }
}

/// AppState → Profile に変換する
fn build_profile_from_state(state: &AppState) -> crate::core::profile::Profile {
    crate::core::profile::Profile {
        version:        1,
        asset_dir:      state.selected_asset_dir.as_ref()
                            .and_then(|p| p.to_str())
                            .unwrap_or("")
                            .to_string(),
        layer_colors:   state.layer_colors.iter()
            .map(|(k, &Rgb(r, g, b))| (k.clone(), [r, g, b]))
            .collect(),
        parts_colors:   state.parts_colors.iter()
            .map(|(k, &Rgb(r, g, b))| (k.clone(), [r, g, b]))
            .collect(),
        individual_colors: Default::default(),
        no_balloon_color:  state.no_balloon_color,
        bulk_color_mode:   state.bulk_color_mode,
        descript_text:     state.descript_text.clone(),
        install_text:      state.install_text.clone(),
        individual_texts:  state.individual_texts.clone(),
        basic_info:        state.basic_info.clone(),
        selected_balloon:  state.selected_balloon.clone(),
        theme: match state.theme {
            crate::gui::state::ThemeMode::Light  => "light".to_string(),
            crate::gui::state::ThemeMode::Dark   => "dark".to_string(),
        },
        panel_left_width:  state.panel_left_width,
        panel_right_width: state.panel_right_width,
        window_size:       state.window_size,
    }
}

// Rgb を分解するためのパターンマッチを使えるようにするための import
use crate::core::color::Rgb;

/// ThemeMode を egui Visuals に変換して適用する
pub fn apply_theme(ctx: &egui::Context, theme: crate::gui::state::ThemeMode) {
    use crate::gui::state::ThemeMode;
    if matches!(theme, ThemeMode::Dark) {
        ctx.set_visuals(egui::Visuals::dark());
    } else {
        ctx.set_visuals(egui::Visuals::light());
    }
}

/// 同梱フォント（BIZ UDPGothic）を egui に設定する
fn setup_japanese_font(ctx: &egui::Context) {
    static FONT_BYTES: &[u8] =
        include_bytes!("../../resource/font/BIZUDPGothic-Regular.ttf");

    let mut fonts = egui::FontDefinitions::default();

    let mut font_data = egui::FontData::from_static(FONT_BYTES);
    font_data.tweak.y_offset_factor = 0.0;

    fonts.font_data.insert("bizud".to_owned(), font_data.into());

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families
            .entry(family)
            .or_default()
            .push("bizud".to_owned());
    }

    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles.iter_mut().for_each(|(_, font_id)| {
        font_id.size *= 1.15;
    });
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    ctx.set_style(style);
}
