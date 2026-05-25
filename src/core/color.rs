/// RGB色を表す型（各成分0〜255）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn r(self) -> u8 { self.0 }
    pub fn g(self) -> u8 { self.1 }
    pub fn b(self) -> u8 { self.2 }

    /// "#RRGGBB" 形式の文字列をパースする
    #[allow(dead_code)]
    pub fn from_hex(s: &str) -> anyhow::Result<Self> {
        let s = s.trim_start_matches('#');
        if s.len() != 6 {
            anyhow::bail!("不正な色コード: #{}", s);
        }
        let r = u8::from_str_radix(&s[0..2], 16)?;
        let g = u8::from_str_radix(&s[2..4], 16)?;
        let b = u8::from_str_radix(&s[4..6], 16)?;
        Ok(Rgb(r, g, b))
    }

    /// "#RRGGBB" 形式の文字列に変換する
    #[allow(dead_code)]
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// 2色の平均を返す
    #[allow(dead_code)]
    pub fn blend(self, other: Rgb) -> Rgb {
        Rgb(
            ((self.0 as u16 + other.0 as u16) / 2) as u8,
            ((self.1 as u16 + other.1 as u16) / 2) as u8,
            ((self.2 as u16 + other.2 as u16) / 2) as u8,
        )
    }
}

/// 4レイヤー分の色設定
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ColorSet {
    pub base:     Rgb,
    pub edge:     Rgb,
    pub text:     Rgb,
    pub parts:    Rgb,
    /// true のとき色オーバーレイをスキップして元画像のまま出力する
    pub no_color: bool,
}

impl ColorSet {
    pub fn new(base: Rgb, edge: Rgb, text: Rgb, parts: Rgb) -> Self {
        Self { base, edge, text, parts, no_color: false }
    }
}

impl Default for ColorSet {
    fn default() -> Self {
        Self::new(
            Rgb(180, 210, 255),
            Rgb(255, 255, 255),
            Rgb(29, 106, 184),
            Rgb(29, 106, 184),
        )
    }
}

/// WCAG 2.x の相対輝度（0.0〜1.0）を計算する
pub fn relative_luminance(color: Rgb) -> f64 {
    fn linearize(c: u8) -> f64 {
        let v = c as f64 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(color.0) + 0.7152 * linearize(color.1) + 0.0722 * linearize(color.2)
}

/// WCAG コントラスト比（1.0〜21.0）を計算する
pub fn contrast_ratio(c1: Rgb, c2: Rgb) -> f64 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

/// コントラスト比からWCAGレベル文字列を返す
pub fn wcag_level(ratio: f64) -> &'static str {
    if ratio >= 7.0 { "AAA" }
    else if ratio >= 4.5 { "AA" }
    else if ratio >= 3.0 { "AA(大文字)" }
    else { "NG" }
}

/// APCA 相対輝度（Ys）を計算する
fn apca_luminance(color: Rgb) -> f64 {
    fn linearize(c: u8) -> f64 {
        (c as f64 / 255.0).powf(2.4)
    }
    0.2126729 * linearize(color.0)
        + 0.7151522 * linearize(color.1)
        + 0.0721750 * linearize(color.2)
}

/// APCA Lc値を計算する（テキスト色, 背景色の順）
/// 返値の符号: 正 = 暗テキスト on 明背景、負 = 明テキスト on 暗背景
pub fn apca_lc(text_color: Rgb, bg_color: Rgb) -> f64 {
    const NORM_BG:  f64 = 0.56;
    const NORM_TXT: f64 = 0.57;
    const REV_BG:   f64 = 0.65;
    const REV_TXT:  f64 = 0.62;
    const SCALE:    f64 = 1.14;
    const BLACK_CLAMP: f64 = 0.022;
    const BLACK_EXP:   f64 = 1.414;
    const DELTA_LIM:   f64 = 0.001;
    const OFFSET:      f64 = 0.027;

    let mut yt = apca_luminance(text_color);
    let mut yb = apca_luminance(bg_color);

    // ブラッククランプ
    if yt < BLACK_CLAMP { yt += (BLACK_CLAMP - yt).powf(BLACK_EXP); }
    if yb < BLACK_CLAMP { yb += (BLACK_CLAMP - yb).powf(BLACK_EXP); }

    let delta = (yb - yt).abs();
    if delta < DELTA_LIM { return 0.0; }

    let lc = if yb > yt {
        // 暗テキスト on 明背景（正値）
        (yb.powf(NORM_BG) - yt.powf(NORM_TXT)) * SCALE
    } else {
        // 明テキスト on 暗背景（負値）
        (yb.powf(REV_BG)  - yt.powf(REV_TXT))  * SCALE
    };

    if lc.abs() < OFFSET { 0.0 }
    else if lc > 0.0 { (lc - OFFSET) * 100.0 }
    else             { (lc + OFFSET) * 100.0 }
}

/// APCA Bronze Simple Mode 合否を返す（|Lc| >= 60 で合格）
pub fn apca_bronze(lc: f64) -> &'static str {
    if lc.abs() >= 60.0 { "Bronze OK" } else { "Bronze NG" }
}
