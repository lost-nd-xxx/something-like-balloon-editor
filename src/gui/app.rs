/// メインウィンドウ（eframe::App の実装）
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::RgbaImage;
use crate::gui::{
    loader::{load_asset_folder, load_asset_folder_keep_texts,
             rebuild_balloon_cache, rebuild_selected_balloon, ensure_balloon_cached,
             rebuild_dynamic_defaults, build_all_balloons_for_export},
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
        // PNG一覧プレビュー中は専用キーの画像を優先
        if self.state.png_preview_name.is_some() {
            if let Some(img) = self.state.balloon_cache.get("\x00png_preview") {
                return Some(img);
            }
        }
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
        // PNG一覧プレビュー中は専用画像をそのままテクスチャ化（描画処理なし）
        if self.state.png_preview_name.is_some() {
            if let Some(img) = self.state.balloon_cache.get("\x00png_preview") {
                let (w, h) = img.dimensions();
                let pixels: Vec<u8> = img.pixels().flat_map(|p| p.0).collect();
                let color_image = ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize], &pixels
                );
                self.preview_texture = Some(ctx.load_texture(
                    "preview",
                    color_image,
                    TextureOptions::NEAREST,
                ));
            }
            return;
        }

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
        // 個別設定と共通設定をマージ（個別設定が優先、差分ファイル仕様に対応）
        // 共通設定をベースに個別設定のキーを上書きしたテキストを生成する
        let descript_text = if let Some(indiv) = self.state.individual_texts.get(&cfg_key) {
            let indiv_parsed = crate::core::descript::parse_descript(indiv);
            let mut merged = self.state.descript_text.clone();
            for (k, v) in &indiv_parsed {
                merged = crate::core::descript::set_descript_value(&merged, k, v);
            }
            merged
        } else {
            self.state.descript_text.clone()
        };

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
                    TextureOptions::NEAREST,
                ));
                self.state.preview_generating = false;
            }
        }
    }

    pub fn err(&mut self, msg: impl Into<String>) {
        self.dialog = Some(("エラー".into(), msg.into()));
    }

    pub fn reload_asset_folder(&mut self, ctx: &Context) {
        // 素材フォルダを切り替えたとき基本情報もリセットする
        self.state.basic_info.clear();
        match load_asset_folder(&mut self.state, &self.root.clone()) {
            Ok(_) => {
                // プロジェクトフォルダなら profile.json があれば色設定を自動復元
                // （テキスト類はフォルダから読んだものを優先するため上書きしない）
                if self.state.is_project_dir() {
                    if let Some(asset_dir) = self.state.asset_dir() {
                        let profile_path = asset_dir.join("profile.json");
                        if let Ok(profile) = crate::core::profile::load_profile(&profile_path) {
                            apply_project_profile_to_state(&mut self.state, &profile);
                        }
                    }
                }

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
                self.flush_load_warnings();
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
                self.flush_load_warnings();
            }
            Err(e) => self.err(format!("読み込みエラー:\n\n{}", e)),
        }
    }

    /// PNG一覧から単一ファイルをプレビュー表示する（selected_balloon は変更しない）
    pub fn preview_single_png(&mut self, name: &str, ctx: &Context) {
        let Some(asset_dir) = self.state.asset_dir() else { return };
        let path = asset_dir.join(name);
        if !path.exists() { return; }
        match crate::core::composer::open_png_rgba(&path) {
            Ok(img) => {
                const PREVIEW_KEY: &str = "\x00png_preview";
                self.state.balloon_cache.insert(PREVIEW_KEY.to_string(), img);
                self.state.png_preview_name = Some(name.to_string());
                // バルーンリストのハイライトを消す・位置編集を終了する
                self.state.selected_balloon = String::new();
                self.state.drag_edit_target = None;
                self.refresh_preview_texture_silent(ctx);
            }
            Err(e) => self.err(format!("画像の読み込みに失敗しました:\n{}", e)),
        }
    }

    /// PNG ファイルを名前変更し、files.txt 内の参照も更新する
    /// files.txt ありモードでは物理ファイルが存在しないバルーンも対象のため、
    /// 物理ファイルが存在する場合のみリネームし、files.txt は常に更新する。
    pub fn rename_png(&mut self, old_name: &str, new_stem: &str, ctx: &Context) {
        let Some(asset_dir) = self.state.asset_dir() else { return };
        let old_stem = old_name.trim_end_matches(".png");
        let new_name = format!("{}.png", new_stem);

        let old_path = asset_dir.join(old_name);
        let new_path = asset_dir.join(&new_name);

        // 物理ファイルが存在する場合のみリネーム
        if old_path.exists() {
            if let Err(e) = std::fs::rename(&old_path, &new_path) {
                self.err(format!("ファイルの名前変更に失敗しました:\n{}", e));
                return;
            }
            // .pna ファイルが存在すれば合わせてリネーム
            let old_pna = asset_dir.join(format!("{}.pna", old_stem));
            let new_pna = asset_dir.join(format!("{}.pna", new_stem));
            if old_pna.exists() {
                let _ = std::fs::rename(&old_pna, &new_pna);
            }
        }

        // files.txt 内の参照を更新（ありモード・なしモード共通）
        let files_txt = asset_dir.join("files.txt");
        if files_txt.exists() {
            if let Ok(content) = std::fs::read_to_string(&files_txt) {
                let updated = content
                    .lines()
                    .map(|line| {
                        line.split(',')
                            .map(|part| {
                                let trimmed = part.trim();
                                if trimmed == old_stem {
                                    part.replacen(trimmed, new_stem, 1)
                                } else {
                                    part.to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = std::fs::write(&files_txt, updated);
            }
        }

        // 選択中バルーンが変更対象だった場合は新名前に更新
        if self.state.selected_balloon == old_name {
            self.state.selected_balloon = new_name.clone();
        }

        self.reload_asset_folder_keep_texts(ctx);
    }

    /// PNG ファイルの削除確認ダイアログを表示して実行する
    pub fn request_delete_png(&mut self, name: &str) {
        let Some(asset_dir) = self.state.asset_dir() else { return };
        let path = asset_dir.join(name);
        let stem = name.trim_end_matches(".png");
        let pna_path = asset_dir.join(format!("{}.pna", stem));
        let has_pna = pna_path.exists();

        let desc = if has_pna {
            format!("「{}」および「{}.pna」をゴミ箱に移動しますか？", name, stem)
        } else {
            format!("「{}」をゴミ箱に移動しますか？", name)
        };

        let confirmed = rfd::MessageDialog::new()
            .set_title("削除確認")
            .set_description(desc)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show() == rfd::MessageDialogResult::Yes;
        if confirmed {
            if let Err(e) = trash::delete(&path) {
                self.err(format!("削除に失敗しました:\n{}", e));
                return;
            }
            if has_pna {
                if let Err(e) = trash::delete(&pna_path) {
                    self.err(format!("{}.pna の削除に失敗しました:\n{}", stem, e));
                }
            }
            // 削除後は next_reload フラグを立てておく（ctx がここでは使えないため）
            self.state.pending_reload = true;
        }
    }

    /// load_warnings が溜まっていればダイアログに表示してクリアする
    fn flush_load_warnings(&mut self) {
        if self.state.load_warnings.is_empty() { return; }
        let msg = self.state.load_warnings.join("\n\n");
        self.state.load_warnings.clear();
        self.dialog = Some(("文字コード警告".into(), msg));
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
    /// プロジェクトに保存（descript.txt / install.txt の書き出し + profile.json の保存）
    pub fn save_project(&mut self) {
        let Some(asset_dir) = self.state.asset_dir() else {
            self.err("プロジェクトが選択されていません。");
            return;
        };
        if !self.state.is_project_dir() {
            self.err("現在開いているフォルダはプロジェクトではありません。");
            return;
        }

        // descript.txt / install.txt を書き出す
        let descript_path = asset_dir.join("descript.txt");
        let install_path  = asset_dir.join("install.txt");
        if let Err(e) = std::fs::write(&descript_path, &self.state.descript_text) {
            self.err(format!("descript.txt の保存に失敗しました:\n{}", e));
            return;
        }
        if let Err(e) = std::fs::write(&install_path, &self.state.install_text) {
            self.err(format!("install.txt の保存に失敗しました:\n{}", e));
            return;
        }

        // profile.json を保存
        let profile_path = asset_dir.join("profile.json");
        let profile = build_profile_from_state(&self.state);
        if let Err(e) = crate::core::profile::save_profile(&profile_path, &profile) {
            self.err(format!("プロファイルの保存に失敗しました:\n{}", e));
        }
    }

    /// 現在のプロジェクトを別名のプロジェクトとしてコピー保存する
    pub fn save_project_as(&mut self, new_name: &str, ctx: &Context) {
        let Some(asset_dir) = self.state.asset_dir() else {
            self.err("プロジェクトが選択されていません。");
            return;
        };
        if !self.state.is_project_dir() {
            self.err("現在開いているフォルダはプロジェクトではありません。");
            return;
        }

        // まず現在のプロジェクトを上書き保存してから別名コピー
        self.save_project();

        match crate::core::project::create_project_from_folder(&asset_dir, new_name) {
            Ok(dir) => {
                self.state.selected_asset_dir = Some(dir);
                self.reload_asset_folder(ctx);
            }
            Err(e) => {
                self.err(format!("別名で保存エラー: {}", e));
            }
        }
    }

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

        let Some(_ad) = self.state.asset_dir() else {
            self.err("素材フォルダが選択されていません。\n\nメニュー「ファイル > 素材フォルダを選択…」から素材フォルダを指定してください。".to_string());
            return;
        };

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

        // バルーン画像を出力（個別色を考慮して全件合成）
        let ad = self.state.asset_dir().unwrap();
        let balloon_images = match build_all_balloons_for_export(&self.state, &ad) {
            Ok(imgs) => imgs,
            Err(e) => {
                self.err(format!("バルーン画像の合成に失敗しました:\n{}", e));
                return;
            }
        };
        for (name, img) in &balloon_images {
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

    /// インポートキュー内の現在インデックスにあるファイルからUI入力値を自動推測してプレセットします。
    pub fn preset_import_from_current_queue(&mut self) {
        let idx = self.state.import_queue_index;
        if idx >= self.state.import_queue.len() {
            return;
        }
        let path = &self.state.import_queue[idx].clone();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();

        // .pna 検出（情報表示用）
        self.state.import_has_pna = path.with_extension("pna").exists();

        // 命名パターンの自動解析・プレセット
        if stem.starts_with("balloons") {
            self.state.import_category = "balloon".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 0;
            self.state.import_target_id_num = stem.strip_prefix("balloons").unwrap_or("0").parse().unwrap_or(0);
        } else if stem.starts_with("balloonk") {
            self.state.import_category = "balloon".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 1;
            self.state.import_target_id_num = stem.strip_prefix("balloonk").unwrap_or("0").parse().unwrap_or(0);
        } else if stem.starts_with("balloonc") {
            self.state.import_category = "balloonc".to_string();
            let id_str = stem.strip_prefix("balloonc").unwrap_or("0");
            self.state.import_balloonc_id = id_str.parse().unwrap_or(0).min(4);
        } else if stem.starts_with("balloonp") && stem.contains("def") {
            self.state.import_category = "balloon".to_string();
            self.state.import_is_scope_specific = true;
            let parts: Vec<&str> = stem.splitn(2, "def").collect();
            if parts.len() == 2 {
                self.state.import_scope_num = parts[0].strip_prefix("balloonp").unwrap_or("2").parse().unwrap_or(2);
                self.state.import_target_id_num = parts[1].parse().unwrap_or(0);
            }
        } else if stem.starts_with("arrows") {
            self.state.import_category = "arrow".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 0;
            self.state.import_arrow_dir = stem.strip_prefix("arrows").unwrap_or("0").parse().unwrap_or(0).min(1);
        } else if stem.starts_with("arrowk") {
            self.state.import_category = "arrow".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 1;
            self.state.import_arrow_dir = stem.strip_prefix("arrowk").unwrap_or("0").parse().unwrap_or(0).min(1);
        } else if stem.starts_with("arrowp") && stem.contains("def") {
            self.state.import_category = "arrow".to_string();
            self.state.import_is_scope_specific = true;
            let parts: Vec<&str> = stem.splitn(2, "def").collect();
            if parts.len() == 2 {
                self.state.import_scope_num = parts[0].strip_prefix("arrowp").unwrap_or("2").parse().unwrap_or(2);
                self.state.import_arrow_dir = parts[1].parse().unwrap_or(0).min(1);
            }
        } else if stem.starts_with("arrow") {
            self.state.import_category = "arrow".to_string();
            self.state.import_is_scope_specific = false;
            self.state.import_arrow_dir = stem.strip_prefix("arrow").unwrap_or("0").parse().unwrap_or(0).min(1);
        } else if stem.starts_with("clickwaits") {
            self.state.import_category = "clickwait".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 0;
        } else if stem.starts_with("clickwaitk") {
            self.state.import_category = "clickwait".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 1;
        } else if stem.starts_with("clickwaitp") && stem.contains("def") {
            self.state.import_category = "clickwait".to_string();
            self.state.import_is_scope_specific = true;
            let parts: Vec<&str> = stem.splitn(2, "def").collect();
            self.state.import_scope_num = parts[0].strip_prefix("clickwaitp").unwrap_or("2").parse().unwrap_or(2);
        } else if stem == "clickwait" {
            self.state.import_category = "clickwait".to_string();
            self.state.import_is_scope_specific = false;
        } else if stem.starts_with("markers") {
            self.state.import_category = "marker".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 0;
        } else if stem.starts_with("markerk") {
            self.state.import_category = "marker".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 1;
        } else if stem.starts_with("markerp") && stem.contains("def") {
            self.state.import_category = "marker".to_string();
            self.state.import_is_scope_specific = true;
            let parts: Vec<&str> = stem.splitn(2, "def").collect();
            self.state.import_scope_num = parts[0].strip_prefix("markerp").unwrap_or("2").parse().unwrap_or(2);
        } else if stem == "marker" {
            self.state.import_category = "marker".to_string();
            self.state.import_is_scope_specific = false;
        } else if stem == "sstp_new" {
            self.state.import_category = "sstp".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 0;
        } else if stem == "sstp_newk" {
            self.state.import_category = "sstp".to_string();
            self.state.import_is_scope_specific = true;
            self.state.import_scope_num = 1;
        } else if stem.starts_with("sstp_newp") && stem.ends_with("def") {
            self.state.import_category = "sstp".to_string();
            self.state.import_is_scope_specific = true;
            let p_part = stem.strip_prefix("sstp_newp").unwrap_or("2").strip_suffix("def").unwrap_or("2");
            self.state.import_scope_num = p_part.parse().unwrap_or(2);
        } else if stem == "sstp" {
            self.state.import_category = "sstp".to_string();
            self.state.import_is_scope_specific = false;
        } else if stem.starts_with("online") {
            self.state.import_category = "online".to_string();
            self.state.import_target_id_num = stem.strip_prefix("online").unwrap_or("0").parse().unwrap_or(0);
        } else if stem == "thumbnail" {
            self.state.import_category = "thumbnail".to_string();
        } else if let Some(rest) = stem.strip_prefix("ok_") {
            self.state.import_category = "inputbtn".to_string();
            self.state.import_inputbtn_kind = "ok".to_string();
            self.state.import_inputbtn_state = rest.to_string();
        } else if let Some(rest) = stem.strip_prefix("cancel_") {
            self.state.import_category = "inputbtn".to_string();
            self.state.import_inputbtn_kind = "cancel".to_string();
            self.state.import_inputbtn_state = rest.to_string();
        } else if let Some(rest) = stem.strip_prefix("mode_") {
            self.state.import_category = "inputbtn".to_string();
            self.state.import_inputbtn_kind = "mode".to_string();
            self.state.import_inputbtn_state = rest.to_string();
        } else {
            self.state.import_category = "free".to_string();
            self.state.import_is_scope_specific = false;
            self.state.import_custom_filename = filename.clone();
            self.state.import_target_id_num = 0;
        }
    }

    /// 各種UI入力パラメータから、最終保存用のファイル名を組み立てて返します。
    pub fn get_import_target_filename(&self) -> String {
        let cat = &self.state.import_category;
        let is_spec = self.state.import_is_scope_specific;
        let scope = self.state.import_scope_num;
        let id = self.state.import_target_id_num;
        let dir = self.state.import_arrow_dir;

        match cat.as_str() {
            "free" => self.state.import_custom_filename.clone(),
            "thumbnail" => "thumbnail.png".to_string(),
            "online" => format!("online{}.png", id),
            "inputbtn" => format!("{}_{}.png",
                self.state.import_inputbtn_kind,
                self.state.import_inputbtn_state),
            "balloon" => {
                if scope == 0 {
                    format!("balloons{}.png", id)
                } else if scope == 1 {
                    format!("balloonk{}.png", id)
                } else {
                    format!("balloonp{}def{}.png", scope, id)
                }
            }
            "balloonc" => format!("balloonc{}.png", self.state.import_balloonc_id),
            "arrow" => {
                if !is_spec {
                    format!("arrow{}.png", dir)
                } else if scope == 0 {
                    format!("arrows{}.png", dir)
                } else if scope == 1 {
                    format!("arrowk{}.png", dir)
                } else {
                    format!("arrowp{}def{}.png", scope, dir)
                }
            }
            "clickwait" => {
                if !is_spec {
                    "clickwait.png".to_string()
                } else if scope == 0 {
                    "clickwaits.png".to_string()
                } else if scope == 1 {
                    "clickwaitk.png".to_string()
                } else {
                    format!("clickwaitp{}def.png", scope)
                }
            }
            "marker" => {
                if !is_spec {
                    "marker.png".to_string()
                } else if scope == 0 {
                    "markers.png".to_string()
                } else if scope == 1 {
                    "markerk.png".to_string()
                } else {
                    format!("markerp{}def.png", scope)
                }
            }
            "sstp" => {
                if !is_spec {
                    "sstp.png".to_string()
                } else if scope == 0 {
                    "sstp_new.png".to_string()
                } else if scope == 1 {
                    "sstp_newk.png".to_string()
                } else {
                    format!("sstp_newp{}def.png", scope)
                }
            }
            _ => "unknown.png".to_string(),
        }
    }
}

impl eframe::App for BalloonEditorApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 削除後などの遅延リロード
        if self.state.pending_reload {
            self.state.pending_reload = false;
            self.reload_asset_folder_keep_texts(ctx);
        }

        // ドラッグ＆ドロップされたファイルの捕捉
        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped_files.is_empty() {
            let png_files: Vec<std::path::PathBuf> = dropped_files
                .into_iter()
                .filter_map(|f| f.path)
                .filter(|p| p.extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase()) == Some("png".to_string()))
                .collect();
            
            if !png_files.is_empty() {
                if self.state.asset_dir().is_none() {
                    self.err("プロジェクトが選択されていません。\n先にプロジェクトを選択または作成してください。");
                } else if !self.state.is_project_dir() {
                    self.err("現在開いているフォルダはプロジェクトではありません。\n画像をインポートするには、先にプロジェクトを作成してください。");
                } else {
                    self.state.import_queue = png_files;
                    self.state.import_queue_index = 0;
                    self.state.show_import_window = true;
                    self.preset_import_from_current_queue();
                }
            }
        }

        // バックグラウンドのプレビュー生成結果を毎フレームチェック
        self.poll_preview_result(ctx);

        // ウィンドウタイトルを素材フォルダ名に合わせて更新
        {
            let title = match self.state.asset_dir_name() {
                Some(name) => format!("{} - {}", crate::gui::state::APP_NAME, name),
                None => crate::gui::state::APP_NAME.to_string(),
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

        // 新規プロジェクト作成モーダル
        // プロジェクトを開くダイアログ
        if self.state.show_open_project_window {
            let mut close = false;
            let mut open_name: Option<String> = None;

            egui::Window::new("プロジェクトを開く")
                .collapsible(false)
                .resizable(false)
                .fixed_size([300.0, 310.0])
                .show(ctx, |ui| {
                    if self.state.open_project_list.is_empty() {
                        ui.label("プロジェクトがありません。");
                        ui.label("「新規プロジェクト」または「フォルダからプロジェクトを作成」で作成してください。");
                    } else {
                        // フィルタ入力欄
                        ui.horizontal(|ui| {
                            ui.label("絞り込み:");
                            let filter_res = ui.add(
                                egui::TextEdit::singleline(&mut self.state.open_project_filter)
                                    .desired_width(f32::INFINITY)
                            );
                            // フィルタが変わったら選択をリセット
                            if filter_res.changed() {
                                self.state.open_project_selected = None;
                            }
                        });
                        ui.add_space(4.0);

                        // フィルタ後リスト（大文字小文字無視）
                        let filter_lower = self.state.open_project_filter.to_lowercase();
                        let filtered: Vec<&String> = self.state.open_project_list.iter()
                            .filter(|n| filter_lower.is_empty() || n.to_lowercase().contains(&filter_lower))
                            .collect();

                        egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            for (i, name) in filtered.iter().enumerate() {
                                let selected = self.state.open_project_selected == Some(i);
                                if ui.selectable_label(selected, *name).clicked() {
                                    self.state.open_project_selected = Some(i);
                                }
                            }
                        });

                        // 件数表示
                        ui.add_space(2.0);
                        let total = self.state.open_project_list.len();
                        let shown = filtered.len();
                        if total != shown {
                            ui.label(format!("{} / {} 件", shown, total));
                        } else {
                            ui.label(format!("{} 件", total));
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can_open = self.state.open_project_selected.is_some();
                        if ui.add_enabled(can_open, egui::Button::new("開く")).clicked() {
                            if let Some(idx) = self.state.open_project_selected {
                                // フィルタ後リストから実際の名前を引く
                                let filter_lower = self.state.open_project_filter.to_lowercase();
                                let filtered_name = self.state.open_project_list.iter()
                                    .filter(|n| filter_lower.is_empty() || n.to_lowercase().contains(&filter_lower))
                                    .nth(idx)
                                    .cloned();
                                open_name = filtered_name;
                            }
                            close = true;
                        }
                        if ui.button("キャンセル").clicked() {
                            close = true;
                        }
                    });
                });

            if let Some(name) = open_name {
                if let Ok(dir) = crate::core::project::get_project_dir(&name) {
                    self.state.selected_asset_dir = Some(dir);
                    self.reload_asset_folder(ctx);
                }
            }
            if close {
                self.state.show_open_project_window = false;
                self.state.open_project_selected = None;
                self.state.open_project_filter.clear();
            }
        }

        if self.state.show_new_project_window {
            let mut close = false;
            egui::Window::new("新規プロジェクト作成")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("新規プロジェクトの名前を入力してください。\n(プロジェクトフォルダが projects/ 配下に作成されます)");
                    ui.add_space(4.0);

                    let placeholder = self.state.get_install_directory_name()
                        .unwrap_or_else(|| "new_balloon".to_string());
                    
                    ui.horizontal(|ui| {
                        ui.label("プロジェクト名:");
                        let resp = ui.text_edit_singleline(&mut self.state.new_project_name);
                        if self.state.new_project_name.is_empty() && resp.gained_focus() {
                            self.state.new_project_name = placeholder.clone();
                        }
                    });

                    let name_trimmed = self.state.new_project_name.trim().to_string();
                    let exists = if !name_trimmed.is_empty() {
                        crate::core::project::project_exists(&name_trimmed)
                    } else {
                        false
                    };

                    if exists {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "[!] 既に存在するプロジェクト名です。保存すると上書きされます。");
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("作成").clicked() {
                            if name_trimmed.is_empty() {
                                self.state.new_project_warning = "プロジェクト名を入力してください。".to_string();
                            } else {
                                let proceed = if exists {
                                    rfd::MessageDialog::new()
                                        .set_title("上書き確認")
                                        .set_description(format!("プロジェクト「{}」は既に存在します。\n既存データをバックアップ退避した上で上書き作成しますか？\n(失敗時は自動で復元されます)", name_trimmed))
                                        .set_buttons(rfd::MessageButtons::YesNo)
                                        .show() == rfd::MessageDialogResult::Yes
                                } else {
                                    true
                                };

                                if proceed {
                                    match crate::core::project::create_project_safe(&name_trimmed) {
                                        Ok(dir) => {
                                            self.state.selected_asset_dir = Some(dir);
                                            self.reload_asset_folder(ctx);
                                            close = true;
                                        }
                                        Err(e) => {
                                            self.err(format!("新規作成エラー: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        if ui.button("キャンセル").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.state.show_new_project_window = false;
                self.state.new_project_name.clear();
            }
        }

        // フォルダからプロジェクトを作成ダイアログ
        if self.state.show_import_folder_window {
            let mut close = false;
            let src = self.state.import_folder_src.clone();
            let src_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();

            egui::Window::new("フォルダからプロジェクトを作成")
                .collapsible(false)
                .resizable(false)
                .fixed_size([380.0, 180.0])
                .show(ctx, |ui| {
                    ui.label(format!("取り込み元: {}", src_name));
                    ui.add_space(6.0);
                    ui.label("プロジェクト名:");
                    ui.text_edit_singleline(&mut self.state.import_folder_project_name);

                    // リアルタイム重複チェック
                    let name_trimmed = self.state.import_folder_project_name.trim().to_string();
                    let already_exists = crate::core::project::project_exists(&name_trimmed);
                    if name_trimmed.is_empty() {
                        self.state.import_folder_warning = String::new();
                    } else if already_exists {
                        self.state.import_folder_warning = "[!] 既に存在するプロジェクト名です。作成すると上書きされます。".to_string();
                    } else {
                        self.state.import_folder_warning = String::new();
                    }

                    if !self.state.import_folder_warning.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(220, 160, 0), &self.state.import_folder_warning.clone());
                    } else {
                        ui.label("");
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can_create = !name_trimmed.is_empty();
                        if ui.add_enabled(can_create, egui::Button::new("作成")).clicked() {
                            let proceed = if already_exists {
                                rfd::MessageDialog::new()
                                    .set_title("上書き確認")
                                    .set_description(format!("プロジェクト「{}」は既に存在します。\n既存データをバックアップ退避した上で上書き作成しますか？", name_trimmed))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show() == rfd::MessageDialogResult::Yes
                            } else {
                                true
                            };
                            if proceed {
                                match crate::core::project::create_project_from_folder(&src, &name_trimmed) {
                                    Ok(dir) => {
                                        self.state.selected_asset_dir = Some(dir);
                                        self.reload_asset_folder(ctx);
                                        close = true;
                                    }
                                    Err(e) => {
                                        self.err(format!("プロジェクト作成エラー: {}", e));
                                    }
                                }
                            }
                        }
                        if ui.button("キャンセル").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.state.show_import_folder_window = false;
                self.state.import_folder_project_name.clear();
                self.state.import_folder_warning.clear();
            }
        }

        // 別名で保存ダイアログ
        if self.state.show_save_as_project_window {
            let mut close = false;
            let current_name = self.state.asset_dir()
                .and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_default();

            egui::Window::new("別名で保存")
                .collapsible(false)
                .resizable(false)
                .fixed_size([380.0, 160.0])
                .show(ctx, |ui| {
                    ui.label(format!("現在のプロジェクト: {}", current_name));
                    ui.add_space(6.0);
                    ui.label("新しいプロジェクト名:");
                    ui.text_edit_singleline(&mut self.state.save_as_project_name);

                    let name_trimmed = self.state.save_as_project_name.trim().to_string();
                    let already_exists = crate::core::project::project_exists(&name_trimmed);
                    let is_same = name_trimmed == current_name;
                    if name_trimmed.is_empty() || is_same {
                        self.state.save_as_project_warning = String::new();
                    } else if already_exists {
                        self.state.save_as_project_warning = "[!] 既に存在するプロジェクト名です。上書きされます。".to_string();
                    } else {
                        self.state.save_as_project_warning = String::new();
                    }

                    if !self.state.save_as_project_warning.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(220, 160, 0), &self.state.save_as_project_warning.clone());
                    } else {
                        ui.label("");
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let can_save = !name_trimmed.is_empty() && !is_same;
                        if ui.add_enabled(can_save, egui::Button::new("保存")).clicked() {
                            let proceed = if already_exists {
                                rfd::MessageDialog::new()
                                    .set_title("上書き確認")
                                    .set_description(format!("プロジェクト「{}」は既に存在します。\n上書きしますか？", name_trimmed))
                                    .set_buttons(rfd::MessageButtons::YesNo)
                                    .show() == rfd::MessageDialogResult::Yes
                            } else {
                                true
                            };
                            if proceed {
                                let name = name_trimmed.clone();
                                self.save_project_as(&name, ctx);
                                close = true;
                            }
                        }
                        if ui.button("キャンセル").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.state.show_save_as_project_window = false;
                self.state.save_as_project_name.clear();
                self.state.save_as_project_warning.clear();
            }
        }

        // ファイル名変更ダイアログ
        if self.state.show_rename_window {
            let mut close = false;
            let target = self.state.rename_target.clone();
            let old_stem = target.trim_end_matches(".png").to_string();

            egui::Window::new("名前変更")
                .collapsible(false)
                .resizable(false)
                .fixed_size([340.0, 150.0])
                .show(ctx, |ui| {
                    ui.label(format!("変更前: {}", old_stem));
                    ui.add_space(4.0);
                    ui.label("新しい名前:");
                    ui.text_edit_singleline(&mut self.state.rename_new_name);

                    let new_stem = self.state.rename_new_name.trim().to_string();
                    let new_name = format!("{}.png", new_stem);
                    let is_same = new_stem == old_stem;
                    let is_empty = new_stem.is_empty();
                    let already_exists = if !is_same && !is_empty {
                        self.state.asset_dir()
                            .map(|d| d.join(&new_name).exists())
                            .unwrap_or(false)
                    } else { false };

                    self.state.rename_warning = if is_empty || is_same {
                        String::new()
                    } else if already_exists {
                        "[!] 同名のファイルが既に存在します。".to_string()
                    } else {
                        String::new()
                    };

                    if !self.state.rename_warning.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), &self.state.rename_warning.clone());
                    } else {
                        ui.label("");
                    }

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let can_rename = !is_empty && !is_same && !already_exists;
                        if ui.add_enabled(can_rename, egui::Button::new("変更")).clicked() {
                            let new_stem = new_stem.clone();
                            self.rename_png(&target, &new_stem, ctx);
                            close = true;
                        }
                        if ui.button("キャンセル").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.state.show_rename_window = false;
                self.state.rename_target.clear();
                self.state.rename_new_name.clear();
                self.state.rename_warning.clear();
            }
        }

        // 画像インポート設定ダイアログ
        if self.state.show_import_window {
            let idx = self.state.import_queue_index;
            if idx >= self.state.import_queue.len() {
                self.state.show_import_window = false;
                self.state.import_queue.clear();
                self.state.import_queue_index = 0;
            } else {
                let mut close = false;
                let mut next = false;
                let path = self.state.import_queue[idx].clone();
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                egui::Window::new("画像インポート設定")
                    .collapsible(false)
                    .resizable(false)
                    .fixed_size([520.0, 360.0])
                    .show(ctx, |ui| {
                        ui.colored_label(egui::Color32::from_rgb(29, 106, 184), format!("インポート画像 ({} / {} 枚目):", idx + 1, self.state.import_queue.len()));
                        ui.label(format!("ファイル名: {}", filename));
                        ui.add_space(6.0);

                        // カテゴリ選択（左）＋ パラメータ（右）の左右分割レイアウト
                        ui.horizontal(|ui| {
                            // 左側: カテゴリ縦リスト
                            ui.vertical(|ui| {
                                ui.set_min_width(160.0);
                                ui.label("カテゴリ:");
                                for (key, label) in &[
                                    // 基本
                                    ("balloon",   "バルーン"),
                                    ("balloonc",  "入力ボックス"),
                                    ("arrow",     "矢印"),
                                    ("online",    "オンライン状態"),
                                    ("sstp",      "SSTP"),
                                    ("thumbnail", "サムネイル"),
                                    // SSP拡張
                                    ("inputbtn",  "入力ボックスのボタン *"),
                                    ("marker",    "マーカー *"),
                                    ("clickwait", "クリック待ち *"),
                                    // その他
                                    ("free",      "自由入力"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.state.import_category,
                                        key.to_string(),
                                        *label,
                                    );
                                }
                                ui.add_space(4.0);
                                ui.small("* SSP拡張");
                            });

                            ui.separator();

                            // 右側: 選択カテゴリのパラメータ
                            let cat = self.state.import_category.clone();
                            ui.vertical(|ui| {
                                ui.set_min_width(220.0);
                                match cat.as_str() {
                                    "free" => {
                                        ui.label("保存ファイル名:");
                                        ui.add(egui::TextEdit::singleline(&mut self.state.import_custom_filename)
                                            .desired_width(180.0));
                                    }
                                    "thumbnail" => {
                                        ui.label("thumbnail.png として保存されます。");
                                    }
                                    "online" => {
                                        ui.horizontal(|ui| {
                                            ui.label("ID番号:");
                                            ui.add(egui::DragValue::new(&mut self.state.import_target_id_num).range(0..=999));
                                        });
                                    }
                                    "inputbtn" => {
                                        ui.label("ボタン種別:");
                                        ui.radio_value(&mut self.state.import_inputbtn_kind, "ok".to_string(), "OK");
                                        ui.radio_value(&mut self.state.import_inputbtn_kind, "cancel".to_string(), "キャンセル");
                                        ui.radio_value(&mut self.state.import_inputbtn_kind, "mode".to_string(), "モード切替 (Address Bar用)");
                                        ui.add_space(4.0);
                                        ui.label("状態:");
                                        ui.radio_value(&mut self.state.import_inputbtn_state, "up".to_string(), "通常 (up)");
                                        ui.radio_value(&mut self.state.import_inputbtn_state, "down".to_string(), "押下 (down)");
                                    }
                                    "balloonc" => {
                                        ui.horizontal(|ui| {
                                            ui.label("ID番号:");
                                            egui::ComboBox::from_id_salt("balloonc_id")
                                                .selected_text(self.state.import_balloonc_id.to_string())
                                                .show_ui(ui, |ui| {
                                                    for (i, desc) in [
                                                        "0: Send ボックス",
                                                        "1: Communicate ボックス",
                                                        "2: Teach ボックス",
                                                        "3: Input ボックス",
                                                        "4: Address Bar (SSP専用)",
                                                    ].iter().enumerate() {
                                                        ui.selectable_value(&mut self.state.import_balloonc_id, i, *desc);
                                                    }
                                                });
                                        });
                                    }
                                    _ => {
                                        // balloon / arrow / clickwait / marker / sstp
                                        let can_be_generic = matches!(cat.as_str(), "arrow" | "clickwait" | "marker" | "sstp");
                                        if can_be_generic {
                                            ui.checkbox(&mut self.state.import_is_scope_specific, "スコープ専用 *");
                                        } else {
                                            self.state.import_is_scope_specific = true;
                                        }

                                        if self.state.import_is_scope_specific {
                                            ui.horizontal(|ui| {
                                                ui.label("スコープ番号:");
                                                ui.add(egui::DragValue::new(&mut self.state.import_scope_num).range(0..=99));
                                                let n = self.state.import_scope_num;
                                                ui.small(format!("({}人目)", n + 1));
                                            });
                                        }

                                        if cat == "arrow" {
                                            ui.horizontal(|ui| {
                                                ui.label("向き:");
                                                ui.radio_value(&mut self.state.import_arrow_dir, 0u8, "上 (0)");
                                                ui.radio_value(&mut self.state.import_arrow_dir, 1u8, "下 (1)");
                                            });
                                        } else if cat == "balloon" {
                                            ui.horizontal(|ui| {
                                                ui.label("ID番号:");
                                                ui.add(egui::DragValue::new(&mut self.state.import_target_id_num).range(0..=999));
                                            });
                                        }
                                        // clickwait / marker / sstp: 追加パラメータなし
                                    }
                                }
                            });
                        });

                        let target_filename = self.get_import_target_filename();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("保存ファイル名予定:");
                            ui.colored_label(egui::Color32::from_rgb(29, 106, 184), &target_filename);
                        });

                        let mut exists = false;
                        if let Some(dir) = self.state.asset_dir() {
                            exists = dir.join(&target_filename).exists();
                        }
                        // 警告行は常に1行分確保（有無でウィンドウ高が変わらないように）
                        if exists {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "[!] 既に同名ファイルが存在します。上書きされます。");
                        } else {
                            ui.label("");
                        }
                        // PNA行も常に1行分確保
                        if self.state.import_has_pna {
                            ui.colored_label(egui::Color32::from_rgb(100, 160, 80), "同名の .pna ファイルも検出されました。一緒にインポートします。");
                        } else {
                            ui.label("");
                        }

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("インポート (決定)").clicked() {
                                if target_filename.is_empty() || target_filename.ends_with('.') {
                                    self.err("保存ファイル名が不正です。");
                                } else {
                                    let proceed = if exists {
                                        rfd::MessageDialog::new()
                                            .set_title("上書き確認")
                                            .set_description(format!("ファイル「{}」は既に存在します。\n既存ファイルをバックアップ退避した上で上書きインポートしますか？\n(失敗時は自動復元されます)", target_filename))
                                            .set_buttons(rfd::MessageButtons::YesNo)
                                            .show() == rfd::MessageDialogResult::Yes
                                    } else {
                                        true
                                    };

                                    if proceed {
                                        match crate::gui::loader::import_image_file_safe(&self.state, &path, &target_filename) {
                                            Ok(_) => {
                                                next = true;
                                            }
                                            Err(e) => {
                                                self.err(format!("インポートエラー: {}", e));
                                            }
                                        }
                                    }
                                }
                            }
                            if ui.button("この画像をスキップ").clicked() {
                                next = true;
                            }
                            if ui.button("インポートをキャンセル").clicked() {
                                close = true;
                            }
                        });
                    });

                if next {
                    self.state.import_queue_index += 1;
                    if self.state.import_queue_index >= self.state.import_queue.len() {
                        // インポート完了: preview_balloons リストも更新するため完全再読み込み
                        // テキスト編集内容は保持する
                        self.reload_asset_folder_keep_texts(ctx);
                        close = true;
                    } else {
                        self.preset_import_from_current_queue();
                    }
                }
                if close {
                    self.state.show_import_window = false;
                    self.state.import_queue.clear();
                    self.state.import_queue_index = 0;
                }
            }
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
            let (mut sr, mut sg, mut sb) = self.state.canvas_bg_solid_color;
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
                        self.state.canvas_bg_solid_color = (sr, sg, sb);
                        self.state.canvas_bg = crate::gui::state::CanvasBg::Solid(sr, sg, sb);
                        self.refresh_preview_texture(ctx);
                    }
                });
            if !open { self.state.show_bg_color_window = false; }
        }

        // キーボードショートカット（ctx.input のクロージャ外で ctx を使う処理を行う）
        #[derive(Default)]
        struct Keys { export: bool, save: bool, undo: bool, redo: bool, refresh: bool }
        let keys = ctx.input(|i| Keys {
            export:  i.key_pressed(egui::Key::E) && i.modifiers.ctrl,
            save:    i.key_pressed(egui::Key::S) && i.modifiers.ctrl,
            undo:    i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && !i.modifiers.shift,
            redo:    (i.key_pressed(egui::Key::Y) && i.modifiers.ctrl)
                  || (i.key_pressed(egui::Key::Z) && i.modifiers.ctrl && i.modifiers.shift),
            refresh: i.key_pressed(egui::Key::F5),
        });
        if keys.export  { self.export(); }
        if keys.save && self.state.is_project_dir() { self.save_project(); }
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
        if keys.refresh { self.reload_asset_folder_keep_texts(ctx); }

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

/// Profile → AppState に色設定のみ適用する（プロジェクト読み込み時用）
/// テキスト類・asset_dir はフォルダから読んだものを優先するため上書きしない
fn apply_project_profile_to_state(state: &mut AppState, profile: &crate::core::profile::Profile) {
    use crate::gui::state::LAYER_DEFS;

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
        profile.parts_colors.iter()
            .map(|(k, &[r, g, b])| (k.clone(), crate::core::color::Rgb(r, g, b)))
            .collect()
    };

    state.individual_colors = profile.individual_colors.iter()
        .map(|(k, ic)| {
            let indiv = crate::gui::state::IndivColors {
                base:  ic.base.map(|[r,g,b]| crate::core::color::Rgb(r,g,b)),
                edge:  ic.edge.map(|[r,g,b]| crate::core::color::Rgb(r,g,b)),
                text:  ic.text.map(|[r,g,b]| crate::core::color::Rgb(r,g,b)),
                parts_colors: ic.parts_colors.iter()
                    .map(|(k, &[r,g,b])| (k.clone(), crate::core::color::Rgb(r,g,b)))
                    .collect(),
            };
            (k.clone(), indiv)
        })
        .collect();

    state.no_balloon_color = profile.no_balloon_color;
    state.bulk_color_mode  = profile.bulk_color_mode;
    state.selected_balloon = if !profile.selected_balloon.is_empty() {
        profile.selected_balloon.clone()
    } else {
        state.selected_balloon.clone()
    };
}

/// Profile → AppState に適用する
fn apply_profile_to_state(state: &mut AppState, profile: &crate::core::profile::Profile) {
    use crate::gui::state::LAYER_DEFS;

    if !profile.asset_dir.is_empty() {
        let p = std::path::PathBuf::from(&profile.asset_dir);
        // プロジェクトフォルダ以外は復元しない（旧「素材フォルダを選択」で保存されたパス対策）
        let is_project = crate::core::project::get_projects_base_dir()
            .map(|base| p.starts_with(&base))
            .unwrap_or(false);
        if p.is_dir() && is_project {
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
