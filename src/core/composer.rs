/// バルーン画像の合成・色オーバーレイ・左右反転を担う
use std::collections::HashMap;
use std::path::Path;
use image::{ImageBuffer, Rgba, RgbaImage};
use crate::core::color::{ColorSet, Rgb};
use crate::core::layout::{ColorType, LayerList, load_balloon_layout, make_odd_flip_sources};

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
        let img = image::open(&path)
            .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?
            .to_rgba8();

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

    // 奇数バルーン: files.txt に記述がないものは偶数番を反転して生成
    let odd_flip = make_odd_flip_sources(layout);
    for (odd, even) in &odd_flip {
        if !layout.contains_key(odd) {
            let even_key = format!("{}.png", even);
            if let Some(even_img) = results.get(&even_key) {
                let flipped = flip_horizontal(even_img);
                results.insert(format!("{}.png", odd), flipped);
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
        let img = image::open(f)
            .map_err(|e| anyhow::anyhow!("{}: {}", f.display(), e))?
            .to_rgba8();

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
