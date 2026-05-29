/// バルーン画像の合成・色オーバーレイ・左右反転を担う
use std::collections::HashMap;
use std::path::Path;
use image::{ImageBuffer, Rgba, RgbaImage};
use crate::core::color::{ColorSet, Rgb};
use crate::core::layout::{ColorType, LayerList, load_balloon_layout, make_odd_flip_sources};

/// PNG ファイルを RGBA 画像として開く。SSP互換の透過処理を適用する。
///
/// 優先順位:
/// 1. 同名 `.pna` が存在する場合: `.png` を RGB、`.pna` をアルファチャンネルとして合成
/// 2. 32bit PNG (アルファチャンネルあり): そのまま使用
/// 3. 24bit PNG + tRNS チャンク: tRNS の透過色を alpha=0 として適用
/// 4. 24bit PNG (tRNSなし): 左上1ピクセルの色をカラーキーとして透過
pub fn open_png_rgba(path: &Path) -> anyhow::Result<RgbaImage> {
    // 1. .pna が隣にあるか確認
    let pna_path = path.with_extension("pna");
    if pna_path.exists() {
        return open_png_with_pna(path, &pna_path);
    }

    // 2/3/4. .pna なし: color_type で分岐
    let dyn_img = image::open(path)
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;

    match dyn_img.color() {
        // 32bit RGBA: アルファそのまま
        image::ColorType::Rgba8 | image::ColorType::Rgba16 | image::ColorType::Rgba32F => {
            Ok(dyn_img.to_rgba8())
        }
        // 24bit RGB: tRNS or 左上1px 透過
        _ => {
            let rgb = dyn_img.to_rgb8();
            apply_rgb_transparency(path, rgb)
        }
    }
}

/// .pna ファイルをアルファチャンネルとして合成する
/// .pna は拡張子が非標準のため image::open が使えない。ファイルバイト列を PNG としてデコードする。
fn open_png_with_pna(png_path: &Path, pna_path: &Path) -> anyhow::Result<RgbaImage> {
    let rgb = image::open(png_path)
        .map_err(|e| anyhow::anyhow!("{}: {}", png_path.display(), e))?
        .to_rgb8();

    // .pna をバイト列として読み込み PNG デコード
    let pna_bytes = std::fs::read(pna_path)
        .map_err(|e| anyhow::anyhow!("{}: {}", pna_path.display(), e))?;
    let alpha_dyn = image::load_from_memory_with_format(&pna_bytes, image::ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("{}: {}", pna_path.display(), e))?;
    let alpha = alpha_dyn.to_luma8();

    let (w, h) = rgb.dimensions();
    if alpha.dimensions() != (w, h) {
        anyhow::bail!(
            ".pna のサイズ({:?})が .png({:?})と一致しません: {}",
            alpha.dimensions(), (w, h), pna_path.display()
        );
    }

    let mut rgba = RgbaImage::new(w, h);
    for (x, y, rgb_px) in rgb.enumerate_pixels() {
        let a = alpha.get_pixel(x, y)[0];
        rgba.put_pixel(x, y, Rgba([rgb_px[0], rgb_px[1], rgb_px[2], a]));
    }
    Ok(rgba)
}

/// 24bit RGB 画像に透過を適用する。
/// tRNS チャンクがあればその色を透明に、なければ左上1pxの色をカラーキーとして透明にする。
fn apply_rgb_transparency(path: &Path, rgb: image::RgbImage) -> anyhow::Result<RgbaImage> {
    let key = read_trns_key(path).or_else(|| {
        // tRNS なし → 左上1pxをカラーキーに
        let px = rgb.get_pixel(0, 0);
        Some((px[0], px[1], px[2]))
    });

    let (w, h) = rgb.dimensions();
    let mut rgba = RgbaImage::new(w, h);
    for (x, y, px) in rgb.enumerate_pixels() {
        let a = match key {
            Some((kr, kg, kb)) if px[0] == kr && px[1] == kg && px[2] == kb => 0,
            _ => 255,
        };
        rgba.put_pixel(x, y, Rgba([px[0], px[1], px[2], a]));
    }
    Ok(rgba)
}

/// PNG ファイルの tRNS チャンクから RGB 透過色を読み取る。
/// tRNS がない・読み取り失敗の場合は None を返す。
fn read_trns_key(path: &Path) -> Option<(u8, u8, u8)> {
    use png::Decoder;
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(path).ok()?;
    let decoder = Decoder::new(BufReader::new(file));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();

    // tRNS（RGB トリプレット、各16bit big-endian）
    // 8bit PNG の場合: 上位バイト=0x00, 下位バイト=実際の値 → trns[1], trns[3], trns[5]
    // ただし SSP 互換素材は 8bit 値を上位バイトに置く場合もあるため、
    // 16bit 値として取り出して >> 8 するか、下位バイトを取る両方を試みる
    if let Some(trns) = &info.trns {
        if trns.len() >= 6 {
            // PNG 仕様: big-endian 16bit。8bit 画像なら下位バイトが実値
            let r = (u16::from_be_bytes([trns[0], trns[1]]) & 0xFF) as u8;
            let g = (u16::from_be_bytes([trns[2], trns[3]]) & 0xFF) as u8;
            let b = (u16::from_be_bytes([trns[4], trns[5]]) & 0xFF) as u8;
            return Some((r, g, b));
        }
    }
    None
}

/// RGBA画像のアルファ形状を保ったまま RGB を指定色で塗り替える
///
/// no_color=true のときは元画像の RGBA をそのまま返す
pub fn apply_color_overlay(src: &RgbaImage, color: Rgb, no_color: bool) -> RgbaImage {
    if no_color {
        return src.clone();
    }
    let (w, h) = src.dimensions();
    let mut dst = RgbaImage::new(w, h);
    for (x, y, px) in src.enumerate_pixels() {
        let a = px[3];
        dst.put_pixel(x, y, Rgba([color.r(), color.g(), color.b(), a]));
    }
    dst
}

/// レイヤーリストを先頭=奥・末尾=手前の順でアルファ合成する
pub fn composite_layers(layers: &[RgbaImage]) -> anyhow::Result<RgbaImage> {
    if layers.is_empty() {
        anyhow::bail!("レイヤーが1枚もありません");
    }
    let (w, h) = layers[0].dimensions();
    let mut result: RgbaImage = ImageBuffer::new(w, h);
    for layer in layers {
        for (x, y, src) in layer.enumerate_pixels() {
            let dst = result.get_pixel_mut(x, y);
            // アルファ合成: src over dst
            let sa = src[3] as f32 / 255.0;
            let da = dst[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            if out_a < f32::EPSILON {
                *dst = Rgba([0, 0, 0, 0]);
                continue;
            }
            let blend = |s: u8, d: u8| -> u8 {
                ((s as f32 * sa + d as f32 * da * (1.0 - sa)) / out_a).round() as u8
            };
            *dst = Rgba([
                blend(src[0], dst[0]),
                blend(src[1], dst[1]),
                blend(src[2], dst[2]),
                (out_a * 255.0).round() as u8,
            ]);
        }
    }
    Ok(result)
}

/// 画像を左右反転する
pub fn flip_horizontal(img: &RgbaImage) -> RgbaImage {
    image::imageops::flip_horizontal(img)
}

/// LayerList に従ってバルーン画像を合成して返す
pub fn build_balloon_from_layout(
    balloon_dir: &Path,
    layers: &LayerList,
    colors: &ColorSet,
) -> anyhow::Result<RgbaImage> {
    let color_map = |ctype: &ColorType| -> Option<Rgb> {
        match ctype {
            ColorType::Edge => Some(colors.edge),
            ColorType::Base => Some(colors.base),
            ColorType::Text => Some(colors.text),
            ColorType::None => None,
        }
    };

    let mut rendered: Vec<RgbaImage> = Vec::new();
    for (fname, ctype) in layers {
        let path = balloon_dir.join(fname);
        let img = open_png_rgba(&path)?;

        let color = color_map(ctype);
        if color.is_none() || colors.no_color {
            rendered.push(img);
        } else {
            rendered.push(apply_color_overlay(&img, color.unwrap(), false));
        }
    }
    composite_layers(&rendered)
}

/// 全バルーン画像を合成して {出力ファイル名: RgbaImage} の HashMap で返す
///
/// layout を省略すると load_balloon_layout(asset_dir) を呼んで自動読み込みする。
pub fn build_all_balloons(
    asset_dir: &Path,
    colors: &ColorSet,
    layout: Option<&HashMap<String, LayerList>>,
    auto_flip: bool,
) -> anyhow::Result<HashMap<String, RgbaImage>> {
    let owned;
    let layout = match layout {
        Some(l) => l,
        None => {
            owned = load_balloon_layout(asset_dir)?;
            &owned
        }
    };

    let mut results: HashMap<String, RgbaImage> = HashMap::new();

    for (bname, layers) in layout {
        let img = build_balloon_from_layout(asset_dir, layers, colors)?;
        results.insert(format!("{}.png", bname), img);
    }

    // 自動補完: files.txt に記述がないものを反転して生成（両方向）
    let odd_flip = make_odd_flip_sources(layout, auto_flip);
    for (target, source) in &odd_flip {
        if !layout.contains_key(target) {
            let source_key = format!("{}.png", source);
            if let Some(source_img) = results.get(&source_key) {
                let flipped = flip_horizontal(source_img);
                results.insert(format!("{}.png", target), flipped);
            }
        }
    }

    Ok(results)
}

/// 指定されたパーツファイルリストを {ファイル名: RgbaImage} で返す
///
/// parts_colors が None のとき全ファイルに colors.parts を適用する。
/// parts_colors が渡されたとき、ファイルの stem に前方一致するキーの色を使い、
/// なければ "all" キーの色（= colors.parts 相当）を使う。
pub fn build_parts(
    _asset_dir: &Path,
    colors: &ColorSet,
    part_files: &[std::path::PathBuf],
    parts_colors: Option<&HashMap<String, Rgb>>,
) -> anyhow::Result<HashMap<String, RgbaImage>> {
    let mut result: HashMap<String, RgbaImage> = HashMap::new();
    let mut sorted = part_files.to_vec();
    sorted.sort();

    for f in &sorted {
        let img = open_png_rgba(f)?;

        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let color = if let Some(pc) = parts_colors {
            pc.iter()
                .find(|(k, _)| k.as_str() != "all" && stem.starts_with(k.as_str()))
                .map(|(_, v)| *v)
                .or_else(|| pc.get("all").copied())
                .unwrap_or(colors.parts)
        } else {
            colors.parts
        };

        let fname = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        result.insert(fname, apply_color_overlay(&img, color, colors.no_color));
    }
    Ok(result)
}

/// 画像中央付近の不透明ピクセルの平均 RGB を返す
///
/// sample_ratio: 中央から何割の領域をサンプリングするか（0〜0.5）
pub fn sample_center_color(img: &RgbaImage, sample_ratio: f32) -> Rgb {
    let (w, h) = img.dimensions();
    let margin_x = (w as f32 * sample_ratio) as u32;
    let margin_y = (h as f32 * sample_ratio) as u32;
    let x0 = w / 2 - margin_x.min(w / 2);
    let y0 = h / 2 - margin_y.min(h / 2);
    let x1 = (w / 2 + margin_x).min(w);
    let y1 = (h / 2 + margin_y).min(h);

    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let mut count: u64 = 0;

    for y in y0..y1 {
        for x in x0..x1 {
            let px = img.get_pixel(x, y);
            if px[3] > 128 {
                sum_r += px[0] as u64;
                sum_g += px[1] as u64;
                sum_b += px[2] as u64;
                count += 1;
            }
        }
    }

    if count == 0 {
        return Rgb(128, 128, 128);
    }
    Rgb(
        (sum_r / count) as u8,
        (sum_g / count) as u8,
        (sum_b / count) as u8,
    )
}
