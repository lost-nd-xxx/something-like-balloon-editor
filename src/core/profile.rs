/// プロジェクト固有データ（色設定）とアプリ固有状態（UI状態）の
/// JSONシリアライズ・デシリアライズを担う。
///
/// 設計方針（プロジェクト保存方式の見直し後）:
/// - SSPテキスト（descript.txt / install.txt / 個別txt）は txt が正本。JSONには保持しない。
/// - プロジェクト固有データ（色設定）→ プロジェクト内 `profile/slbe_profile.json`（ProjectProfile）
/// - アプリ固有状態（テーマ・パネル幅・ウィンドウサイズ・最後に開いたプロジェクト）
///   → 実行ファイル階層の `state.json`（AppConfig）
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::core::color::Rgb;

const PROFILE_VERSION: u32 = 2;

/// 個別バルーン設定の色情報
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualColors {
    #[serde(default)]
    pub base:  Option<[u8; 3]>,
    #[serde(default)]
    pub edge:  Option<[u8; 3]>,
    #[serde(default)]
    pub text:  Option<[u8; 3]>,
    /// パーツ個別色: {"all": [r,g,b], "arrow0": [r,g,b], ...}
    #[serde(default)]
    pub parts_colors: HashMap<String, [u8; 3]>,
}

// ============================================================================
// ProjectProfile: プロジェクト固有データ（色設定）→ profile/slbe_profile.json
// ============================================================================

/// プロジェクト固有の色設定。プロジェクト内 `profile/slbe_profile.json` に保存する。
/// 出力（export）時には書き出さない。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProfile {
    #[serde(default)]
    pub version: u32,

    /// レイヤー色: {"base": [r,g,b], "edge": ..., "text": ..., "parts": ...}
    #[serde(default)]
    pub layer_colors: HashMap<String, [u8; 3]>,

    /// パーツ個別色: {"all": [r,g,b], "arrow0": [r,g,b], ...}
    #[serde(default)]
    pub parts_colors: HashMap<String, [u8; 3]>,

    /// バルーンごとの個別色設定: {設定ファイル名: IndividualColors}
    #[serde(default)]
    pub individual_colors: HashMap<String, IndividualColors>,

    /// バルーン画像への色オーバーレイを無効にするか
    #[serde(default)]
    pub no_balloon_color: bool,

    /// 色を全バルーンに一括適用するか
    #[serde(default = "default_true")]
    pub bulk_color_mode: bool,
}

fn default_true() -> bool { true }

impl Default for ProjectProfile {
    fn default() -> Self {
        Self {
            version:           PROFILE_VERSION,
            layer_colors:      HashMap::new(),
            parts_colors:      HashMap::new(),
            individual_colors: HashMap::new(),
            no_balloon_color:  false,
            bulk_color_mode:   true,
        }
    }
}

impl ProjectProfile {
    /// レイヤー色を Rgb で取得する（なければデフォルト値）
    pub fn layer_color(&self, key: &str, default: Rgb) -> Rgb {
        self.layer_colors
            .get(key)
            .map(|&[r, g, b]| Rgb(r, g, b))
            .unwrap_or(default)
    }
}

/// プロジェクトプロファイル（slbe_profile.json）の保存先パスを返す。
/// プロジェクト直下の `profile/slbe_profile.json`。
pub fn project_profile_path(asset_dir: &Path) -> std::path::PathBuf {
    asset_dir.join("profile").join("slbe_profile.json")
}

/// プロジェクトプロファイルをJSONファイルから読み込む。失敗時はエラーを返す。
pub fn load_project_profile(path: &Path) -> anyhow::Result<ProjectProfile> {
    let text = std::fs::read_to_string(path)?;
    let profile: ProjectProfile = serde_json::from_str(&text)?;
    Ok(profile)
}

/// プロジェクトプロファイルをJSONファイルへアトミックに保存する。
/// 一時ファイル（.tmp）に書いてからリネームすることで、
/// 書き込み中断でファイルが破損しないようにする。
/// 親ディレクトリ（profile/）が無ければ作成する。
pub fn save_project_profile(path: &Path, profile: &ProjectProfile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(profile)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ============================================================================
// AppConfig: アプリ固有状態（UI状態）→ state.json
// ============================================================================

/// アプリ固有の状態。実行ファイル階層の `state.json` に保存する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub version: u32,

    /// 最後に開いたプロジェクトの素材フォルダ絶対パス
    #[serde(default)]
    pub asset_dir: String,

    /// UIテーマ: "light" / "dark"
    #[serde(default)]
    pub theme: String,

    /// 左ペイン幅
    #[serde(default)]
    pub panel_left_width: f32,

    /// 右ペイン幅
    #[serde(default)]
    pub panel_right_width: f32,

    /// ウィンドウサイズ [width, height]
    #[serde(default)]
    pub window_size: [f32; 2],
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version:           PROFILE_VERSION,
            asset_dir:         String::new(),
            theme:             String::new(),
            panel_left_width:  0.0,
            panel_right_width: 0.0,
            window_size:       [0.0, 0.0],
        }
    }
}

/// アプリ設定をJSONファイルへアトミックに保存する。
pub fn save_app_config(path: &Path, config: &AppConfig) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(config)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 起動時の状態ファイル（state.json）を読み込む。ファイルがなければ None を返す。
/// 前回の保存が中断して .tmp が残っている場合は削除してから読む。
pub fn load_app_config(state_path: &Path) -> Option<AppConfig> {
    if !state_path.exists() {
        return None;
    }
    let tmp = state_path.with_extension("tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    let text = std::fs::read_to_string(state_path).ok()?;
    serde_json::from_str(&text).ok()
}
