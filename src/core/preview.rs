/// バルーン画像へのサンプルテキスト・パーツ描画
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use image::{RgbaImage, Rgba};
use crate::core::color::Rgb;
use crate::core::descript::{parse_descript, get_color_from_descript, pos_str};
use crate::core::gdi_text;

#[cfg(windows)]
use crate::core::gdi_text::GdiSession;

/// draw_preview 全体で使い回す GDI セッションのラッパー
/// Windows 以外では None のまま fontdue フォールバックを使う
struct Sess {
    #[cfg(windows)]
    inner: Option<GdiSession>,
    #[allow(dead_code)]
    font_name: String,
    size_px: f32,
    #[allow(dead_code)]
    no_aa: bool,
}

impl Sess {
    fn new(font_name: &str, size_px: f32, no_aa: bool) -> Self {
        #[cfg(windows)]
        {
            let gdi_name = font_name.split(',').next().unwrap_or(font_name).trim();
            Self { inner: GdiSession::new(gdi_name, size_px, no_aa), font_name: font_name.to_string(), size_px, no_aa }
        }
        #[cfg(not(windows))]
        {
            let _ = (font_name, size_px, no_aa);
            Self { font_name: font_name.to_string(), size_px, no_aa }
        }
    }

    fn draw(
        &mut self,
        img: &mut RgbaImage,
        text: &str,
        px: i32, py: i32,
        color: Rgb,
        shadow: Option<(Rgb, bool)>,
        max_x: Option<i32>,
        size_px: f32,
    ) -> bool {
        #[cfg(windows)]
        // サイズが異なる場合はセッションを使えない → 呼び出し元の単発GDIへフォールバック
        if (size_px - self.size_px).abs() > 0.1 { return false; }
        #[cfg(windows)]
        if let Some(sess) = &mut self.inner {
            let clipped = match max_x {
                Some(mx) => sess.clip_to_max_x(text, px, mx),
                None => text.to_string(),
            };
            let text = clipped.as_str();
            if text.is_empty() { return true; }
            // 実際のテキスト幅を GDI で計測してからビットマップサイズを決める
            let measured_w = sess.measure(text);
            let bmp_w = (measured_w + 4.0) as i32;
            let bmp_h = (size_px * 1.5 + 4.0) as i32;
            if bmp_w == 0 || bmp_h == 0 { return true; }
            sess.ensure_bmp(bmp_w, bmp_h);
            sess.render_to_bmp(text, bmp_w, bmp_h);
            let stride = sess.bmp_stride();
            let alpha_mask: Vec<u32> = sess.raw_buf[..(stride * bmp_h) as usize].to_vec();
            if let Some((sc, is_outline)) = shadow {
                if is_outline {
                    for (ddx, ddy) in [(-1i32,-1i32),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                        gdi_text::paste_raw(img, &alpha_mask, stride, bmp_w, bmp_h, px + ddx, py + ddy, sc);
                    }
                } else {
                    gdi_text::paste_raw(img, &alpha_mask, stride, bmp_w, bmp_h, px + 1, py + 1, sc);
                }
            }
            gdi_text::paste_raw(img, &alpha_mask, stride, bmp_w, bmp_h, px, py, color);
            return true;
        }
        #[cfg(not(windows))]
        let _ = (img, text, px, py, color, shadow, max_x, size_px);
        false
    }

    /// GDI フォントの internal leading（グリフ上端より上の余白ピクセル数）を返す
    fn internal_leading(&self) -> i32 {
        #[cfg(windows)]
        if let Some(sess) = &self.inner {
            return sess.internal_leading();
        }
        0
    }

    fn measure_with_size(&self, text: &str, size_px: f32) -> Option<f32> {
        // サイズが一致しない場合は None（呼び出し元で単発GDIにフォールバック）
        if (size_px - self.size_px).abs() > 0.1 { return None; }
        #[cfg(windows)]
        if let Some(sess) = &self.inner {
            return Some(sess.measure(text));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode { A, B }

pub struct DrawOptions {
    pub mode:         PreviewMode,
    pub show_overlay: bool,
    pub is_balloonc:  bool,
    pub balloon_name: String,
    /// 素材フォルダのパス（フォント検索に使用）
    pub asset_dir:    Option<PathBuf>,
}

impl Default for DrawOptions {
    fn default() -> Self {
        DrawOptions {
            mode:         PreviewMode::A,
            show_overlay: false,
            is_balloonc:  false,
            balloon_name: String::new(),
            asset_dir:    None,
        }
    }
}

struct ValidRect { left: i32, top: i32, right: i32, bottom: i32 }

// ---------------------------------------------------------------------------
// エントリポイント
// ---------------------------------------------------------------------------

pub fn draw_preview(
    base: &RgbaImage,
    descript_text: &str,
    parts_cache: &HashMap<String, RgbaImage>,
    opts: &DrawOptions,
) -> RgbaImage {
    let mut img = base.clone();
    let (w, h) = (img.width() as i32, img.height() as i32);
    let parsed = parse_descript(descript_text);
    let vr = build_valid_rect(&parsed, w, h);

    // font.name を読む（空・未設定はＭＳ ゴシックへフォールバック）
    let font_name = {
        let v = parsed.get("font.name").map(|s| s.trim().to_string()).unwrap_or_default();
        if v.is_empty() { "ＭＳ ゴシック".to_string() } else { v }
    };
    let no_aa = is_bitmap_font(&font_name);

    // 素材フォルダ内の同梱フォントを GDI にプライベート登録（Drop 時に自動解除）
    let _private_font_guard = opts.asset_dir.as_deref().map(|dir| {
        gdi_text::PrivateFontGuard::register_from_dir(&font_name, dir)
    });

    // GDI セッション（フォント高さ = font.height の値、なければ 12）
    let font_height: f32 = parsed.get("font.height")
        .and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let mut sess = Sess::new(&font_name, font_height, no_aa);

    // fontdue はフォールバック用（GDI が使えない環境向け）
    let parsed_font = resolve_and_parse_font(&font_name, opts.asset_dir.as_deref());

    let parsed_font_ref = parsed_font.as_deref();
    if opts.is_balloonc {
        draw_communicatebox(&mut img, &parsed, parts_cache, &opts.balloon_name, &font_name, parsed_font_ref, &mut sess);
    } else {
        draw_parts(&mut img, &parsed, parts_cache, w, h, &opts.balloon_name, &font_name, parsed_font_ref, no_aa, &mut sess);
        draw_sample_text(&mut img, &parsed, &vr, parts_cache, opts.mode, &opts.balloon_name, &font_name, parsed_font_ref, no_aa, &mut sess);
    }

    if opts.show_overlay {
        draw_overlay(&mut img, &parsed, &vr, opts.is_balloonc, w, h);
    }

    img
}

// ---------------------------------------------------------------------------
// フォント解決
// ---------------------------------------------------------------------------

/// font.name 文字列（カンマ区切り可）から使えるフォントデータを返す。
/// 解決順: 1. asset_dir 内のフォントファイル
///         2. Windows システムフォントフォルダ
///         3. ＭＳ ゴシック → メイリオ（最終フォールバック）
/// システムフォント検索結果はプロセス内でキャッシュする。
pub fn resolve_font(font_name_raw: &str, asset_dir: Option<&Path>) -> Option<Vec<u8>> {
    use std::sync::Mutex;
    // キャッシュキー: "フォント名\0asset_dir" の文字列
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Option<Vec<u8>>>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));

    let asset_dir_str = asset_dir.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let cache_key = format!("{}\x00{}", font_name_raw, asset_dir_str);

    // キャッシュヒット
    if let Ok(map) = cache.lock() {
        if let Some(cached) = map.get(&cache_key) {
            return cached.clone();
        }
    }

    let result = resolve_font_uncached(font_name_raw, asset_dir);

    if let Ok(mut map) = cache.lock() {
        map.insert(cache_key, result.clone());
    }

    result
}

fn resolve_font_uncached(font_name_raw: &str, asset_dir: Option<&Path>) -> Option<Vec<u8>> {
    let names: Vec<&str> = font_name_raw.split(',').map(|s| s.trim()).collect();

    for name in &names {
        // 1. asset_dir 内のフォントファイル（拡張子付き完全一致 → stem 前方一致）
        if let Some(dir) = asset_dir {
            if let Some(data) = find_font_in_dir(name, dir) {
                return Some(data);
            }
        }

        // 2. システムフォントフォルダ
        let font_dirs = [
            PathBuf::from(r"C:\Windows\Fonts"),
            dirs_font_local(),
        ];
        for dir in &font_dirs {
            if dir.exists() {
                if let Some(data) = find_font_in_dir(name, dir) {
                    return Some(data);
                }
            }
        }
    }

    // 3. 最終フォールバック
    let fallbacks = [
        r"C:\Windows\Fonts\msgothic.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\YuGothM.ttc",
    ];
    fallbacks.iter().find_map(|p| std::fs::read(p).ok())
}

fn dirs_font_local() -> PathBuf {
    // %LOCALAPPDATA%\Microsoft\Windows\Fonts
    std::env::var("LOCALAPPDATA")
        .map(|s| PathBuf::from(s).join("Microsoft").join("Windows").join("Fonts"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// フォント名（拡張子あり・なし両対応）でディレクトリ内を検索してファイルデータを返す
fn find_font_in_dir(name: &str, dir: &Path) -> Option<Vec<u8>> {
    let name_lower = name.to_lowercase();
    let norm = normalize_font_key(name);

    let Ok(entries) = std::fs::read_dir(dir) else { return None; };
    let mut candidates: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") { continue; }

        let file_name_lower = path.file_name()
            .and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        let stem = path.file_stem()
            .and_then(|s| s.to_str()).unwrap_or("");

        // 拡張子付き完全一致（"MyFont.ttf" のような指定）
        if file_name_lower == name_lower {
            if let Ok(data) = std::fs::read(&path) { return Some(data); }
        }

        // stem の前方一致（Regular/Medium 系を優先）
        let norm_stem = normalize_font_key(stem);
        if norm_stem.starts_with(&norm) || norm.starts_with(&norm_stem) {
            candidates.push(path);
        }
    }

    // Bold 系を後回しにして Regular/Medium 系を優先
    candidates.sort_by_key(|p| {
        let s = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
        let is_bold = s.contains("bold") || s.contains("heavy") || s.contains("black");
        (if is_bold { 1 } else { 0 }, p.file_name().map(|n| n.to_owned()))
    });

    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

/// フォント名を小文字・スペース除去・記号除去で正規化する
fn normalize_font_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_lowercase()
}

/// descript の色キーを読む。#RRGGBB 直値と .r/.g/.b 形式の両方に対応。
/// "none" または不在なら None を返す。
fn get_color(parsed: &HashMap<String, String>, key: &str) -> Option<Rgb> {
    // まず直値キー（#RRGGBB または "none"）を試みる
    if let Some(val) = parsed.get(key) {
        let v = val.trim();
        if v.to_lowercase() == "none" { return None; }
        if let Some(rgb) = parse_hex_color(v) { return Some(rgb); }
    }
    // 次に .r/.g/.b 形式
    get_color_from_descript(parsed, key)
}

fn parse_hex_color(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Rgb(r, g, b))
}

/// フォントバイト列から fontdue::Font を生成する（TTC は index=0）
fn parse_font(font_bytes: &[u8]) -> Option<fontdue::Font> {
    let settings = fontdue::FontSettings { collection_index: 0, scale: 40.0, load_substitutions: true };
    fontdue::Font::from_bytes(font_bytes, settings).ok()
}

/// フォント名からfontdue::Fontを解決してキャッシュする。
/// 同じフォント名に対してパースは初回のみ。
fn resolve_and_parse_font(font_name_raw: &str, asset_dir: Option<&Path>) -> Option<std::sync::Arc<fontdue::Font>> {
    use std::sync::{Arc, Mutex};
    static FONT_CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<String, Option<Arc<fontdue::Font>>>>> =
        std::sync::OnceLock::new();
    let cache = FONT_CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));

    let asset_dir_str = asset_dir.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let key = format!("{}\x00{}", font_name_raw, asset_dir_str);

    if let Ok(map) = cache.lock() {
        if let Some(cached) = map.get(&key) {
            return cached.clone();
        }
    }

    let result = resolve_font(font_name_raw, asset_dir)
        .as_deref()
        .and_then(parse_font)
        .map(Arc::new);

    if let Ok(mut map) = cache.lock() {
        map.insert(key, result.clone());
    }

    result
}

/// ビットマップフォントとして扱うべきか判定する（二値化の要否）
/// カンマ区切りの場合は先頭フォント名のみで判定する。
fn is_bitmap_font(name: &str) -> bool {
    // カンマ区切りの先頭フォント名のみを使う
    let first = name.split(',').next().unwrap_or(name).trim();
    normalize_font_name_for_match(first)
}

/// フォント名を正規化してビットマップフォントかを判定する
fn normalize_font_name_for_match(name: &str) -> bool {
    // スペース除去・全角英数字→半角・小文字化
    let normalized: String = name
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            let cp = c as u32;
            if (0xFF21..=0xFF3A).contains(&cp) {
                char::from_u32(cp - 0xFF21 + b'a' as u32).unwrap_or(c)
            } else if (0xFF41..=0xFF5A).contains(&cp) {
                char::from_u32(cp - 0xFF41 + b'a' as u32).unwrap_or(c)
            } else if (0xFF10..=0xFF19).contains(&cp) {
                char::from_u32(cp - 0xFF10 + b'0' as u32).unwrap_or(c)
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    // ＭＳ ゴシック / ＭＳ 明朝 / MS Gothic / MS Mincho 等
    normalized.contains("msgothic") || normalized.contains("msmincho")
    || normalized.contains("mspgothic") || normalized.contains("mspmincho")
    || normalized.contains("msゴシック") || normalized.contains("ms明朝")
    || normalized.contains("mspゴシック")
}

/// テキストを描画する（セッション優先、失敗時は fontdue フォールバック）
fn draw_text_on(
    img: &mut RgbaImage,
    font_name: &str,
    font: Option<&fontdue::Font>,
    text: &str,
    px: i32,
    py: i32,
    size_px: f32,
    color: Rgb,
    shadow: Option<(Rgb, bool)>,
    max_x: Option<i32>,
    no_aa: bool,
    sess: &mut Sess,
) {
    if sess.draw(img, text, px, py, color, shadow, max_x, size_px) {
        return;
    }
    // フォールバック: GDI 単発 → fontdue
    let gdi_font_name = font_name.split(',').next().unwrap_or(font_name).trim();
    if gdi_text::draw_text_gdi(img, gdi_font_name, text, px, py, size_px, color, shadow, max_x, no_aa) {
        return;
    }
    let Some(font) = font else { return; };
    draw_text_fontdue(img, font, text, px, py, size_px, color, shadow, max_x);
}

/// fontdue によるフォールバック描画
fn draw_text_fontdue(
    img: &mut RgbaImage,
    font: &fontdue::Font,
    text: &str,
    px: i32,
    py: i32,
    size_px: f32,
    color: Rgb,
    shadow: Option<(Rgb, bool)>,
    max_x: Option<i32>,
) {
    let metrics_ref = font.metrics('M', size_px);
    let ascent = metrics_ref.bounds.ymin + metrics_ref.bounds.height;

    let draw_str = |img: &mut RgbaImage, ox: i32, oy: i32, col: Rgb| {
        let mut cursor_x = 0.0f32;
        for ch in text.chars() {
            if let Some(mx) = max_x {
                if px + cursor_x as i32 >= mx { break; }
            }
            let (metrics, bitmap) = font.rasterize(ch, size_px);
            if metrics.width > 0 && metrics.height > 0 && !bitmap.is_empty() {
                let bx = ox + cursor_x as i32 + metrics.xmin;
                let by = oy + (ascent - metrics.bounds.ymin - metrics.bounds.height) as i32;
                let (iw, ih) = (img.width() as i32, img.height() as i32);
                for ry in 0..metrics.height {
                    for rx in 0..metrics.width {
                        let cov = bitmap[ry * metrics.width + rx];
                        if cov == 0 { continue; }
                        let dx = bx + rx as i32;
                        let dy = by + ry as i32;
                        if dx < 0 || dy < 0 || dx >= iw || dy >= ih { continue; }
                        blend_px(img, dx, dy, col, cov);
                    }
                }
            }
            cursor_x += metrics.advance_width;
        }
    };

    if let Some((sc, is_outline)) = shadow {
        if is_outline {
            for (ddx, ddy) in [(-1i32,-1i32),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                draw_str(img, px + ddx, py + ddy, sc);
            }
        } else {
            draw_str(img, px + 1, py + 1, sc);
        }
    }
    draw_str(img, px, py, color);
}

/// テキストの描画幅を計算する（セッション優先、フォールバック: fontdue）
fn measure_text(font_name: &str, font: Option<&fontdue::Font>, text: &str, size_px: f32, no_aa: bool, sess: &Sess) -> f32 {
    if let Some(w) = sess.measure_with_size(text, size_px) { return w; }
    let gdi_font_name = font_name.split(',').next().unwrap_or(font_name).trim();
    if let Some(w) = gdi_text::measure_text_gdi(gdi_font_name, text, size_px, no_aa) {
        return w;
    }
    if let Some(f) = font {
        return text.chars().map(|ch| f.metrics(ch, size_px).advance_width).sum();
    }
    size_px * 0.6 * text.chars().count() as f32
}

// ---------------------------------------------------------------------------
// ValidRect
// ---------------------------------------------------------------------------

fn build_valid_rect(parsed: &HashMap<String, String>, w: i32, h: i32) -> ValidRect {
    ValidRect {
        left:   pos_str(parsed.get("validrect.left").map(|s| s.as_str()).unwrap_or("10"), w),
        top:    pos_str(parsed.get("validrect.top").map(|s| s.as_str()).unwrap_or("10"), h),
        right:  pos_str(parsed.get("validrect.right").map(|s| s.as_str()).unwrap_or("-10"), w),
        bottom: pos_str(parsed.get("validrect.bottom").map(|s| s.as_str()).unwrap_or("-10"), h),
    }
}

// ---------------------------------------------------------------------------
// パーツ描画（k/s/p 系）
// ---------------------------------------------------------------------------

fn draw_parts(
    img: &mut RgbaImage,
    parsed: &HashMap<String, String>,
    cache: &HashMap<String, RgbaImage>,
    _w: i32, _h: i32,
    balloon_name: &str,
    font_name: &str,
    font: Option<&fontdue::Font>,
    _no_aa: bool,
    sess: &mut Sess,
) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);

    // バルーン種別に応じた arrow/clickwait ファイル名を解決する
    // s系: arrows{d}.png → arrow{d}.png
    // k系: arrowk{d}.png → arrow{d}.png
    // p系: arrowp{n}def{d}.png → arrow{d}.png
    let balloon_stem = balloon_name.trim_end_matches(".png");
    let p_num = if balloon_stem.starts_with("balloonp") {
        let after = balloon_stem.trim_start_matches("balloonp");
        after.find("def").map(|i| after[..i].to_string()).unwrap_or_default()
    } else {
        String::new()
    };

    // arrow0
    let arrow0_img: Option<&RgbaImage> = if balloon_stem.starts_with("balloonk") {
        cache.get("arrowk0.png").or_else(|| cache.get("arrow0.png"))
    } else if balloon_stem.starts_with("balloonp") && !p_num.is_empty() {
        let key = format!("arrowp{}def0.png", p_num);
        cache.get(key.as_str()).or_else(|| cache.get("arrow0.png"))
    } else {
        cache.get("arrows0.png").or_else(|| cache.get("arrow0.png"))
    };
    let arrow0_def_x = arrow0_img.map(|a| format!("-{}", 5 + a.width())).unwrap_or_else(|| "-5".to_string());
    if let Some(a) = arrow0_img {
        let x = pos_str(parsed.get("arrow0.x").map(|s| s.as_str()).unwrap_or(&arrow0_def_x), iw);
        let y = pos_str(parsed.get("arrow0.y").map(|s| s.as_str()).unwrap_or("5"), ih);
        alpha_paste(img, a, x, y);
    }

    // arrow1
    let arrow1_img: Option<&RgbaImage> = if balloon_stem.starts_with("balloonk") {
        cache.get("arrowk1.png").or_else(|| cache.get("arrow1.png"))
    } else if balloon_stem.starts_with("balloonp") && !p_num.is_empty() {
        let key = format!("arrowp{}def1.png", p_num);
        cache.get(key.as_str()).or_else(|| cache.get("arrow1.png"))
    } else {
        cache.get("arrows1.png").or_else(|| cache.get("arrow1.png"))
    };
    let arrow1_def_x = arrow1_img.map(|a| format!("-{}", 5 + a.width())).unwrap_or_else(|| "-5".to_string());
    let arrow1_def_y = arrow1_img.map(|a| format!("-{}", 5 + a.height())).unwrap_or_else(|| "-5".to_string());
    if let Some(a) = arrow1_img {
        let x = pos_str(parsed.get("arrow1.x").map(|s| s.as_str()).unwrap_or(&arrow1_def_x), iw);
        let y = pos_str(parsed.get("arrow1.y").map(|s| s.as_str()).unwrap_or(&arrow1_def_y), ih);
        alpha_paste(img, a, x, y);
    }

    if let Some(onl) = cache.get("online0.png") {
        let ow = onl.width() as i32;
        let oh = onl.height() as i32;
        // デフォルト: バルーン下部中央
        let ox = parsed.get("onlinemarker.x").map(|s| pos_str(s, iw))
            .unwrap_or((iw - ow) / 2);
        let oy = parsed.get("onlinemarker.y").map(|s| pos_str(s, ih))
            .unwrap_or(ih - oh);
        alpha_paste(img, onl, ox, oy);
    }

    // sstpmarker: sstp_new.png → sstp.png の順でフォールバック
    let sstp_marker_img = cache.get("sstp_new.png").or_else(|| cache.get("sstp.png"));
    if let Some(sm) = sstp_marker_img {
        let sx = pos_str(parsed.get("sstpmarker.x").map(|s| s.as_str()).unwrap_or("5"),  iw);
        // デフォルト Y: -(5 + 画像高さ)
        let sstp_def_y = format!("-{}", 5 + sm.height());
        let sy = pos_str(parsed.get("sstpmarker.y").map(|s| s.as_str()).unwrap_or(&sstp_def_y), ih);
        alpha_paste(img, sm, sx, sy);
    }

    // clickwait: 種別対応（s系: clickwaits.png、k系: clickwaitk.png、p系: clickwaitp{n}def.png）→ clickwait.png
    let clickwait_img = if balloon_stem.starts_with("balloonk") {
        cache.get("clickwaitk.png").or_else(|| cache.get("clickwait.png"))
    } else if balloon_stem.starts_with("balloonp") && !p_num.is_empty() {
        let specific = format!("clickwaitp{}def.png", p_num);
        cache.get(specific.as_str()).or_else(|| cache.get("clickwait.png"))
    } else {
        cache.get("clickwaits.png").or_else(|| cache.get("clickwait.png"))
    };
    if let Some(cw) = clickwait_img {
        let cw_def_x = format!("-{}", 10 + cw.width());
        let cw_def_y = format!("-{}", 10 + cw.height());
        let cx = parsed.get("clickwaitmarker.x").map(|s| pos_str(s, iw))
            .unwrap_or_else(|| pos_str(&cw_def_x, iw));
        let cy = parsed.get("clickwaitmarker.y").map(|s| pos_str(s, ih))
            .unwrap_or_else(|| pos_str(&cw_def_y, ih));
        alpha_paste(img, cw, cx, cy);
    }

    // SSTPメッセージ
    let ssx = pos_str(parsed.get("sstpmessage.x").map(|s| s.as_str()).unwrap_or("10"), iw);
    let ssy = pos_str(parsed.get("sstpmessage.y").map(|s| s.as_str()).unwrap_or("-5"), ih);
    let sstp_fh: f32 = parsed.get("sstpmessage.font.height")
        .and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let sstp_col = get_color(parsed, "sstpmessage.font.color")
        .unwrap_or(Rgb(0, 0, 192));
    let sstp_font_name = {
        let v = parsed.get("sstpmessage.font.name").map(|s| s.trim().to_string()).unwrap_or_default();
        if v.is_empty() { font_name.to_string() } else { v }
    };
    let sstp_no_aa = is_bitmap_font(&sstp_font_name);
    let sstp_leading = gdi_text::font_internal_leading(
        sstp_font_name.split(',').next().unwrap_or(&sstp_font_name).trim(), sstp_fh, sstp_no_aa);
    draw_text_on(img, &sstp_font_name, font, "SSTPメッセージ", ssx, ssy - sstp_leading / 2, sstp_fh, sstp_col, None, None, sstp_no_aa, sess);

    // カウンタ数値
    let num_xr: i32 = parsed.get("number.xr").and_then(|s| s.parse().ok()).unwrap_or(-20);
    let num_y   = pos_str(parsed.get("number.y").map(|s| s.as_str()).unwrap_or("-5"), ih);
    let num_fh: f32 = parsed.get("number.font.height")
        .and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let num_col = get_color(parsed, "number.font.color")
        .unwrap_or(Rgb(0, 0, 0));
    let num_font_name = {
        let v = parsed.get("number.font.name").map(|s| s.trim().to_string()).unwrap_or_default();
        if v.is_empty() { font_name.to_string() } else { v }
    };
    let num_no_aa = is_bitmap_font(&num_font_name);
    let tw = measure_text(&num_font_name, font, "999", num_fh, num_no_aa, sess);
    let num_x = iw + num_xr - tw as i32;
    let num_leading = gdi_text::font_internal_leading(
        num_font_name.split(',').next().unwrap_or(&num_font_name).trim(), num_fh, num_no_aa);
    draw_text_on(img, &num_font_name, font, "999", num_x, num_y - num_leading / 2, num_fh, num_col, None, None, num_no_aa, sess);
}

// ---------------------------------------------------------------------------
// communicatebox（balloonc 系）
// ---------------------------------------------------------------------------

fn draw_communicatebox(
    img: &mut RgbaImage,
    parsed: &HashMap<String, String>,
    cache: &HashMap<String, RgbaImage>,
    balloon_name: &str,
    font_name: &str,
    font: Option<&fontdue::Font>,
    sess: &mut Sess,
) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);

    let cb_x: i32 = parsed.get("communicatebox.x").and_then(|s| s.parse().ok()).unwrap_or(10);
    let cb_y: i32 = parsed.get("communicatebox.y").and_then(|s| s.parse().ok()).unwrap_or(10);
    let cb_w: i32 = parsed.get("communicatebox.width").and_then(|s| s.parse().ok()).unwrap_or(100);
    let cb_h: i32 = parsed.get("communicatebox.height").and_then(|s| s.parse().ok()).unwrap_or(30);

    let bl = cb_x.max(0).min(iw - 1);
    let bt = (if cb_y < 0 { ih + cb_y } else { cb_y }).max(0).min(ih - 1);
    let br = (bl + cb_w).min(iw);
    let bb = (bt + cb_h).min(ih);
    if br <= bl || bb <= bt { return; }

    let cb_bg = get_color(parsed, "communicatebox.background.color")
        .unwrap_or(Rgb(255, 255, 255));
    let cb_fc = get_color(parsed, "communicatebox.font.color")
        .unwrap_or(Rgb(0, 0, 0));

    // communicatebox.font.name があれば優先、なければ font.name にフォールバック
    let cb_font_name = {
        let v = parsed.get("communicatebox.font.name").map(|s| s.trim().to_string()).unwrap_or_default();
        if v.is_empty() { font_name.to_string() } else { v }
    };
    let cb_no_aa = is_bitmap_font(&cb_font_name);

    // フォントサイズ（入力域の高さに合わせる）
    let font_h = (cb_h as f32 * 0.75).max(8.0);

    // GDI フォントの internal leading（字形上端より上の余白）を取得して垂直中央補正に使う
    let gdi_font_name = cb_font_name.split(',').next().unwrap_or(&cb_font_name).trim();
    let leading = gdi_text::font_internal_leading(gdi_font_name, font_h, cb_no_aa);

    // 垂直中央の text_y を計算するクロージャ（leading 補正込み）
    let center_y = |area_top: i32, area_h: i32| -> i32 {
        area_top + ((area_h as f32 - font_h) / 2.0) as i32 - leading
    };

    let has_ok     = cache.contains_key("ok_up.png");
    let has_cancel = cache.contains_key("cancel_up.png");
    let has_mode   = cache.contains_key("mode_up.png");

    // balloonc4 はバルーン名で判定
    let stem = balloon_name.trim_end_matches(".png");
    let is_c4 = stem.ends_with('4') && stem.contains("balloonc");

    // 画像ボタンを垂直中央に貼るヘルパー
    let paste_centered_y = |img: &mut RgbaImage, key: &str, bx: i32| {
        let bimg = &cache[key];
        let bh = bimg.height() as i32;
        let by = bt + (cb_h - bh) / 2;
        alpha_paste(img, bimg, bx, by);
    };

    if is_c4 {
        // balloonc4: 左にmodeボタン、右にcancelボタン
        let cancel_w = if has_cancel {
            cache["cancel_up.png"].width() as i32
        } else {
            cb_h  // フォールバック: cb_h × cb_h の正方形
        };
        let mode_w = if has_mode {
            cache["mode_up.png"].width() as i32
        } else {
            cb_h * 2  // フォールバック: 縦1:横2の五角形
        };

        let input_left  = bl + mode_w;
        let input_right = br - cancel_w;

        // 入力域を背景色で塗る（ボタン領域は除く）
        fill_rect(img, input_left, bt, input_right, bb, cb_bg, 255);

        // 入力域枠線
        draw_input_border(img, input_left, bt, input_right, bb);

        // modeボタン（画像があれば垂直中央に貼り上にAutoテキストを重ねる、なければフォールバック描画）
        if has_mode {
            paste_centered_y(img, "mode_up.png", bl);
            let tw = measure_text(&cb_font_name, font, "Auto", font_h, cb_no_aa, sess);
            let tx = bl + ((mode_w as f32 - tw) / 2.0) as i32;
            let ty = center_y(bt, cb_h);
            draw_text_on(img, &cb_font_name, font, "Auto", tx, ty, font_h, cb_fc, None, None, cb_no_aa, sess);
        } else {
            draw_mode_pentagon(img, bl, bt, bl + mode_w, bb, cb_fc, &cb_font_name, font, font_h, leading, cb_no_aa, sess);
        }

        // cancelボタン
        if has_cancel {
            paste_centered_y(img, "cancel_up.png", br - cancel_w);
        } else {
            draw_button_box(img, br - cancel_w, bt, br, bb, "×", cb_fc, &cb_font_name, font, font_h, leading, cb_no_aa, sess);
        }

        // 入力テキスト（modeボタンの右から）
        let text_x = input_left + 4;
        let text_y = center_y(bt, cb_h);
        draw_text_on(img, &cb_font_name, font, "入力テキスト", text_x, text_y, font_h, cb_fc, None, None, cb_no_aa, sess);
    } else {
        // balloonc0-3: 右に OK + × ボタン
        let ok_w = if has_ok {
            cache["ok_up.png"].width() as i32
        } else {
            cb_h  // フォールバック: cb_h × cb_h の正方形
        };
        let cancel_w = if has_cancel {
            cache["cancel_up.png"].width() as i32
        } else {
            cb_h  // フォールバック: cb_h × cb_h の正方形
        };

        let input_right = br - ok_w - cancel_w;

        // 入力域を背景色で塗る（ボタン領域は除く）
        fill_rect(img, bl, bt, input_right, bb, cb_bg, 255);

        // 入力域枠線
        draw_input_border(img, bl, bt, input_right, bb);

        // OKボタン
        let ok_x = input_right;
        if has_ok {
            paste_centered_y(img, "ok_up.png", ok_x);
        } else {
            draw_button_box(img, ok_x, bt, ok_x + ok_w, bb, "OK", cb_fc, &cb_font_name, font, font_h, leading, cb_no_aa, sess);
        }

        // cancelボタン
        let cancel_x = br - cancel_w;
        if has_cancel {
            paste_centered_y(img, "cancel_up.png", cancel_x);
        } else {
            draw_button_box(img, cancel_x, bt, cancel_x + cancel_w, bb, "×", cb_fc, &cb_font_name, font, font_h, leading, cb_no_aa, sess);
        }

        // 入力テキスト
        let text_x = bl + 4;
        let text_y = center_y(bt, cb_h);
        draw_text_on(img, &cb_font_name, font, "入力テキスト", text_x, text_y, font_h, cb_fc, None, None, cb_no_aa, sess);
    }
}

/// 入力域の枠線（白上左右・青下）
fn draw_input_border(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32) {
    let white = Rgb(255, 255, 255);
    let blue  = Rgb(0, 0, 255);
    for x in x0..x1 {
        set_px(img, x, y0,     white, 255);
        set_px(img, x, y1 - 1, blue,  255);
    }
    for y in y0..y1 {
        set_px(img, x0,     y, white, 255);
        set_px(img, x1 - 1, y, white, 255);
    }
}

/// テキストと枠で構成されるフォールバックボタン（正方形）
fn draw_button_box(
    img: &mut RgbaImage,
    x0: i32, y0: i32, x1: i32, y1: i32,
    label: &str,
    color: Rgb,
    font_name: &str,
    font: Option<&fontdue::Font>,
    font_h: f32,
    leading: i32,
    no_aa: bool,
    sess: &mut Sess,
) {
    draw_rect_outline(img, x0, y0, x1 - 1, y1 - 1, color, 180);
    let tw = measure_text(font_name, font, label, font_h, no_aa, sess);
    let bw = (x1 - x0) as f32;
    let bh = (y1 - y0) as f32;
    let tx = x0 + ((bw - tw) / 2.0) as i32;
    let ty = y0 + ((bh - font_h) / 2.0) as i32 - leading;
    draw_text_on(img, font_name, font, label, tx, ty, font_h, color, None, None, no_aa, sess);
}

/// balloonc4 の mode ボタン（左四角・右山型の五角形）
fn draw_mode_pentagon(
    img: &mut RgbaImage,
    x0: i32, y0: i32, x1: i32, y1: i32,
    color: Rgb,
    font_name: &str,
    font: Option<&fontdue::Font>,
    font_h: f32,
    leading: i32,
    no_aa: bool,
    sess: &mut Sess,
) {
    let h = y1 - y0;
    let tip_jut = (h / 10).max(4);
    let tip_x = x1 - 1;
    let tip_y = (y0 + y1) / 2;

    let poly = [
        (x0,               y0),
        (tip_x - tip_jut,  y0),
        (tip_x,            tip_y),
        (tip_x - tip_jut,  y1 - 1),
        (x0,               y1 - 1),
    ];
    draw_polygon_outline(img, &poly, color, 200);

    let tw = measure_text(font_name, font, "Auto", font_h, no_aa, sess);
    let bw = (x1 - x0 - tip_jut) as f32;
    let bh = (y1 - y0) as f32;
    let tx = x0 + ((bw - tw) / 2.0) as i32;
    let ty = y0 + ((bh - font_h) / 2.0) as i32 - leading;
    draw_text_on(img, font_name, font, "Auto", tx, ty, font_h, color, None, None, no_aa, sess);
}

fn draw_polygon_outline(img: &mut RgbaImage, pts: &[(i32, i32)], color: Rgb, alpha: u8) {
    let n = pts.len();
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        draw_line(img, x0, y0, x1, y1, color, alpha);
    }
}

fn draw_line(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb, alpha: u8) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        set_px(img, x, y, color, alpha);
        if x == x1 && y == y1 { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 <  dx { err += dx; y += sy; }
    }
}

// ---------------------------------------------------------------------------
// サンプルテキスト描画（k/s/p 系）
// ---------------------------------------------------------------------------

fn draw_sample_text(
    img: &mut RgbaImage,
    parsed: &HashMap<String, String>,
    vr: &ValidRect,
    cache: &HashMap<String, RgbaImage>,
    mode: PreviewMode,
    balloon_name: &str,
    font_name: &str,
    font: Option<&fontdue::Font>,
    no_aa: bool,
    sess: &mut Sess,
) {
    let iw = img.width() as i32;
    let font_height: f32 = parsed.get("font.height")
        .and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let line_h = (font_height + 2.0) as i32;

    // 影設定
    let shadow_color = get_color(parsed, "font.shadowcolor");
    let shadow_style = parsed.get("font.shadowstyle").map(|s| s.as_str()).unwrap_or("offset");
    let shadow = shadow_color.map(|sc| (sc, shadow_style.contains("outline")));

    // blendmethod の状態を3値で判定:
    //   RasterOp  = none以外の値が存在 → ラスタオペレーションあり（pen/brush使用）
    //   Simple    = none または未定義  → シンプルモード（font.color使用、矩形・下線は描画）
    //   NoDeco    = Simple かつ font.color.r も未定義 → 装飾なし扱い（通常文字色にフォールバック）
    let blend_state = |blend_key: &str, font_color_key: &str| -> u8 {
        // 0=NoDeco, 1=Simple, 2=RasterOp
        match parsed.get(blend_key).map(|s| s.trim().to_lowercase()).as_deref() {
            Some(v) if v != "none" => 2, // RasterOp
            _ => {
                // none または未定義 → font.color.r の存在で判定
                if parsed.contains_key(font_color_key) { 1 } else { 0 }
            }
        }
    };
    let cursor_ns_state  = blend_state("cursor.notselect.blendmethod", "cursor.notselect.font.color.r");
    let cursor_state     = blend_state("cursor.blendmethod",           "cursor.font.color.r");
    let anchor_ns_state  = blend_state("anchor.notselect.blendmethod", "anchor.notselect.font.color.r");
    let anchor_state     = blend_state("anchor.blendmethod",           "anchor.font.color.r");
    let anchor_vis_state = blend_state("anchor.visited.blendmethod",   "anchor.visited.font.color.r");

    // 各色
    let font_color    = get_color(parsed, "font.color").unwrap_or(Rgb(0,0,0));
    // disable.font.color が none（または未指定）のとき SSP はバルーン画像色と通常文字色を
    // ミックスした色で描画する。プレビューではバルーン画像中央付近の色を代表値として使う。
    let disable_color = get_color(parsed, "disable.font.color").unwrap_or_else(|| {
        let cx = (img.width() / 2) as i64;
        let cy = (img.height() / 2) as i64;
        // 中央 3x3 の不透明ピクセルを平均してバルーン画像色を推定する
        let mut sum = (0i64, 0i64, 0i64);
        let mut cnt = 0i64;
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                let x = (cx + dx).clamp(0, img.width() as i64 - 1) as u32;
                let y = (cy + dy).clamp(0, img.height() as i64 - 1) as u32;
                let px = img.get_pixel(x, y);
                if px[3] > 0 {
                    sum.0 += px[0] as i64;
                    sum.1 += px[1] as i64;
                    sum.2 += px[2] as i64;
                    cnt += 1;
                }
            }
        }
        let bg = if cnt > 0 {
            Rgb((sum.0 / cnt) as u8, (sum.1 / cnt) as u8, (sum.2 / cnt) as u8)
        } else {
            Rgb(255, 255, 255) // 全透明なら白とみなす
        };
        // SSP: (font_color + bg * 2) / 3 に相当（文字色を薄く見せる）
        Rgb(
            ((font_color.0 as i32 + bg.0 as i32 * 2) / 3) as u8,
            ((font_color.1 as i32 + bg.1 as i32 * 2) / 3) as u8,
            ((font_color.2 as i32 + bg.2 as i32 * 2) / 3) as u8,
        )
    });

    // 文字色解決: NoDeco(0)は通常文字色、Simple(1)/RasterOp(2)はfont.color.*を使用
    let resolve_color = |key: &str, fallback: Rgb| -> Rgb {
        get_color(parsed, key).unwrap_or(fallback)
    };
    let choice_color     = if cursor_ns_state  == 0 { font_color } else { resolve_color("cursor.notselect.font.color", Rgb(0,0,255)) };
    let choice_sel_color = if cursor_state     == 0 { font_color } else { resolve_color("cursor.font.color",           Rgb(255,255,255)) };
    let anchor_color     = if anchor_ns_state  == 0 { font_color } else { resolve_color("anchor.notselect.font.color", Rgb(0,0,255)) };
    let anchor_sel_color = if anchor_state     == 0 { font_color } else { resolve_color("anchor.font.color",           Rgb(0,0,255)) };
    let anchor_vis_color = if anchor_vis_state == 0 { font_color } else { resolve_color("anchor.visited.font.color",   Rgb(170,0,170)) };

    // pen/brush色: RasterOp(2)のときのみ使用。Simple(1)のときはfont.colorを使用（仮表示）
    let cursor_brush     = if cursor_state     == 1 { choice_sel_color } else { get_color(parsed, "cursor.brush.color").unwrap_or(Rgb(0,0,255)) };
    let cursor_pen       = if cursor_state     == 1 { choice_sel_color } else { get_color(parsed, "cursor.pen.color").unwrap_or(Rgb(0,0,0)) };
    let anchor_brush     = if anchor_state     == 1 { anchor_sel_color } else { get_color(parsed, "anchor.brush.color").unwrap_or(Rgb(0,0,0)) };
    let anchor_pen       = if anchor_state     == 1 { anchor_sel_color } else { get_color(parsed, "anchor.pen.color").unwrap_or(Rgb(127,127,0)) };
    let anchor_vis_brush = if anchor_vis_state == 1 { anchor_vis_color } else { get_color(parsed, "anchor.visited.brush.color").unwrap_or(Rgb(0,0,0)) };
    let anchor_vis_pen   = if anchor_vis_state == 1 { anchor_vis_color } else { get_color(parsed, "anchor.visited.pen.color").unwrap_or(Rgb(0,0,0)) };
    let cursor_ns_brush  = if cursor_ns_state  == 1 { choice_color     } else { get_color(parsed, "cursor.notselect.brush.color").unwrap_or(Rgb(0,0,0)) };
    let cursor_ns_pen    = if cursor_ns_state  == 1 { choice_color     } else { get_color(parsed, "cursor.notselect.pen.color").unwrap_or(Rgb(0,0,0)) };
    let anchor_ns_brush  = if anchor_ns_state  == 1 { anchor_color     } else { get_color(parsed, "anchor.notselect.brush.color").unwrap_or(Rgb(0,0,0)) };
    let anchor_ns_pen    = if anchor_ns_state  == 1 { anchor_color     } else { get_color(parsed, "anchor.notselect.pen.color").unwrap_or(Rgb(0,0,0)) };

    // style キーのデフォルト値（field_def と一致させる）
    let style_defaults: std::collections::HashMap<&str, &str> = [
        ("cursor.style",              "square"),
        ("anchor.style",              "underline"),
        ("anchor.visited.style",      "none"),
        ("cursor.notselect.style",    "none"),
        ("anchor.notselect.style",    "none"),
    ].iter().cloned().collect();

    let parse_style = |key: &str| -> (bool, bool) {
        let default = style_defaults.get(key).copied().unwrap_or("none");
        let v = parsed.get(key).map(|s| s.as_str()).unwrap_or(default).to_lowercase();
        (v.contains("square"), v.contains("underline"))
    };
    // NoDeco(0)のみ矩形・下線なし。Simple(1)/RasterOp(2)はstyleに従い描画
    let (cursor_ns_rect,  cursor_ns_ul)  = if cursor_ns_state  == 0 { (false, false) } else { parse_style("cursor.notselect.style") };
    let (cursor_rect,     cursor_ul)     = if cursor_state     == 0 { (false, false) } else { parse_style("cursor.style") };
    let (anchor_ns_rect,  anchor_ns_ul)  = if anchor_ns_state  == 0 { (false, false) } else { parse_style("anchor.notselect.style") };
    let (anchor_rect,     anchor_ul)     = if anchor_state     == 0 { (false, false) } else { parse_style("anchor.style") };
    let (anchor_vis_rect, anchor_vis_ul) = if anchor_vis_state == 0 { (false, false) } else { parse_style("anchor.visited.style") };

    // シンプルモード(state==1)時の影設定: {prefix}.font.shadowcolor/shadowstyle を参照
    // 未定義または none のときは通常文字の影設定にフォールバック
    let resolve_shadow = |sc_key: &str, ss_key: &str| -> Option<(Rgb, bool)> {
        let sc = get_color(parsed, sc_key)?; // none または未定義なら None
        let style = parsed.get(ss_key).map(|s| s.as_str()).unwrap_or("offset");
        Some((sc, style.contains("outline")))
    };
    let cursor_shadow     = if cursor_state     == 1 { resolve_shadow("cursor.font.shadowcolor",           "cursor.font.shadowstyle")           .or(shadow) } else { shadow };
    let cursor_ns_shadow  = if cursor_ns_state  == 1 { resolve_shadow("cursor.notselect.font.shadowcolor", "cursor.notselect.font.shadowstyle")  .or(shadow) } else { shadow };
    let anchor_shadow     = if anchor_state     == 1 { resolve_shadow("anchor.font.shadowcolor",           "anchor.font.shadowstyle")           .or(shadow) } else { shadow };
    let anchor_ns_shadow  = if anchor_ns_state  == 1 { resolve_shadow("anchor.notselect.font.shadowcolor", "anchor.notselect.font.shadowstyle")  .or(shadow) } else { shadow };
    let anchor_vis_shadow = if anchor_vis_state == 1 { resolve_shadow("anchor.visited.font.shadowcolor",   "anchor.visited.font.shadowstyle")   .or(shadow) } else { shadow };

    // GDI フォントの internal leading（字形上端より上の余白）
    // square/underline の描画位置をグリフの実描画範囲に合わせるために使う
    let leading = sess.internal_leading();

    // 折り返しX: validrect.right と wordwrappoint.x のうち左側を採用（モード共通）
    let ww_wrap = parsed.get("wordwrappoint.x")
        .map(|s| pos_str(s.as_str(), iw))
        .unwrap_or(vr.right);
    let wrap_x = vr.right.min(ww_wrap);

    // モードB用：十分長い繰り返し文字列（draw_text_on の max_x で打ち切る）
    let mode_b_half = "1234567890".repeat(100);
    let mode_b_full = "１２３４５６７８９０".repeat(100);

    let sample_lines: &[(&str, u8)] = &[
        // role: 0=normal 1=disable 2=choice 3=choice_sel 4=anchor 5=anchor_sel 6=anchor_vis
        ("普通の文字",         0),
        ("←マーカー",         0),
        ("無効な表示",         1),
        ("選択肢(非選択)",     2),
        ("選択肢(選択中)",     3),
        ("アンカー(非選択)",   4),
        ("アンカー(選択中)",   5),
        ("アンカー(訪問済み)", 6),
    ];

    // origin.x/y: テキスト開始位置のオフセット（validrect 左上に加算）
    let origin_x: i32 = parsed.get("origin.x").and_then(|s| s.parse().ok()).unwrap_or(0);
    let origin_y: i32 = parsed.get("origin.y").and_then(|s| s.parse().ok()).unwrap_or(0);
    let text_x = vr.left + origin_x;
    let mut text_y = vr.top + origin_y;

    // サンプルテキスト行頭マーカー（UKADOC仕様）
    // s系: markers.png → marker.png → sstp.png
    // k系: markerk.png → marker.png → sstp.png
    // p系: markerp{n}def.png → marker.png → sstp.png
    // sstp.png は marker*.png が存在しない旧バルーン向けの代用
    let balloon_stem = balloon_name.trim_end_matches(".png");
    let marker_img = if balloon_stem.starts_with("balloonk") {
        cache.get("markerk.png")
            .or_else(|| cache.get("marker.png"))
            .or_else(|| cache.get("sstp.png"))
    } else if balloon_stem.starts_with("balloonp") {
        // 例: balloonp2def0 → markerp2def.png
        let p_marker_key = {
            let after = balloon_stem.trim_start_matches("balloonp");
            if let Some(def_pos) = after.find("def") {
                format!("markerp{}def.png", &after[..def_pos])
            } else {
                String::new()
            }
        };
        let specific = if !p_marker_key.is_empty() { cache.get(p_marker_key.as_str()) } else { None };
        specific
            .or_else(|| cache.get("marker.png"))
            .or_else(|| cache.get("sstp.png"))
    } else {
        // s系（デフォルト）
        cache.get("markers.png")
            .or_else(|| cache.get("marker.png"))
            .or_else(|| cache.get("sstp.png"))
    };

    let mut line_idx = 0usize;
    loop {
        if text_y + font_height as i32 > vr.bottom { break; }

        let (label_owned, role) = if mode == PreviewMode::A {
            if line_idx >= sample_lines.len() { break; }
            let (l, r) = sample_lines[line_idx];
            (l.to_string(), r)
        } else {
            if line_idx >= 2 { break; }
            let s = if line_idx == 0 { mode_b_half.clone() } else { mode_b_full.clone() };
            (s, 0u8)
        };
        let label = label_owned.as_str();

        let txt_color = match role {
            0 => font_color,
            1 => disable_color,
            2 => choice_color,
            3 => choice_sel_color,
            4 => anchor_color,
            5 => anchor_sel_color,
            6 => anchor_vis_color,
            _ => font_color,
        };

        let (use_rect, use_ul, pen, brush) = match role {
            2 => (cursor_ns_rect, cursor_ns_ul, cursor_ns_pen, cursor_ns_brush),
            3 => (cursor_rect,    cursor_ul,    cursor_pen,    cursor_brush),
            4 => (anchor_ns_rect, anchor_ns_ul, anchor_ns_pen, anchor_ns_brush),
            5 => (anchor_rect,    anchor_ul,    anchor_pen,    anchor_brush),
            6 => (anchor_vis_rect, anchor_vis_ul, anchor_vis_pen, anchor_vis_brush),
            _ => (false, false, Rgb(0,0,0), Rgb(0,0,0)),
        };

        // marker を先頭に貼る（2行目のみ）：フォント行の縦中央に合わせる
        let mut tx = text_x;
        if mode == PreviewMode::A && line_idx == 1 {
            if let Some(mk) = marker_img {
                let mk_h = mk.height() as i32;
                let marker_y = text_y + (font_height as i32 - mk_h) / 2;
                alpha_paste(img, mk, tx, marker_y);
                tx += mk.width() as i32 + 2;
            }
        }

        // テキスト幅を計測して装飾範囲を決める
        let text_w = measure_text(font_name, font, label, font_height, no_aa, sess) as i32;

        // テキスト右端は wrap_x を超えない（square/underline が折り返し位置を超えないように）
        let decor_right = (tx + text_w).min(wrap_x);

        // square/underline は font_height の範囲に合わせる
        let decor_top = text_y;
        let decor_bot = text_y + font_height as i32;
        if use_rect {
            fill_rect(img, tx, decor_top, decor_right, decor_bot, brush, 255);
            draw_rect_outline(img, tx, decor_top, decor_right, decor_bot - 1, pen, 255);
        }
        if use_ul {
            draw_hline(img, tx, decor_bot - 1, decor_right, pen, 255);
        }

        // テキスト描画: leading を上下均等に振り分けて font_height 内で垂直中央に配置
        let text_draw_y = text_y - leading / 2;
        let clip = Some(wrap_x);
        // シンプルモード時はグループごとの影設定を使用
        let line_shadow = match role {
            2 => cursor_ns_shadow,
            3 => cursor_shadow,
            4 => anchor_ns_shadow,
            5 => anchor_shadow,
            6 => anchor_vis_shadow,
            _ => shadow,
        };
        draw_text_on(img, font_name, font, label, tx, text_draw_y, font_height, txt_color, line_shadow, clip, no_aa, sess);

        text_y += line_h;
        line_idx += 1;

        // モードB: 2行後に残りを行番号テキストで埋める
        if mode == PreviewMode::B && line_idx >= 2 {
            let mut n = 3usize;
            while text_y + font_height as i32 <= vr.bottom {
                let num_label = n.to_string();
                draw_text_on(img, font_name, font, &num_label, text_x, text_y, font_height, font_color, None, None, no_aa, sess);
                text_y += line_h;
                n += 1;
            }
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// オーバーレイ
// ---------------------------------------------------------------------------

fn draw_overlay(
    img: &mut RgbaImage,
    parsed: &HashMap<String, String>,
    vr: &ValidRect,
    is_balloonc: bool,
    _w: i32, _h: i32,
) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let white = Rgb(255, 255, 255);
    let red   = Rgb(255, 0, 0);

    if is_balloonc {
        let cb_x: i32 = parsed.get("communicatebox.x").and_then(|s| s.parse().ok()).unwrap_or(10);
        let cb_y: i32 = parsed.get("communicatebox.y").and_then(|s| s.parse().ok()).unwrap_or(10);
        let cb_w: i32 = parsed.get("communicatebox.width").and_then(|s| s.parse().ok()).unwrap_or(100);
        let cb_h: i32 = parsed.get("communicatebox.height").and_then(|s| s.parse().ok()).unwrap_or(30);
        let bl = cb_x.max(0).min(iw - 1);
        let bt = (if cb_y < 0 { ih + cb_y } else { cb_y }).max(0).min(ih - 1);
        let br = (bl + cb_w).min(iw) - 1;
        let bb = (bt + cb_h).min(ih) - 1;
        draw_overlay_rect(img, bl, bt, br, bb, white, red);
        for (cx, cy) in [(bl, bt), (br, bt), (bl, bb), (br, bb)] {
            draw_cross(img, cx, cy, white, red);
        }
    } else {
        let (l, t, r, b) = (vr.left, vr.top, vr.right - 1, vr.bottom - 1);
        draw_overlay_rect(img, l, t, r, b, white, red);
        for (cx, cy) in [(l, t), (r, t), (l, b), (r, b)] {
            draw_cross(img, cx, cy, white, red);
        }
        let wwx_raw = parsed.get("wordwrappoint.x").map(|s| s.as_str()).unwrap_or("-0");
        let wwx = pos_str(wwx_raw, iw);
        if wwx > vr.right {
            draw_vline_dashed(img, wwx, t, b, Rgb(180,180,180), Rgb(100,100,100));
        } else {
            draw_overlay_vline(img, wwx, t, b, white, red);
        }

        // origin.x/y が 0 以外のとき、テキスト実開始点を緑の十字で表示
        let origin_x: i32 = parsed.get("origin.x").and_then(|s| s.parse().ok()).unwrap_or(0);
        let origin_y: i32 = parsed.get("origin.y").and_then(|s| s.parse().ok()).unwrap_or(0);
        if origin_x != 0 || origin_y != 0 {
            let tx = vr.left + origin_x;
            let ty = vr.top  + origin_y;
            draw_cross(img, tx, ty, Rgb(0, 200, 0), Rgb(0, 200, 0));
        }
    }
}

// ---------------------------------------------------------------------------
// 低レベル描画
// ---------------------------------------------------------------------------

fn blend_px(img: &mut RgbaImage, x: i32, y: i32, color: Rgb, alpha: u8) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 { return; }
    let px = img.get_pixel_mut(x as u32, y as u32);
    let sa = alpha as u32;
    let da = px[3] as u32;
    let out_a = (sa + da * (255 - sa) / 255).min(255);
    for (i, &c) in [color.0, color.1, color.2].iter().enumerate() {
        px[i] = ((c as u32 * sa + px[i] as u32 * da * (255 - sa) / 255) / out_a.max(1)) as u8;
    }
    px[3] = out_a as u8;
}

fn alpha_paste(dst: &mut RgbaImage, src: &RgbaImage, px: i32, py: i32) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    for sy in 0..sh {
        for sx in 0..sw {
            let dx = px + sx;
            let dy = py + sy;
            if dx < 0 || dy < 0 || dx >= dw || dy >= dh { continue; }
            let sp = src.get_pixel(sx as u32, sy as u32);
            let sa = sp[3] as u32;
            if sa == 0 { continue; }
            let dp = dst.get_pixel_mut(dx as u32, dy as u32);
            let da = dp[3] as u32;
            let out_a = (sa + da * (255 - sa) / 255).min(255);
            for i in 0..3 {
                dp[i] = ((sp[i] as u32 * sa + dp[i] as u32 * da * (255 - sa) / 255) / out_a.max(1)) as u8;
            }
            dp[3] = out_a as u8;
        }
    }
}

fn fill_rect(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb, alpha: u8) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in y0.max(0)..y1.min(h) {
        for x in x0.max(0)..x1.min(w) {
            blend_px(img, x, y, color, alpha);
        }
    }
}

fn set_px(img: &mut RgbaImage, x: i32, y: i32, color: Rgb, alpha: u8) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 { return; }
    *img.get_pixel_mut(x as u32, y as u32) = Rgba([color.0, color.1, color.2, alpha]);
}

fn draw_hline(img: &mut RgbaImage, x0: i32, y: i32, x1: i32, color: Rgb, alpha: u8) {
    if y < 0 || y >= img.height() as i32 { return; }
    for x in x0.max(0)..x1.min(img.width() as i32) { set_px(img, x, y, color, alpha); }
}

fn draw_vline(img: &mut RgbaImage, x: i32, y0: i32, y1: i32, color: Rgb, alpha: u8) {
    if x < 0 || x >= img.width() as i32 { return; }
    for y in y0.max(0)..=y1.min(img.height() as i32 - 1) { set_px(img, x, y, color, alpha); }
}

fn draw_rect_outline(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb, alpha: u8) {
    draw_hline(img, x0, y0, x1 + 1, color, alpha);
    draw_hline(img, x0, y1, x1 + 1, color, alpha);
    draw_vline(img, x0, y0, y1, color, alpha);
    draw_vline(img, x1, y0, y1, color, alpha);
}

fn draw_overlay_rect(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, outer: Rgb, inner: Rgb) {
    for dx in -1..=1i32 { for dy in -1..=1i32 {
        if dx == 0 && dy == 0 { continue; }
        draw_rect_outline(img, x0+dx, y0+dy, x1+dx, y1+dy, outer, 255);
    }}
    draw_dashed_hline(img, x0, y0, x1, inner);
    draw_dashed_hline(img, x0, y1, x1, inner);
    draw_dashed_vline(img, x0, y0, y1, inner);
    draw_dashed_vline(img, x1, y0, y1, inner);
}

fn draw_overlay_vline(img: &mut RgbaImage, x: i32, y0: i32, y1: i32, outer: Rgb, inner: Rgb) {
    for dx in -1..=1i32 { draw_vline(img, x+dx, y0, y1, outer, 255); }
    draw_dashed_vline(img, x, y0, y1, inner);
}

fn draw_vline_dashed(img: &mut RgbaImage, x: i32, y0: i32, y1: i32, outer: Rgb, inner: Rgb) {
    for dx in -1..=1i32 { draw_vline(img, x+dx, y0, y1, outer, 200); }
    draw_dashed_vline(img, x, y0, y1, inner);
}

fn draw_dashed_hline(img: &mut RgbaImage, x0: i32, y: i32, x1: i32, color: Rgb) {
    let (on, off) = (6i32, 4i32);
    let mut x = x0;
    while x <= x1 { draw_hline(img, x, y, (x+on).min(x1+1), color, 255); x += on + off; }
}

fn draw_dashed_vline(img: &mut RgbaImage, x: i32, y0: i32, y1: i32, color: Rgb) {
    let (on, off) = (6i32, 4i32);
    let mut y = y0;
    while y <= y1 { draw_vline(img, x, y, (y+on-1).min(y1), color, 255); y += on + off; }
}

fn draw_cross(img: &mut RgbaImage, cx: i32, cy: i32, outer: Rgb, inner: Rgb) {
    let s = 6i32;
    for d in -1..=1i32 {
        draw_hline(img, cx-s, cy+d, cx+s+1, outer, 255);
        draw_vline(img, cx+d, cy-s, cy+s, outer, 255);
    }
    draw_hline(img, cx-s, cy, cx+s+1, inner, 255);
    draw_vline(img, cx, cy-s, cy+s, inner, 255);
}
