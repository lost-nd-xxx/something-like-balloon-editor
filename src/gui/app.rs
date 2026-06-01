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
    /// 出力完了ダイアログ表示中の出力先フォルダ（Some の間「フォルダを開く」ボタン付きダイアログを表示）
    pub export_done_dir: Option<PathBuf>,
    /// 出力要求フラグ。立つと次フレームで「出力中…」描画後に実際の出力を実行する。
    pub pending_export: bool,
    /// 「出力中…」オーバーレイを1フレーム描画済みか（描画→次フレームで実行のため）
    pub export_overlay_drawn: bool,
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

        // 起動時の状態復元（アプリ固有状態 state.json）
        let state_path = root.join("state.json");
        let has_config = if let Some(config) = crate::core::profile::load_app_config(&state_path) {
            apply_app_config_to_state(&mut state, &config);
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
            export_done_dir: None,
            pending_export: false,
            export_overlay_drawn: false,
            preview_result: Arc::new(Mutex::new(None)),
        };

        // 素材フォルダを初期読み込み（txt が正本のため常にフォルダから読む）
        let _ = has_config;
        match load_asset_folder(&mut app.state, &app.root) {
            Ok(_) => {
                // プロジェクトフォルダなら色設定を slbe_profile.json から復元
                if app.state.is_project_dir() {
                    if let Some(asset_dir) = app.state.asset_dir() {
                        let profile_path = crate::core::profile::project_profile_path(&asset_dir);
                        if let Ok(profile) = crate::core::profile::load_project_profile(&profile_path) {
                            apply_project_profile_to_state(&mut app.state, &profile);
                        }
                    }
                }
                // descript / install から基本情報を再構築
                app.rebuild_basic_info();
                // 起動時の警告（name 食い違い・文字コード等）を表示する
                app.flush_load_warnings();
            }
            Err(e) => {
                app.dialog = Some(("エラー".into(), format!("素材フォルダの読み込みに失敗しました:\n\n{}", e)));
            }
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
            is_balloonc:  self.state.is_balloonc(),
            show_overlay: self.state.overlay_mode == "layout" && !self.state.is_balloonc(),
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

    /// 未保存確認ダイアログ通過後の保留アクションを実行する
    fn run_pending_action(&mut self, action: crate::gui::state::PendingAction, ctx: &Context) {
        use crate::gui::state::PendingAction;
        match action {
            PendingAction::OpenProject(name) => self.do_open_project(&name, ctx),
            PendingAction::Quit => {
                self.state.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// プロジェクトを実際に開く（未保存チェックは呼び出し側で済ませる前提）
    pub fn do_open_project(&mut self, name: &str, ctx: &Context) {
        if let Ok(dir) = crate::core::project::get_project_dir(name) {
            self.state.selected_asset_dir = Some(dir);
            // 開き直す前に未保存フラグをクリア。
            // reload 内の rebuild_basic_info が name 食い違いを検出した場合は
            // その場で dirty=true が立つため、ここでクリアしておく順序にする。
            self.state.dirty = false;
            self.reload_asset_folder(ctx);
        }
    }

    pub fn reload_asset_folder(&mut self, ctx: &Context) {
        // 素材フォルダを切り替えたとき基本情報もリセットする
        self.state.basic_info.clear();
        match load_asset_folder(&mut self.state, &self.root.clone()) {
            Ok(_) => {
                // プロジェクトフォルダなら slbe_profile.json があれば色設定を自動復元
                // （テキスト類はフォルダから読んだものを優先するため上書きしない）
                if self.state.is_project_dir() {
                    if let Some(asset_dir) = self.state.asset_dir() {
                        let profile_path = crate::core::profile::project_profile_path(&asset_dir);
                        if let Ok(profile) = crate::core::profile::load_project_profile(&profile_path) {
                            apply_project_profile_to_state(&mut self.state, &profile);
                        }
                    }
                }

                // descript / install から基本情報を再構築
                self.rebuild_basic_info();
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
                // テキストから基本情報を再構築
                self.rebuild_basic_info();
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
        // 元ファイルの拡張子を保持する（.png / .pnr 等）
        let ext = std::path::Path::new(old_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_string();
        let old_stem = std::path::Path::new(old_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(old_name);
        let new_name = format!("{}.{}", new_stem, ext);

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

        // slbe_files_text（メモリ）内の参照を更新し、ディスクにも書き出す
        if !self.state.slbe_files_text.is_empty() {
            let updated = self.state.slbe_files_text
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
            let updated = format!("{}\n", updated);
            self.state.slbe_files_text = updated.clone();
            let files_txt = crate::core::layout::slbe_files_txt_path(&asset_dir);
            if files_txt.exists() {
                let _ = std::fs::write(&files_txt, &updated);
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
        let stem = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(name);
        let pna_path = asset_dir.join(format!("{}.pna", stem));
        let cfg_path = asset_dir.join(format!("{}s.txt", stem));
        let has_file = path.exists();
        let has_pna  = pna_path.exists();
        let has_cfg  = cfg_path.exists();

        if !has_file {
            // 物理ファイルなし（files.txt 定義のみのバルーン）
            // files.txt から該当行を削除するか確認する
            let confirmed = rfd::MessageDialog::new()
                .set_title("削除確認")
                .set_description(format!(
                    "「{}」は画像ファイルを持たず、レイアウト定義（files.txt）のみに存在します。\n\nfiles.txt の定義を削除しますか？",
                    name
                ))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show() == rfd::MessageDialogResult::Yes;
            if confirmed {
                self.remove_balloon_from_files_txt(stem);
                self.state.pending_reload = true;
            }
            return;
        }

        // 付随ファイル（ファイル名 + 種別の説明）を列挙する
        let mut extras = Vec::new();
        if has_pna { extras.push(format!("{}.pna（アルファチャンネル画像）", stem)); }
        if has_cfg { extras.push(format!("{}s.txt（個別設定ファイル）", stem)); }
        let desc = if extras.is_empty() {
            format!("「{}」をゴミ箱に移動しますか？", name)
        } else {
            format!(
                "「{}」をゴミ箱に移動しますか？\n\n以下の付随ファイルも同時に削除されます:\n・{}",
                name,
                extras.join("\n・")
            )
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
            if has_cfg {
                if let Err(e) = trash::delete(&cfg_path) {
                    self.err(format!("{}s.txt の削除に失敗しました:\n{}", stem, e));
                }
            }
            // files.txt からも定義行を削除する
            self.remove_balloon_from_files_txt(stem);
            // 削除後は next_reload フラグを立てておく（ctx がここでは使えないため）
            self.state.pending_reload = true;
        }
    }

    /// files.txt（slbe_files_text）から指定 stem のバルーン定義行を削除してディスクに書き出す
    fn remove_balloon_from_files_txt(&mut self, stem: &str) {
        if self.state.slbe_files_text.is_empty() { return; }
        let updated: String = self.state.slbe_files_text
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("//") || trimmed.is_empty() { return true; }
                // 先頭カラム（バルーン名）が stem と一致する行を除去
                let first = trimmed.split(',').next().unwrap_or("").trim();
                first != stem
            })
            .collect::<Vec<_>>()
            .join("\n");
        let updated = format!("{}\n", updated);
        self.state.slbe_files_text = updated.clone();
        if let Some(asset_dir) = self.state.asset_dir() {
            let files_txt = crate::core::layout::slbe_files_txt_path(&asset_dir);
            if files_txt.exists() {
                let _ = std::fs::write(&files_txt, &updated);
            }
        }
    }

    /// descript.txt / install.txt から基本情報（basic_info）を再構築する。
    /// name は descript / install の双方に存在しうるため、食い違いを検出したら
    /// 警告を出した上で descript 側の値を採用する。
    fn rebuild_basic_info(&mut self) {
        use crate::core::descript::parse_descript;
        let parsed_d = parse_descript(&self.state.descript_text);
        let parsed_i = parse_descript(&self.state.install_text);

        self.state.basic_info.clear();
        for key in &["name", "craftman", "craftmanw", "craftmanurl", "homeurl"] {
            if let Some(v) = parsed_d.get(*key) {
                self.state.basic_info.insert(key.to_string(), v.clone());
            }
        }
        if let Some(v) = parsed_i.get("directory") {
            self.state.basic_info.insert("directory".to_string(), v.clone());
        }

        // name の食い違いを検出（descript と install の双方に name がある場合）
        if let (Some(nd), Some(ni)) = (parsed_d.get("name"), parsed_i.get("name")) {
            if nd != ni {
                let nd = nd.clone();
                let ni = ni.clone();
                self.state.load_warnings.push(format!(
                    "descript.txt と install.txt の name が食い違っています。\n\
                     descript.txt: \"{nd}\"\n\
                     install.txt: \"{ni}\"\n\
                     descript.txt 側の値を採用します。"
                ));
                // descript 側を採用 → install_text も合わせて未保存状態にする
                // （採用結果が保存されないまま終了できてしまうのを防ぐ）
                self.state.install_text =
                    crate::core::descript::set_descript_value(&self.state.install_text, "name", &nd);
                self.state.dirty = true;
            }
        }
    }

    /// load_warnings が溜まっていればダイアログに表示してクリアする
    fn flush_load_warnings(&mut self) {
        if self.state.load_warnings.is_empty() { return; }
        let msg = self.state.load_warnings.join("\n\n――――――――――\n\n");
        self.state.load_warnings.clear();
        self.dialog = Some(("読み込み時の通知".into(), msg));
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
    /// プロジェクトに保存（descript.txt / install.txt の書き出し + profile/slbe_profile.json の保存）
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

        // 個別設定ファイル（{バルーン名}s.txt）を書き出す。
        // - individual_texts に非空エントリがあれば書き出す（txt が正本）
        // - 空 / 未定義になった項目は、ディスク上の {stem}s.txt を削除して古い設定を残さない
        for balloon_name in &self.state.preview_balloons {
            let stem = balloon_name.trim_end_matches(".png");
            let cfg_name = format!("{}s.txt", stem);
            let cfg_path = asset_dir.join(&cfg_name);
            match self.state.individual_texts.get(&cfg_name) {
                Some(text) if !text.trim().is_empty() => {
                    // descript からの差分のみに圧縮して保存（個別設定ファイル仕様）
                    let diff = crate::core::descript::diff_against_descript(
                        text, &self.state.descript_text);
                    if diff.trim().is_empty() {
                        // 差分が無ければ個別設定として保持する意味がない → 既存ファイルを削除
                        if cfg_path.exists() {
                            let _ = std::fs::remove_file(&cfg_path);
                        }
                    } else if let Err(e) = std::fs::write(&cfg_path, &diff) {
                        self.err(format!("{} の保存に失敗しました:\n{}", cfg_name, e));
                        return;
                    }
                }
                _ => {
                    // 空 / 未定義 → 既存ファイルがあれば削除
                    if cfg_path.exists() {
                        let _ = std::fs::remove_file(&cfg_path);
                    }
                }
            }
        }

        // slbe_files.txt を profile/ に書き出す
        {
            let files_path = crate::core::layout::slbe_files_txt_path(&asset_dir);
            let text = &self.state.slbe_files_text;
            if text.trim().is_empty() {
                if files_path.exists() {
                    let _ = std::fs::remove_file(&files_path);
                }
            } else {
                if let Some(parent) = files_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&files_path, text) {
                    self.err(format!("slbe_files.txt の保存に失敗しました:\n{}", e));
                    return;
                }
            }
        }

        // 色設定を profile/slbe_profile.json に保存
        let profile_path = crate::core::profile::project_profile_path(&asset_dir);
        let profile = build_project_profile_from_state(&self.state);
        if let Err(e) = crate::core::profile::save_project_profile(&profile_path, &profile) {
            self.err(format!("プロファイルの保存に失敗しました:\n{}", e));
            return;
        }

        // 保存成功 → 未保存フラグをクリア
        self.state.dirty = false;
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
            Ok((dir, _filled)) => {
                self.state.selected_asset_dir = Some(dir);
                self.reload_asset_folder(ctx);
            }
            Err(e) => {
                self.err(format!("別名で保存エラー: {}", e));
            }
        }
    }

    /// 出力処理
    pub fn export(&mut self) {
        use crate::core::descript::parse_descript;

        let Some(_ad) = self.state.asset_dir() else {
            self.err("素材フォルダが選択されていません。\n\nメニュー「ファイル > 素材フォルダを選択...」から素材フォルダを指定してください。".to_string());
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
            if let Err(e) = save_png_optimized(img, &path) {
                self.err(format!("{} の保存に失敗しました:\n{}", name, e));
                return;
            }
        }

        // パーツ画像を出力（先頭バルーンのキャッシュを代表として使う）
        if let Some(parts) = self.state.parts_cache.values().next() {
            for (name, img) in parts {
                let path = output_dir.join(name);
                if let Err(e) = save_png_optimized(img, &path) {
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

        // テキストファイルを出力（readme は中身を触らず実ファイルを物理コピーする）
        let texts = [
            ("descript.txt", descript_out),
            ("install.txt",  self.state.install_text.clone()),
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

        // readme は descript.txt の "readme,ファイル名"（無ければ readme.txt）で
        // 指定された実ファイルをプロジェクトからそのまま物理コピーする。
        // アプリは中身・charset を一切編集しない。
        let readme_name = {
            let parsed = crate::core::descript::parse_descript(&self.state.descript_text);
            let raw = parsed.get("readme").map(|s| s.as_str()).unwrap_or("readme.txt");
            // パストラバーサル対策: ファイル名部分のみ使用
            std::path::Path::new(raw)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("readme.txt")
                .to_string()
        };
        let readme_src = ad.join(&readme_name);
        if readme_src.is_file() {
            let readme_dest = output_dir.join(&readme_name);
            if let Err(e) = std::fs::copy(&readme_src, &readme_dest) {
                self.err(format!("{} のコピーに失敗しました:\n{}", readme_name, e));
                return;
            }
        }

        // 個別設定ファイルを出力
        for (cfg, text) in &self.state.individual_texts.clone() {
            // パストラバーサル対策: ファイル名部分のみ使用
            let cfg_safe = std::path::Path::new(cfg)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if cfg_safe.is_empty() { continue; }
            let path = output_dir.join(cfg_safe);
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                self.err(format!("{} の書き込みに失敗しました:\n{}", cfg, e));
                return;
            }
        }

        // アプリが合成・生成しない素材ファイル（thumbnail.png/.pnr、cursor画像など）を
        // プロジェクトフォルダから物理コピーする。
        // 既に出力済みのファイル名・テキスト類・profile/配下・.pna は対象外。
        {
            // 出力済みファイル名の集合を作る
            let mut already: std::collections::HashSet<String> = std::collections::HashSet::new();
            for name in balloon_images.keys() { already.insert(name.to_lowercase()); }
            if let Some(parts) = self.state.parts_cache.values().next() {
                for name in parts.keys() { already.insert(name.to_lowercase()); }
            }
            for fname in ["descript.txt", "install.txt"] {
                already.insert(fname.to_string());
            }
            // readme は実ファイル名（readme,ファイル名）で出力済み
            already.insert(readme_name.to_lowercase());
            for cfg in self.state.individual_texts.keys() {
                already.insert(cfg.to_lowercase());
            }

            if let Ok(entries) = std::fs::read_dir(&ad) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() { continue; }
                    let fname = match path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    let fname_lower = fname.to_lowercase();
                    // 既に出力済み・テキスト・.pna は対象外
                    if already.contains(&fname_lower) { continue; }
                    if fname_lower.ends_with(".txt") { continue; }
                    if fname_lower.ends_with(".pna") { continue; }
                    // 画像系（png/pnr/jpg等）のみコピー対象
                    let is_image = [".png", ".pnr", ".jpg", ".jpeg", ".bmp"]
                        .iter().any(|ext| fname_lower.ends_with(ext));
                    if !is_image { continue; }
                    let dest = output_dir.join(&fname);
                    if let Err(e) = std::fs::copy(&path, &dest) {
                        self.err(format!("{} のコピーに失敗しました:\n{}", fname, e));
                        return;
                    }
                }
            }
        }

        // 出力完了ダイアログ（「フォルダを開く」ボタン付き）を表示する。
        // 自動でエクスプローラは開かず、ユーザーの選択に委ねる。
        self.export_done_dir = Some(output_dir.clone());
    }

    /// インポートキュー内の現在インデックスにあるファイルからUI入力値を自動推測してプレセットします。
    pub fn preset_import_from_current_queue(&mut self, ctx: &egui::Context) {
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

        // インポート画像のプレビューテクスチャを生成
        self.state.import_preview_texture = load_import_preview_texture(ctx, path);
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
        // ウィンドウ「×」やショートカットによる終了要求をインターセプトする。
        // 未保存編集があり、まだ終了許可が出ていなければ閉じるのを取り消して確認ダイアログを出す。
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.allow_close {
            if self.state.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.state.pending_unsaved_action.is_none() {
                    self.state.pending_unsaved_action =
                        Some(crate::gui::state::PendingAction::Quit);
                }
            } else {
                // 未保存なし → そのまま閉じてよい
                self.state.allow_close = true;
            }
        }

        // 削除後などの遅延リロード
        if self.state.pending_reload {
            self.state.pending_reload = false;
            self.reload_asset_folder_keep_texts(ctx);
        }

        // 出力の遅延実行: 前フレームで「出力中…」オーバーレイを描画済みなら、
        // このフレームで実際の出力（同期・重い処理）を行う。
        if self.pending_export && self.export_overlay_drawn {
            self.pending_export = false;
            self.export_overlay_drawn = false;
            self.export();
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
                    self.preset_import_from_current_queue(ctx);
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

        // 出力完了ダイアログ（「フォルダを開く」ボタン付き）
        if let Some(out_dir) = self.export_done_dir.clone() {
            egui::Window::new("出力完了")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("出力が完了しました。\n\n出力先:\n{}", out_dir.display()));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("フォルダを開く").clicked() {
                            let _ = open::that(&out_dir);
                            self.export_done_dir = None;
                        }
                        if ui.button("閉じる").clicked() {
                            self.export_done_dir = None;
                        }
                    });
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
                if self.state.dirty {
                    // 未保存編集あり → 確認ダイアログを経由する
                    self.state.pending_unsaved_action =
                        Some(crate::gui::state::PendingAction::OpenProject(name));
                } else {
                    self.do_open_project(&name, ctx);
                }
            }
            if close {
                self.state.show_open_project_window = false;
                self.state.open_project_selected = None;
                self.state.open_project_filter.clear();
            }
        }

        // 未保存編集の確認ダイアログ
        if self.state.pending_unsaved_action.is_some() {
            use crate::gui::state::PendingAction;
            #[derive(PartialEq)]
            enum Choice { None, Save, Discard, Cancel }
            let mut choice = Choice::None;

            // 保留アクションごとに文言を切り替える
            let is_quit = matches!(self.state.pending_unsaved_action, Some(PendingAction::Quit));
            let (message, save_label, discard_label) = if is_quit {
                ("保存していない変更があります。\n保存して終了しますか？", "保存して終了", "保存せず終了")
            } else {
                ("保存していない変更があります。\n保存しますか？", "保存して続行", "保存せず続行")
            };

            egui::Window::new("未保存の変更")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(message);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(save_label).clicked()    { choice = Choice::Save; }
                        if ui.button(discard_label).clicked() { choice = Choice::Discard; }
                        if ui.button("キャンセル").clicked()  { choice = Choice::Cancel; }
                    });
                });

            match choice {
                Choice::Save => {
                    self.save_project();
                    // 保存に失敗した場合は dirty が残るので続行しない
                    if !self.state.dirty {
                        if let Some(action) = self.state.pending_unsaved_action.take() {
                            self.run_pending_action(action, ctx);
                        }
                    } else {
                        self.state.pending_unsaved_action = None;
                    }
                }
                Choice::Discard => {
                    if let Some(action) = self.state.pending_unsaved_action.take() {
                        self.state.dirty = false;
                        self.run_pending_action(action, ctx);
                    }
                }
                Choice::Cancel => {
                    self.state.pending_unsaved_action = None;
                }
                Choice::None => {}
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
                            match crate::core::project::create_project_from_folder(&src, &name_trimmed) {
                                Ok((dir, filled)) => {
                                    // blendmethod 警告チェック
                                    let bm_warns = crate::core::project::check_blendmethod_warnings(&dir);
                                    self.state.selected_asset_dir = Some(dir);
                                    self.reload_asset_folder(ctx);
                                    // 補完通知と blendmethod 警告を1つのダイアログに集約する
                                    // （reload_asset_folder 後に組み立てて dialog を上書き）
                                    let mut sections: Vec<String> = Vec::new();
                                    // reload_asset_folder 内の flush_load_warnings が設定した
                                    // 文字コード警告ダイアログがあれば先頭に取り込んで潰さないようにする
                                    if let Some((_, prev_msg)) = self.dialog.take() {
                                        sections.push(prev_msg);
                                    }
                                    if !filled.is_empty() {
                                        let group_names: Vec<&str> = filled.iter().map(|k| match k.as_str() {
                                            "cursor.blendmethod"           => "選択肢(選択中)",
                                            "cursor.notselect.blendmethod" => "選択肢(非選択)",
                                            "anchor.blendmethod"           => "アンカー(選択中)",
                                            "anchor.notselect.blendmethod" => "アンカー(非選択)",
                                            "anchor.visited.blendmethod"   => "アンカー(訪問済み)",
                                            other => other,
                                        }).collect();
                                        sections.push(format!(
                                            "descript.txt に以下のグループの装飾設定が見つからなかったため、初期値を補完しました。\n\
                                             内容を確認し、必要に応じて設定を調整してください。\n\n{}",
                                            group_names.join("\n")
                                        ));
                                    }
                                    if !bm_warns.is_empty() {
                                        sections.push(format!(
                                            "以下の blendmethod 設定はこのアプリでは再現できません。\n\
                                             SSPでの表示と異なる場合があります。\n\n{}",
                                            bm_warns.iter().map(|w| {
                                                w.trim_end_matches(" (このアプリでは描画を再現できません)").to_string()
                                            }).collect::<Vec<_>>().join("\n")
                                        ));
                                    }
                                    if !sections.is_empty() {
                                        self.dialog = Some((
                                            "プロジェクト作成 通知".into(),
                                            sections.join("\n\n――――――――――\n\n"),
                                        ));
                                    }
                                    close = true;
                                }
                                Err(e) => {
                                    self.err(format!("プロジェクト作成エラー: {}", e));
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
                            let name = name_trimmed.clone();
                            self.save_project_as(&name, ctx);
                            close = true;
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
            // 元ファイルの拡張子を保持（.png / .pnr 等）
            let target_ext = std::path::Path::new(&target)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_string();
            let old_stem = std::path::Path::new(&target)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&target)
                .to_string();

            if self.state.rename_is_balloon {
                // ── バルーンリスト由来：構造化入力UI ──────────────────────
                // スコープ番号・ID番号から変更後ファイル名を生成するヘルパー
                let make_stem = |is_c: bool, scope: i32, id: u32| -> String {
                    if is_c {
                        format!("balloonc{}", id)
                    } else {
                        match scope {
                            0 => format!("balloons{}", id),
                            1 => format!("balloonk{}", id),
                            n => format!("balloonp{}def{}", n, id),
                        }
                    }
                };

                egui::Window::new("名前変更")
                    .collapsible(false)
                    .resizable(false)
                    .fixed_size([300.0, 180.0])
                    .show(ctx, |ui| {
                        ui.label(format!("変更前: {}", old_stem));
                        ui.add_space(6.0);

                        // 種別ラジオボタン
                        ui.horizontal(|ui| {
                            ui.label("種別:");
                            ui.radio_value(&mut self.state.rename_balloon_is_c, false, "通常バルーン（s/k/p系）");
                            ui.radio_value(&mut self.state.rename_balloon_is_c, true,  "入力ボックス（c系）");
                        });

                        ui.add_space(4.0);

                        if self.state.rename_balloon_is_c {
                            // c系: ID番号 0〜4
                            ui.horizontal(|ui| {
                                ui.label("ID番号:");
                                ui.add(egui::DragValue::new(&mut self.state.rename_id_num).range(0..=4));
                            });
                        } else {
                            // s/k/p系: スコープ番号 + ID番号
                            ui.horizontal(|ui| {
                                ui.label("スコープ番号:");
                                ui.add(egui::DragValue::new(&mut self.state.rename_scope_num).range(0..=99));
                                let n = self.state.rename_scope_num;
                                ui.small(format!("({}人目)", n + 1));
                            });
                            ui.horizontal(|ui| {
                                ui.label("ID番号:");
                                ui.add(egui::DragValue::new(&mut self.state.rename_id_num).range(0..=999));
                            });
                        }

                        ui.add_space(4.0);
                        let new_stem = make_stem(
                            self.state.rename_balloon_is_c,
                            self.state.rename_scope_num,
                            self.state.rename_id_num,
                        );
                        let is_same = new_stem == old_stem;
                        // 物理ファイル OR files.txt 定義のいずれかに同名があれば重複
                        let already_exists = !is_same && self.state.balloon_name_exists(&new_stem);

                        self.state.rename_warning = if already_exists {
                            "[!] 同名のバルーンが既に存在します。".to_string()
                        } else {
                            String::new()
                        };

                        ui.label(format!("変更後: {}", new_stem));
                        if !self.state.rename_warning.is_empty() {
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), &self.state.rename_warning.clone());
                        } else {
                            ui.label("");
                        }

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let can_rename = !is_same && !already_exists;
                            if ui.add_enabled(can_rename, egui::Button::new("変更")).clicked() {
                                self.rename_png(&target, &new_stem, ctx);
                                close = true;
                            }
                            if ui.button("キャンセル").clicked() {
                                close = true;
                            }
                        });
                    });
            } else {
                // ── PNG一覧由来：自由テキスト入力UI ──────────────────────
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
                        let new_name = format!("{}.{}", new_stem, target_ext);
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
            }

            if close {
                self.state.show_rename_window = false;
                self.state.rename_target.clear();
                self.state.rename_new_name.clear();
                self.state.rename_warning.clear();
                self.state.rename_is_balloon = false;
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
                    .fixed_size([740.0, 360.0])
                    .show(ctx, |ui| {
                        ui.colored_label(egui::Color32::from_rgb(29, 106, 184), format!("インポート画像 ({} / {} 枚目):", idx + 1, self.state.import_queue.len()));
                        ui.label(format!("ファイル名: {}", filename));
                        ui.add_space(6.0);

                        // カテゴリ選択（左）＋ パラメータ（中）＋ プレビュー（右）の左右分割レイアウト
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

                            ui.separator();

                            // 右側: インポート画像プレビュー
                            ui.vertical(|ui| {
                                ui.set_min_width(180.0);
                                ui.label("プレビュー:");
                                if let Some(tex) = &self.state.import_preview_texture {
                                    let [tw, th] = tex.size();
                                    // 最大 160×160 に収まるようスケーリング
                                    const MAX: f32 = 160.0;
                                    let scale = (MAX / tw as f32).min(MAX / th as f32).min(1.0);
                                    let disp = egui::vec2(tw as f32 * scale, th as f32 * scale);
                                    ui.image((tex.id(), disp));
                                    ui.small(format!("{}×{} px", tw, th));
                                } else {
                                    ui.label("(読み込み失敗)");
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
                        // slbe_files.txt ありモードで、インポート先バルーン名が既に定義済みかチェック
                        let target_stem = target_filename.trim_end_matches(".png");
                        let defined_in_files_txt = !self.state.direct_image_mode
                            && self.state.balloon_layout.contains_key(target_stem);

                        // 警告行は常に1行分確保（有無でウィンドウ高が変わらないように）
                        if defined_in_files_txt {
                            ui.colored_label(egui::Color32::from_rgb(220, 140, 0), "[!] このバルーン名は slbe_files.txt に既に定義されています。インポート時に該当定義を無効化します。");
                        } else if exists {
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
                                    // slbe_files.txt に定義済みの場合はコメントアウト（確認なし・undo対象外）
                                    // インポート後の reload_asset_folder_keep_texts でディスクから
                                    // 再パースされるため、メモリと同時にディスクにも即書き出す
                                    if defined_in_files_txt {
                                        let new_text = comment_out_balloon_in_files_txt(
                                            &self.state.slbe_files_text,
                                            target_stem,
                                        );
                                        self.state.slbe_files_text = new_text.clone();
                                        if let Some(asset_dir) = self.state.asset_dir() {
                                            let files_path = crate::core::layout::slbe_files_txt_path(&asset_dir);
                                            if let Some(parent) = files_path.parent() {
                                                let _ = std::fs::create_dir_all(parent);
                                            }
                                            let _ = std::fs::write(&files_path, &new_text);
                                        }
                                    }

                                    let proceed = if exists {
                                        rfd::MessageDialog::new()
                                            .set_title("上書き確認")
                                            .set_description(format!("ファイル「{}」は既に存在します。上書きしますか？", target_filename))
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
                        self.preset_import_from_current_queue(ctx);
                    }
                }
                if close {
                    self.state.show_import_window = false;
                    self.state.import_queue.clear();
                    self.state.import_queue_index = 0;
                }
            }
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
        if keys.export  { self.pending_export = true; }
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

        // files.txt エディタウィンドウ
        panels::files_editor::show(self, ctx);

        // 出力中オーバーレイ: pending_export が立っている間、画面中央に「出力中…」を表示する。
        // 描画した最初のフレームで export_overlay_drawn を立て、次フレームで実出力を行う
        // （実出力は同期・重い処理のため、先にこの表示を1フレーム見せる）。
        if self.pending_export {
            let screen = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("export_overlay"),
            ));
            // 画面全体を半透明で覆う
            painter.rect_filled(screen, 0.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 100));
            let label = "出力中…";
            let font_id = egui::FontId::proportional(20.0);
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(label.to_string(), font_id, egui::Color32::WHITE)
            });
            let text_size = galley.size();
            let pad = egui::vec2(20.0, 12.0);
            let bg_size = text_size + pad * 2.0;
            let bg_pos = egui::pos2(
                screen.center().x - bg_size.x / 2.0,
                screen.center().y - bg_size.y / 2.0,
            );
            let bg_rect = egui::Rect::from_min_size(bg_pos, bg_size);
            painter.rect_filled(bg_rect, 8.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220));
            painter.galley(bg_pos + pad, galley, egui::Color32::WHITE);

            // このフレームで描画した印を付け、次フレームを要求する（次フレームで実出力）
            self.export_overlay_drawn = true;
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 終了時にアプリ固有状態を state.json へ保存
        let state_path = self.root.join("state.json");
        let config = build_app_config_from_state(&self.state);
        let _ = crate::core::profile::save_app_config(&state_path, &config);
    }
}

/// ProjectProfile → AppState に色設定を適用する（プロジェクト読み込み時用）
/// テキスト類・asset_dir はフォルダから読んだものを優先するため上書きしない
fn apply_project_profile_to_state(state: &mut AppState, profile: &crate::core::profile::ProjectProfile) {
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
}

/// AppConfig → AppState にアプリ固有状態を適用する（起動時）
fn apply_app_config_to_state(state: &mut AppState, config: &crate::core::profile::AppConfig) {
    if !config.asset_dir.is_empty() {
        let p = std::path::PathBuf::from(&config.asset_dir);
        // プロジェクトフォルダ以外は復元しない（旧「素材フォルダを選択」で保存されたパス対策）
        let is_project = crate::core::project::get_projects_base_dir()
            .map(|base| p.starts_with(&base))
            .unwrap_or(false);
        if p.is_dir() && is_project {
            state.selected_asset_dir = Some(p);
        }
    }

    state.theme = match config.theme.as_str() {
        "dark" => crate::gui::state::ThemeMode::Dark,
        _      => crate::gui::state::ThemeMode::Light,
    };

    if config.panel_left_width  > 0.0 { state.panel_left_width  = config.panel_left_width; }
    if config.panel_right_width > 0.0 { state.panel_right_width = config.panel_right_width; }
    if config.window_size[0]    > 0.0 { state.window_size       = config.window_size; }
}

/// AppState → ProjectProfile に変換する（色設定 → slbe_profile.json）
fn build_project_profile_from_state(state: &AppState) -> crate::core::profile::ProjectProfile {
    use crate::core::profile::IndividualColors;
    crate::core::profile::ProjectProfile {
        version:        2,
        layer_colors:   state.layer_colors.iter()
            .map(|(k, &Rgb(r, g, b))| (k.clone(), [r, g, b]))
            .collect(),
        parts_colors:   state.parts_colors.iter()
            .map(|(k, &Rgb(r, g, b))| (k.clone(), [r, g, b]))
            .collect(),
        individual_colors: state.individual_colors.iter()
            .map(|(k, ic)| {
                (k.clone(), IndividualColors {
                    base: ic.base.map(|Rgb(r, g, b)| [r, g, b]),
                    edge: ic.edge.map(|Rgb(r, g, b)| [r, g, b]),
                    text: ic.text.map(|Rgb(r, g, b)| [r, g, b]),
                    parts_colors: ic.parts_colors.iter()
                        .map(|(k, &Rgb(r, g, b))| (k.clone(), [r, g, b]))
                        .collect(),
                })
            })
            .collect(),
        no_balloon_color:  state.no_balloon_color,
        bulk_color_mode:   state.bulk_color_mode,
    }
}

/// AppState → AppConfig に変換する（アプリ固有状態 → state.json）
fn build_app_config_from_state(state: &AppState) -> crate::core::profile::AppConfig {
    crate::core::profile::AppConfig {
        version:        2,
        asset_dir:      state.selected_asset_dir.as_ref()
                            .and_then(|p| p.to_str())
                            .unwrap_or("")
                            .to_string(),
        theme: match state.theme {
            crate::gui::state::ThemeMode::Light => "light".to_string(),
            crate::gui::state::ThemeMode::Dark  => "dark".to_string(),
        },
        panel_left_width:  state.panel_left_width,
        panel_right_width: state.panel_right_width,
        window_size:       state.window_size,
    }
}

// Rgb を分解するためのパターンマッチを使えるようにするための import
use crate::core::color::Rgb;

/// slbe_files.txt テキスト内の指定バルーン名行をコメントアウトして返す。
/// 行の先頭が `balloon_name,` にマッチする行を `//` でコメントアウトする。
fn comment_out_balloon_in_files_txt(text: &str, balloon_name: &str) -> String {
    let prefix = format!("{},", balloon_name);
    let lines: Vec<String> = text.lines().map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("//") && trimmed.starts_with(&prefix) {
            format!("//{}", line)
        } else {
            line.to_string()
        }
    }).collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

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

    // 先頭に挿入してグリフ解決を BIZ UDPGothic 優先にする
    // （末尾 push だと全角記号・全角英字が egui 既定フォントで描画され字体が混ざる）
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families
            .entry(family)
            .or_default()
            .insert(0, "bizud".to_owned());
    }

    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles.iter_mut().for_each(|(_, font_id)| {
        font_id.size *= 1.15;
    });
    style.spacing.button_padding = egui::vec2(6.0, 3.0);
    ctx.set_style(style);
}

/// RRGBA画像を高圧縮設定（CompressionType::Best + Adaptiveフィルタ）でPNG保存する。
/// image クレートのデフォルト保存より小さいファイルサイズになる。
fn save_png_optimized(img: &RgbaImage, path: &std::path::Path) -> anyhow::Result<()> {
    use image::codecs::png::{PngEncoder, CompressionType, FilterType};
    use image::ImageEncoder;
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let encoder = PngEncoder::new_with_quality(
        writer,
        CompressionType::Best,
        FilterType::Adaptive,
    );
    encoder.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

/// インポートダイアログ用プレビューテクスチャを生成する。
/// 画像をチェッカー背景に合成してから egui テクスチャとして返す。
fn load_import_preview_texture(ctx: &egui::Context, path: &std::path::Path) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let (w, h) = img.dimensions();

    // チェッカー背景（8px ごとに明暗切り替え）に画像を合成
    const CELL: u32 = 8;
    const LIGHT: [u8; 4] = [200, 200, 200, 255];
    const DARK:  [u8; 4] = [140, 140, 140, 255];

    let mut pixels: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let checker = if ((x / CELL) + (y / CELL)) % 2 == 0 { LIGHT } else { DARK };
            let src = img.get_pixel(x, y).0;
            let a = src[3] as f32 / 255.0;
            let r = (src[0] as f32 * a + checker[0] as f32 * (1.0 - a)) as u8;
            let g = (src[1] as f32 * a + checker[1] as f32 * (1.0 - a)) as u8;
            let b = (src[2] as f32 * a + checker[2] as f32 * (1.0 - a)) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }

    let color_image = ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &pixels,
    );
    Some(ctx.load_texture("import_preview", color_image, TextureOptions::NEAREST))
}
