/// プロファイル（編集状態）のJSONシリアライズ・デシリアライズを担う
use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::core::color::Rgb;

const PROFILE_VERSION: u32 = 1;

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

/// 保存・読み込みに使うプロファイルの全データ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub version: u32,

    /// 選択中の素材フォルダの絶対パス
    #[serde(default)]
    pub asset_dir: String,

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

    /// descript.txt の編集中テキスト
    #[serde(default)]
    pub descript_text: String,

    /// install.txt の編集中テキスト
    #[serde(default)]
    pub install_text: String,

    /// 個別設定ファイルテキスト: {設定ファイル名: テキスト}
    #[serde(default)]
    pub individual_texts: HashMap<String, String>,

    /// 基本情報フィールド: {"name": "...", "craftman": "...", ...}
    #[serde(default)]
    pub basic_info: HashMap<String, String>,

    /// 選択中のバルーン名（例: "balloonk0.png"）
    #[serde(default)]
    pub selected_balloon: String,

    /// UIテーマ: "system" / "light" / "dark"
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

fn default_true() -> bool { true }

impl Default for Profile {
    fn default() -> Self {
        Self {
            version:           PROFILE_VERSION,
            asset_dir:         String::new(),
            layer_colors:      HashMap::new(),
            parts_colors:      HashMap::new(),
            individual_colors: HashMap::new(),
            no_balloon_color:  false,
            bulk_color_mode:   true,
            descript_text:     String::new(),
            install_text:      String::new(),
            individual_texts:  HashMap::new(),
            basic_info:        HashMap::new(),
            selected_balloon:   String::new(),
            theme:              String::new(),
            panel_left_width:   0.0,
            panel_right_width:  0.0,
            window_size:        [0.0, 0.0],
        }
    }
}

impl Profile {
    /// レイヤー色を Rgb で取得する（なければデフォルト値）
    pub fn layer_color(&self, key: &str, default: Rgb) -> Rgb {
        self.layer_colors
            .get(key)
            .map(|&[r, g, b]| Rgb(r, g, b))
            .unwrap_or(default)
    }

}

/// プロファイルをJSONファイルから読み込む。失敗時はエラーを返す。
pub fn load_profile(path: &Path) -> anyhow::Result<Profile> {
    let text = std::fs::read_to_string(path)?;
    let profile: Profile = serde_json::from_str(&text)?;
    validate_profile(&profile)?;
    Ok(profile)
}

/// プロファイルをJSONファイルへアトミックに保存する。
/// 一時ファイル（.tmp）に書いてからリネームすることで、
/// 書き込み中断でファイルが破損しないようにする。
pub fn save_profile(path: &Path, profile: &Profile) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(profile)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// プロファイルの色値を簡易検証する
fn validate_profile(profile: &Profile) -> anyhow::Result<()> {
    for (key, &[r, g, b]) in &profile.layer_colors {
        validate_rgb(r, g, b, &format!("layer_colors.{}", key))?;
    }
    for (key, &[r, g, b]) in &profile.parts_colors {
        validate_rgb(r, g, b, &format!("parts_colors.{}", key))?;
    }
    Ok(())
}

fn validate_rgb(r: u8, g: u8, b: u8, label: &str) -> anyhow::Result<()> {
    // u8 は 0〜255 の範囲を型で保証しているため、追加チェック不要。
    // 将来的に JSON が [u32; 3] などになった場合のために枠だけ残す。
    let _ = (r, g, b, label);
    Ok(())
}

/// 起動時の状態ファイルを読み込む。ファイルがなければ None を返す。
/// 前回の保存が中断して .tmp が残っている場合は削除してから読む。
pub fn load_state(state_path: &Path) -> Option<Profile> {
    if !state_path.exists() {
        return None;
    }
    let tmp = state_path.with_extension("tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    load_profile(state_path).ok()
}
