/// アプリ全体の編集状態
use std::collections::HashMap;
use std::path::PathBuf;
use image::RgbaImage;
use crate::core::color::{ColorSet, Rgb};
use crate::core::layout::LayerList;
use egui;

pub const APP_NAME: &str = "Something Like Balloon Editor";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_REPOSITORY_URL: &str =
    "https://github.com/lost-nd-xxx/something-like-balloon-editor/";

/// レイヤー定義: (キー名, 表示名, デフォルト色)
pub const LAYER_DEFS: &[(&str, &str, Rgb)] = &[
    ("base",  "塗り",   Rgb(180, 210, 255)),
    ("edge",  "縁取り", Rgb(255, 255, 255)),
    ("text",  "題字",   Rgb(29, 106, 184)),
    ("parts", "部品",   Rgb(29, 106, 184)),
];

/// アンドゥ/リドゥ用スナップショット
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub layer_colors:      HashMap<String, Rgb>,
    pub parts_colors:      HashMap<String, Rgb>,
    pub individual_colors: HashMap<String, IndivColors>,
    pub descript_text:     String,
    pub install_text:      String,
    pub individual_texts:  HashMap<String, String>,
    pub basic_info:        HashMap<String, String>,
    /// slbe_files.txt のテキスト内容（undo で復元するため保持）
    pub slbe_files_text:   String,
}

/// 個別バルーン色設定
#[derive(Debug, Clone, Default)]
pub struct IndivColors {
    pub base:        Option<Rgb>,
    pub edge:        Option<Rgb>,
    pub text:        Option<Rgb>,
    pub parts_colors: HashMap<String, Rgb>,
}

/// UIテーマ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

/// プレビューテキストモード
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewTextMode {
    #[default]
    A, // 通常サンプル
    B, // 折り返し幅確認
}

/// プレビュー背景
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CanvasBg {
    #[default]
    Checker, // チェッカー柄（透明）
    Solid(u8, u8, u8),
}

/// ドラッグ位置編集の対象パーツ
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragEditTarget {
    Arrow0,          // arrow0.x / arrow0.y
    Arrow1,          // arrow1.x / arrow1.y
    ClickWait,       // clickwaitmarker.x / .y
    SstpMarker,      // sstpmarker.x / .y
    SstpMessage,     // sstpmessage.x / .y
    OnlineMarker,    // onlinemarker.x / .y
    Counter,         // number.xr / number.y
    ValidRect,       // validrect.top/bottom/left/right（4辺まとめ）
    WordWrap,        // wordwrappoint.x
    CommunicateBox,  // communicatebox 4辺まとめ
}

/// ValidRect/CommunicateBox ドラッグ時にどの辺を操作しているか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectEdge { Top, Bottom, Left, Right }

/// 未保存確認ダイアログを経由して実行する保留アクション
#[derive(Debug, Clone)]
pub enum PendingAction {
    /// projects/ 配下のプロジェクトを開く（プロジェクト名）
    OpenProject(String),
    /// アプリを終了する
    Quit,
}

/// ドラッグ編集中の状態
#[derive(Debug, Clone)]
pub struct DragState {
    /// ドラッグ開始時のマウス位置（画像座標）
    pub start_img: egui::Pos2,
    /// ドラッグ開始時の値（x, y）
    pub start_val: (i32, i32),
    /// 現在のドラッグ中の仮の値（x, y）
    pub current_val: (i32, i32),
    /// ValidRect/CommunicateBox で操作中の辺（None = 自由ドラッグ）
    pub active_edge: Option<RectEdge>,
    /// 編集開始前の descript 生値が負値だったか (x軸, y軸)
    /// ValidRect/CommunicateBox の場合は active_edge の軸のみ意味を持つ
    pub was_negative: (bool, bool),
}

/// アプリ全体の状態
pub struct AppState {
    /// 選択中の素材フォルダの絶対パス（未選択時は None）
    pub selected_asset_dir: Option<PathBuf>,

    // --- 色設定 ---
    pub layer_colors:      HashMap<String, Rgb>,
    pub parts_colors:      HashMap<String, Rgb>,
    pub individual_colors: HashMap<String, IndivColors>,
    pub no_balloon_color:  bool,
    pub bulk_color_mode:   bool,

    // --- テキスト ---
    pub descript_text:    String,
    pub install_text:     String,
    pub readme_text:      String,
    pub individual_texts: HashMap<String, String>,
    pub basic_info:       HashMap<String, String>,
    /// slbe_files.txt のテキスト内容（メモリ上の編集バッファ）
    pub slbe_files_text:  String,

    // --- レイアウト ---
    pub balloon_layout:     HashMap<String, LayerList>,
    pub direct_image_mode:  bool,
    /// 奇数・偶数番バルーンの自動補完（反転生成）On/Off
    pub auto_flip:          bool,

    // --- バルーンリスト・選択 ---
    pub preview_balloons:  Vec<String>,   // "balloonk0.png" などの一覧
    pub selected_balloon:  String,        // 選択中バルーン名

    // --- キャッシュ ---
    /// 合成済みバルーン画像（テキスト描画前）
    pub balloon_cache:    HashMap<String, RgbaImage>,
    /// 色適用済みパーツ画像
    pub parts_cache:      HashMap<String, HashMap<String, RgbaImage>>,

    // --- UI状態 ---
    pub theme:              ThemeMode,
    pub preview_text_mode:  PreviewTextMode,
    pub overlay_mode:       String, // "" / "layout"
    pub canvas_bg:          CanvasBg,
    /// 単色背景の色（チェッカー選択中も保持）
    pub canvas_bg_solid_color: (u8, u8, u8),
    pub panel_left_width:   f32,
    pub panel_right_width:  f32,
    pub window_size:        [f32; 2],
    pub preview_generating: bool,
    pub show_bg_color_window: bool,

    // --- アンドゥ/リドゥ ---
    pub undo_stack: Vec<Snapshot>,
    pub redo_stack: Vec<Snapshot>,

    // --- 画像サイズから動的に計算したデフォルト値（descript.txtにキーがない場合のUI表示用）---
    pub dynamic_defaults: HashMap<String, String>,

    // --- テキスト/数値入力の編集中バッファ ---
    // フォーカス取得時の値を記憶し、Enter/フォーカス喪失時にコミットする
    pub editing_buf: Option<EditingBuf>,

    // --- ドラッグ位置編集 ---
    pub drag_edit_target: Option<DragEditTarget>,
    pub drag_state:       Option<DragState>,
    /// ValidRect/CommunicateBox 編集開始前の overlay_mode（編集終了時に復元）
    pub overlay_before_drag_edit: Option<String>,

    /// 「装飾なし」ON 直前の各キー値スナップショット。
    /// キーは "{cfg_key}::{blend_key}"、値は (descriptキー → 値) のマップ。
    /// OFF に戻したとき初期値ではなくこの保持値を復元する。
    pub decoration_backup: HashMap<String, HashMap<String, String>>,

    /// 左ペイン「バルーン」一覧の展開状態
    pub balloon_list_open: bool,
    /// 左ペイン「PNG一覧」の展開状態
    pub png_list_open: bool,

    // --- 読み込み時警告 ---
    /// 素材フォルダ読み込み時に蓄積された警告メッセージ。呼び出し元がダイアログ表示後クリアする。
    pub load_warnings: Vec<String>,

    // --- プロジェクト新規作成UI ---
    pub show_new_project_window: bool,
    pub new_project_name: String,
    pub new_project_warning: String,
    /// 未保存の編集があるか（dirtyフラグ）。保存/読み込み/新規で false に戻す。
    pub dirty: bool,
    /// 未保存確認ダイアログ後に実行する保留アクション
    pub pending_unsaved_action: Option<PendingAction>,
    /// 終了確認を通過して実際にウィンドウを閉じてよいか
    pub allow_close: bool,

    // --- プロジェクトを開くUI ---
    pub show_open_project_window: bool,
    /// projects/ 配下のプロジェクト名一覧（ダイアログ表示用）
    pub open_project_list: Vec<String>,
    /// 一覧の選択中インデックス（フィルタ後リスト上のインデックス）
    pub open_project_selected: Option<usize>,
    /// 絞り込みフィルタ文字列
    pub open_project_filter: String,

    // --- フォルダからプロジェクト作成UI ---
    pub show_import_folder_window: bool,
    /// インポート元フォルダパス
    pub import_folder_src: std::path::PathBuf,
    /// 作成するプロジェクト名（初期値 = ソースフォルダ名）
    pub import_folder_project_name: String,
    pub import_folder_warning: String,

    // --- 別名で保存UI ---
    pub show_save_as_project_window: bool,
    pub save_as_project_name: String,
    pub save_as_project_warning: String,

    // --- 非同期処理フラグ ---
    pub pending_reload: bool,   // 次フレームで reload_asset_folder_keep_texts を実行
    /// PNG一覧からプレビュー中のファイル名（selected_balloon とは独立）
    pub png_preview_name: Option<String>,

    // --- ファイル名変更UI ---
    pub show_rename_window: bool,
    pub rename_target: String,       // 変更前ファイル名（拡張子含む）
    pub rename_new_name: String,     // 変更後ファイル名（拡張子なし）、PNG一覧用
    pub rename_warning: String,
    /// true = バルーンリスト由来（構造化入力UI）
    pub rename_is_balloon: bool,
    /// false = 通常バルーン（s/k/p系）、true = 入力ボックス（c系）
    pub rename_balloon_is_c: bool,
    /// スコープ番号（0=s系, 1=k系, 2以上=pNdef系）
    pub rename_scope_num: i32,
    /// ID番号
    pub rename_id_num: u32,

    // --- files.txt エディタUI ---
    pub show_files_editor_window: bool,

    // --- 画像インポートUI（キュー対応） ---
    pub show_import_window: bool,
    pub import_queue: Vec<std::path::PathBuf>,
    pub import_queue_index: usize,
    pub import_category: String,
    pub import_is_scope_specific: bool,
    pub import_scope_num: i32,
    /// 汎用ID（balloon/online のID番号など）
    pub import_target_id_num: u32,
    /// 矢印の向き: 0=上, 1=下
    pub import_arrow_dir: u8,
    /// 入力ボックス (balloonc) のID: 0〜4
    pub import_balloonc_id: usize,
    /// inputbtn カテゴリのボタン種別: "ok" / "cancel" / "mode"
    pub import_inputbtn_kind: String,
    /// inputbtn カテゴリのボタン状態: "up" / "down"
    pub import_inputbtn_state: String,
    pub import_custom_filename: String,
    /// D&Dファイルに同名 .pna が存在するか（情報表示用）
    pub import_has_pna: bool,
    /// インポートダイアログ用プレビューテクスチャ（現在のキュー画像）
    pub import_preview_texture: Option<egui::TextureHandle>,
}

/// テキスト・数値入力の編集中バッファ
#[derive(Debug, Clone)]
pub struct EditingBuf {
    /// どのフィールドを編集中か（field.key）
    pub field_key: String,
    /// フォーカス取得時点の値（変化判定用）
    pub committed: String,
    /// 現在の入力中の値
    pub current: String,
}

impl AppState {
    pub fn new() -> Self {
        let default_colors: HashMap<String, Rgb> = LAYER_DEFS
            .iter()
            .map(|&(k, _, c)| (k.to_string(), c))
            .collect();
        let parts_color = default_colors["parts"];

        Self {
            selected_asset_dir: None,
            layer_colors:      default_colors,
            parts_colors:      {
                let mut m = HashMap::new();
                m.insert("all".to_string(), parts_color);
                m
            },
            individual_colors: HashMap::new(),
            no_balloon_color:  false,
            bulk_color_mode:   true,
            descript_text:     String::new(),
            install_text:      String::new(),
            readme_text:       String::new(),
            individual_texts:  HashMap::new(),
            basic_info:        HashMap::new(),
            slbe_files_text:   String::new(),
            balloon_layout:    HashMap::new(),
            direct_image_mode: false,
            auto_flip:         true,
            preview_balloons:  Vec::new(),
            selected_balloon:  String::new(),
            balloon_cache:     HashMap::new(),
            parts_cache:       HashMap::new(),
            theme:               ThemeMode::Light,
            panel_left_width:    150.0,
            panel_right_width:   320.0,
            window_size:         [1400.0, 720.0],
            preview_generating:  false,
            show_bg_color_window: false,
            preview_text_mode: PreviewTextMode::A,
            overlay_mode:      String::new(),
            canvas_bg:         CanvasBg::Checker,
            canvas_bg_solid_color: (255, 255, 255),
            undo_stack:        Vec::new(),
            redo_stack:        Vec::new(),
            editing_buf:       None,
            dynamic_defaults:  HashMap::new(),
            drag_edit_target:  None,
            drag_state:        None,
            overlay_before_drag_edit: None,
            decoration_backup: HashMap::new(),
            balloon_list_open: true,
            png_list_open: true,
            load_warnings:     Vec::new(),

            // --- プロジェクト新規作成UI ---
            show_new_project_window: false,
            new_project_name: String::new(),
            new_project_warning: String::new(),
            dirty: false,
            pending_unsaved_action: None,
            allow_close: false,

            // --- プロジェクトを開くUI ---
            show_open_project_window: false,
            open_project_list: Vec::new(),
            open_project_selected: None,
            open_project_filter: String::new(),

            // --- フォルダからプロジェクト作成UI ---
            show_import_folder_window: false,
            import_folder_src: std::path::PathBuf::new(),
            import_folder_project_name: String::new(),
            import_folder_warning: String::new(),

            // --- 別名で保存UI ---
            show_save_as_project_window: false,
            save_as_project_name: String::new(),
            save_as_project_warning: String::new(),

            // --- files.txt エディタUI ---
            show_files_editor_window: false,

            // --- 非同期処理フラグ ---
            pending_reload: false,
            png_preview_name: None,

            // --- ファイル名変更UI ---
            show_rename_window: false,
            rename_target: String::new(),
            rename_new_name: String::new(),
            rename_warning: String::new(),
            rename_is_balloon: false,
            rename_balloon_is_c: false,
            rename_scope_num: 0,
            rename_id_num: 0,

            // --- 画像インポートUI（キュー対応） ---
            show_import_window: false,
            import_queue: Vec::new(),
            import_queue_index: 0,
            import_category: "balloon".to_string(),
            import_is_scope_specific: true,
            import_scope_num: 0,
            import_target_id_num: 0,
            import_arrow_dir: 0,
            import_balloonc_id: 0,
            import_inputbtn_kind: "ok".to_string(),
            import_inputbtn_state: "up".to_string(),
            import_custom_filename: String::new(),
            import_has_pna: false,
            import_preview_texture: None,
        }
    }

    /// 指定 stem のバルーン名が既に存在するか（リネーム重複判定用）。
    /// 「物理ファイル（任意拡張子）が存在する」OR「files.txt に同名定義がある」場合に true。
    pub fn balloon_name_exists(&self, stem: &str) -> bool {
        // files.txt（slbe_files_text）の先頭カラムに同名定義があるか
        let in_files_txt = self.slbe_files_text.lines().any(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.is_empty() { return false; }
            trimmed.split(',').next().map(|f| f.trim()) == Some(stem)
        });
        if in_files_txt { return true; }

        // 物理ファイル（拡張子問わず stem 一致）が存在するか
        if let Some(asset_dir) = self.asset_dir() {
            if let Ok(entries) = std::fs::read_dir(&asset_dir) {
                for e in entries.flatten() {
                    let path = e.path();
                    let fstem = path.file_stem().and_then(|s| s.to_str());
                    // .pna は対象外（アルファ用の付随ファイル）
                    let ext = path.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase());
                    if ext.as_deref() == Some("pna") { continue; }
                    if fstem == Some(stem) { return true; }
                }
            }
        }
        false
    }

    /// 選択中バルーンが balloonc 系かどうか
    pub fn is_balloonc(&self) -> bool {
        let name = self.selected_balloon.trim_end_matches(".png");
        name.starts_with("balloonc")
    }

    /// 選択中の素材フォルダの絶対パスを返す
    pub fn asset_dir(&self) -> Option<PathBuf> {
        self.selected_asset_dir.as_ref().map(|p| {
            // Windows の \\?\ UNC long-path プレフィックスを除去して通常パスを返す
            let s = p.to_string_lossy();
            if s.starts_with(r"\\?\") {
                std::path::PathBuf::from(&s[4..])
            } else {
                p.clone()
            }
        })
    }

    /// 選択中の素材フォルダが projects/ 配下のプロジェクトかどうかを返す。
    /// projects/ 配下でない場合（外部素材フォルダ読み込み）はインポート不可。
    pub fn is_project_dir(&self) -> bool {
        let Some(asset_dir) = self.asset_dir() else { return false; };
        let Ok(base) = crate::core::project::get_projects_base_dir() else { return false; };
        asset_dir.starts_with(&base)
    }

    /// descript.txt 内の "directory"（インストール先フォルダ名）を取得します。
    pub fn get_install_directory_name(&self) -> Option<String> {
        if self.descript_text.is_empty() {
            return None;
        }
        let parsed = crate::core::descript::parse_descript(&self.descript_text);
        parsed.get("directory").cloned()
    }

    /// 素材フォルダの表示名（フォルダ名部分のみ）を返す
    pub fn asset_dir_name(&self) -> Option<String> {
        self.selected_asset_dir.as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }

    /// 現在の ColorSet を生成する（no_balloon_color を反映）
    pub fn color_set(&self) -> ColorSet {
        self.color_set_for(None)
    }

    /// 指定バルーン名の ColorSet を生成する。
    /// individual_colors にそのバルーンの個別色があれば優先する。
    pub fn color_set_for(&self, balloon_name: Option<&str>) -> ColorSet {
        let mut cs = ColorSet::default();
        if let Some(&c) = self.layer_colors.get("base")  { cs.base  = c; }
        if let Some(&c) = self.layer_colors.get("edge")  { cs.edge  = c; }
        if let Some(&c) = self.layer_colors.get("text")  { cs.text  = c; }
        if let Some(&c) = self.layer_colors.get("parts") { cs.parts = c; }
        // 画像編集なしモードでは常に色オーバーレイを無効にする
        cs.no_color = self.no_balloon_color || self.direct_image_mode;

        // 個別色で上書き
        if let Some(name) = balloon_name {
            if let Some(indiv) = self.individual_colors.get(name) {
                if let Some(c) = indiv.base  { cs.base  = c; }
                if let Some(c) = indiv.edge  { cs.edge  = c; }
                if let Some(c) = indiv.text  { cs.text  = c; }
                // parts は "all" キーで管理
                if let Some(&c) = indiv.parts_colors.get("all") { cs.parts = c; }
            }
        }
        cs
    }

    /// 選択バルーン用の parts_colors を返す（個別色があれば優先）
    pub fn parts_colors_for(&self, balloon_name: Option<&str>) -> HashMap<String, Rgb> {
        let mut pc = self.parts_colors.clone();
        if let Some(name) = balloon_name {
            if let Some(indiv) = self.individual_colors.get(name) {
                if !indiv.parts_colors.is_empty() {
                    pc = indiv.parts_colors.clone();
                }
            }
        }
        pc
    }

    /// アンドゥスナップショットを積む（最大20件）
    pub fn push_undo(&mut self) {
        let snap = self.snapshot();
        self.undo_stack.push(snap);
        if self.undo_stack.len() > 20 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.dirty = true;
    }

    /// アンドゥを実行する。バルーン画像の再構築が必要なら true を返す。
    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_stack.pop() {
            let current = self.snapshot();
            let needs_rebuild = snap.layer_colors != current.layer_colors
                || snap.parts_colors != current.parts_colors;
            self.redo_stack.push(current);
            self.apply_snapshot(snap);
            self.dirty = true;
            needs_rebuild
        } else {
            false
        }
    }

    /// リドゥを実行する。バルーン画像の再構築が必要なら true を返す。
    pub fn redo(&mut self) -> bool {
        if let Some(snap) = self.redo_stack.pop() {
            let current = self.snapshot();
            let needs_rebuild = snap.layer_colors != current.layer_colors
                || snap.parts_colors != current.parts_colors;
            self.undo_stack.push(current);
            self.apply_snapshot(snap);
            self.dirty = true;
            needs_rebuild
        } else {
            false
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            layer_colors:      self.layer_colors.clone(),
            parts_colors:      self.parts_colors.clone(),
            individual_colors: self.individual_colors.clone(),
            descript_text:     self.descript_text.clone(),
            install_text:      self.install_text.clone(),
            individual_texts:  self.individual_texts.clone(),
            basic_info:        self.basic_info.clone(),
            slbe_files_text:   self.slbe_files_text.clone(),
        }
    }

    fn apply_snapshot(&mut self, snap: Snapshot) {
        self.layer_colors      = snap.layer_colors;
        self.parts_colors      = snap.parts_colors;
        self.individual_colors = snap.individual_colors;
        self.descript_text     = snap.descript_text;
        self.install_text      = snap.install_text;
        self.individual_texts  = snap.individual_texts;
        self.basic_info        = snap.basic_info;
        self.slbe_files_text   = snap.slbe_files_text;
        self.balloon_cache.clear();
        // slbe_files_text が変化した場合に備えて balloon_layout を再パース
        if let Some(asset_dir) = self.asset_dir() {
            match crate::core::layout::parse_balloon_layout(
                &self.slbe_files_text,
                &asset_dir,
            ) {
                Ok(layout) => {
                    self.balloon_layout    = layout;
                    self.direct_image_mode = crate::core::layout::is_direct_image_mode(&self.balloon_layout);
                }
                Err(_) => {} // パース失敗時は現状のまま保持
            }
        }
    }
}
