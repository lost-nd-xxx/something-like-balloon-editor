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
    "https://github.com/lost-nd-xxx/something_like_balloon_editor/";

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

    // --- レイアウト ---
    pub balloon_layout:     HashMap<String, LayerList>,
    pub direct_image_mode:  bool,

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
    #[allow(dead_code)]
    pub edit_mode:          String, // "" / パーツID
    pub panel_left_width:   f32,
    pub panel_right_width:  f32,
    pub window_size:        [f32; 2],
    pub preview_generating: bool,
    pub show_bg_color_window: bool,

    // --- パーツターゲット ---
    #[allow(dead_code)]
    pub parts_targets:      Vec<(String, String)>, // (表示名, キー)
    #[allow(dead_code)]
    pub parts_target:       String,                // 選択中の表示名

    // --- アンドゥ/リドゥ ---
    pub undo_stack: Vec<Snapshot>,
    pub redo_stack: Vec<Snapshot>,

    // --- 読み込み中フラグ ---
    #[allow(dead_code)]
    pub loading: bool,

    // --- 画像サイズから動的に計算したデフォルト値（descript.txtにキーがない場合のUI表示用）---
    pub dynamic_defaults: HashMap<String, String>,

    // --- テキスト/数値入力の編集中バッファ ---
    // フォーカス取得時の値を記憶し、Enter/フォーカス喪失時にコミットする
    pub editing_buf: Option<EditingBuf>,

    // --- ドラッグ位置編集 ---
    pub drag_edit_target: Option<DragEditTarget>,
    pub drag_state:       Option<DragState>,

    // --- 読み込み時警告 ---
    /// 素材フォルダ読み込み時に蓄積された警告メッセージ。呼び出し元がダイアログ表示後クリアする。
    pub load_warnings: Vec<String>,
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
            balloon_layout:    HashMap::new(),
            direct_image_mode: false,
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
            edit_mode:         String::new(),
            parts_targets:     vec![("全て".to_string(), "all".to_string())],
            parts_target:      "全て".to_string(),
            undo_stack:        Vec::new(),
            redo_stack:        Vec::new(),
            loading:           false,
            editing_buf:       None,
            dynamic_defaults:  HashMap::new(),
            drag_edit_target:  None,
            drag_state:        None,
            load_warnings:     Vec::new(),
        }
    }

    /// 選択中バルーンが balloonc 系かどうか
    pub fn is_balloonc(&self) -> bool {
        let name = self.selected_balloon.trim_end_matches(".png");
        name.starts_with("balloonc")
    }

    /// 選択中の素材フォルダの絶対パスを返す
    pub fn asset_dir(&self) -> Option<PathBuf> {
        self.selected_asset_dir.clone()
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
    }

    /// アンドゥを実行する。バルーン画像の再構築が必要なら true を返す。
    pub fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo_stack.pop() {
            let current = self.snapshot();
            let needs_rebuild = snap.layer_colors != current.layer_colors
                || snap.parts_colors != current.parts_colors;
            self.redo_stack.push(current);
            self.apply_snapshot(snap);
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
        self.balloon_cache.clear();
    }
}
