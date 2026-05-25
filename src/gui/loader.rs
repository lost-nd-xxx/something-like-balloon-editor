/// 素材フォルダの読み込みとバルーン画像キャッシュの構築を担う
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::core::{
    color::Rgb,
    composer::{build_all_balloons, build_parts, open_png_rgba},
    layout::{
        detect_direct_balloons, is_direct_image_mode, load_balloon_layout,
        make_odd_flip_sources, sort_key,
    },
};
use crate::gui::state::AppState;

/// 素材フォルダを読み込んで AppState を更新する。
/// エラーが発生した場合は Err を返す（呼び出し元がダイアログ表示する）。
pub fn load_asset_folder(state: &mut AppState, _root: &Path) -> anyhow::Result<()> {
    load_asset_folder_inner(state, false)
}

/// テキストファイルを上書きせずに素材フォルダを読み込む（プロファイル復元用）。
pub fn load_asset_folder_keep_texts(state: &mut AppState, _root: &Path) -> anyhow::Result<()> {
    load_asset_folder_inner(state, true)
}

fn load_asset_folder_inner(state: &mut AppState, keep_texts: bool) -> anyhow::Result<()> {
    let asset_dir = match state.selected_asset_dir.clone() {
        Some(p) => p,
        None => return Ok(()),
    };

    // files.txt のパース（なければ画像編集なしモード）
    let layout = load_balloon_layout(&asset_dir)?;
    let direct = is_direct_image_mode(&layout);
    state.direct_image_mode = direct;
    state.balloon_layout = layout.clone();

    // プレビューリスト生成
    state.preview_balloons = if direct {
        detect_direct_balloons(&asset_dir)
            .into_iter()
            .map(|n| format!("{}.png", n))
            .collect()
    } else {
        let odd_flip = make_odd_flip_sources(&layout);
        let all_names: std::collections::HashSet<String> = layout
            .keys()
            .chain(odd_flip.keys())
            .cloned()
            .collect();
        let mut sorted: Vec<String> = all_names.into_iter().collect();
        sorted.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        sorted.into_iter().map(|n| format!("{}.png", n)).collect()
    };

    // 選択中バルーンが新リストにない場合は先頭に戻す
    if !state.preview_balloons.contains(&state.selected_balloon) {
        state.selected_balloon = state
            .preview_balloons
            .first()
            .cloned()
            .unwrap_or_default();
    }

    // テキストファイル読み込み（keep_texts のとき既存値を保持）
    if !keep_texts {
        state.descript_text = read_utf8(&asset_dir.join("descript.txt"));
        state.install_text  = read_utf8(&asset_dir.join("install.txt"));
        state.readme_text   = read_utf8(&asset_dir.join("readme.txt"));

        // 個別設定ファイル（{バルーン名}s.txt）を自動検出して読み込む
        // 例: balloonk3.png → balloonk3s.txt
        state.individual_texts.clear();
        for balloon_name in &state.preview_balloons {
            let stem = balloon_name.trim_end_matches(".png");
            let cfg_name = format!("{}s.txt", stem);
            let cfg_path = asset_dir.join(&cfg_name);
            if cfg_path.exists() {
                let text = read_utf8(&cfg_path);
                if !text.is_empty() {
                    state.individual_texts.insert(cfg_name, text);
                }
            }
        }
    }

    // バルーン画像キャッシュをクリアして再構築
    state.balloon_cache.clear();
    state.parts_cache.clear();
    rebuild_balloon_cache(state, &asset_dir)?;

    // パーツ画像サイズから動的デフォルト値を計算
    rebuild_dynamic_defaults(state);

    Ok(())
}

/// バルーン画像キャッシュを再構築する
pub fn rebuild_balloon_cache(state: &mut AppState, asset_dir: &Path) -> anyhow::Result<()> {
    if state.direct_image_mode {
        // 画像編集なしモード: balloon*.png をそのまま読む
        state.balloon_cache.clear();
        for name in &state.preview_balloons.clone() {
            let stem = name.trim_end_matches(".png");
            let path = asset_dir.join(name);
            // 奇数番で元ファイルがない場合は偶数番を反転
            let img = if path.exists() {
                open_png_rgba(&path)?
            } else {
                // 偶数番を反転して生成
                let even = even_name_of(stem);
                let even_path = asset_dir.join(format!("{}.png", even));
                if even_path.exists() {
                    crate::core::composer::flip_horizontal(
                        &open_png_rgba(&even_path)?,
                    )
                } else {
                    continue;
                }
            };
            state.balloon_cache.insert(name.clone(), img);
        }
    } else {
        let cs = state.color_set();
        let images = build_all_balloons(asset_dir, &cs, Some(&state.balloon_layout))?;
        state.balloon_cache = images;
    }

    // パーツキャッシュ
    let part_files = collect_part_files(asset_dir);
    let cs = state.color_set();
    let pc: HashMap<String, Rgb> = state.parts_colors.clone();
    let parts = build_parts(asset_dir, &cs, &part_files, Some(&pc))?;
    // 全バルーン共通で同じパーツを使う
    for name in &state.preview_balloons.clone() {
        state.parts_cache.insert(name.clone(), parts.clone());
    }

    Ok(())
}

/// 選択バルーンのみ合成してキャッシュを更新する（Undo/Redo 後の高速プレビュー用）。
/// 他のバルーンのキャッシュはクリアし、選択時に遅延合成される。
pub fn rebuild_selected_balloon(state: &mut AppState, asset_dir: &Path) -> anyhow::Result<()> {
    let selected = state.selected_balloon.clone();
    let selected_stem = selected.trim_end_matches(".png").to_string();

    // 他のバルーンのキャッシュはクリア（古い色のまま残らないように）
    state.balloon_cache.clear();

    if state.direct_image_mode {
        // 画像編集なしモード: 選択バルーンのみ読み込む
        let path = asset_dir.join(&selected);
        let img = if path.exists() {
            open_png_rgba(&path)?
        } else {
            let even = even_name_of(&selected_stem);
            let even_path = asset_dir.join(format!("{}.png", even));
            if even_path.exists() {
                crate::core::composer::flip_horizontal(&open_png_rgba(&even_path)?)
            } else {
                return Ok(());
            }
        };
        state.balloon_cache.insert(selected.clone(), img);
    } else {
        let cs = state.color_set_for(Some(&selected_stem));
        let layout = &state.balloon_layout;

        // 奇数バルーンの場合は反転元（偶数）も合成が必要
        let even_stem = even_name_of(&selected_stem);
        let needs_flip = even_stem != selected_stem && !layout.contains_key(&selected_stem);

        if needs_flip {
            // 偶数番を合成して反転
            if let Some(layers) = layout.get(&even_stem) {
                let even_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                let flipped = crate::core::composer::flip_horizontal(&even_img);
                state.balloon_cache.insert(selected.clone(), flipped);
            }
        } else if let Some(layers) = layout.get(&selected_stem) {
            let img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
            state.balloon_cache.insert(selected.clone(), img);
        }
    }

    // パーツキャッシュ（選択バルーンのみ更新）
    state.parts_cache.clear();
    let part_files = collect_part_files(asset_dir);
    let cs = state.color_set_for(Some(&selected.trim_end_matches(".png")));
    let pc = state.parts_colors_for(Some(&selected.trim_end_matches(".png")));
    let parts = build_parts(asset_dir, &cs, &part_files, Some(&pc))?;
    for name in &state.preview_balloons.clone() {
        state.parts_cache.insert(name.clone(), parts.clone());
    }

    Ok(())
}

/// 指定バルーン名のキャッシュが無ければ合成して追加する（バルーン選択時の遅延合成）。
pub fn ensure_balloon_cached(state: &mut AppState, asset_dir: &Path) -> anyhow::Result<()> {
    let selected = state.selected_balloon.clone();
    if state.balloon_cache.contains_key(&selected) {
        return Ok(());
    }
    let selected_stem = selected.trim_end_matches(".png").to_string();

    if state.direct_image_mode {
        let path = asset_dir.join(&selected);
        let img = if path.exists() {
            open_png_rgba(&path)?
        } else {
            let even = even_name_of(&selected_stem);
            let even_path = asset_dir.join(format!("{}.png", even));
            if even_path.exists() {
                crate::core::composer::flip_horizontal(&open_png_rgba(&even_path)?)
            } else {
                return Ok(());
            }
        };
        state.balloon_cache.insert(selected, img);
    } else {
        let cs = state.color_set_for(Some(&selected_stem));
        let layout = &state.balloon_layout;
        let even_stem = even_name_of(&selected_stem);
        let needs_flip = even_stem != selected_stem && !layout.contains_key(&selected_stem);

        if needs_flip {
            if let Some(layers) = layout.get(&even_stem) {
                let even_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                let flipped = crate::core::composer::flip_horizontal(&even_img);
                state.balloon_cache.insert(selected, flipped);
            }
        } else if let Some(layers) = layout.get(&selected_stem) {
            let img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
            state.balloon_cache.insert(selected, img);
        }
    }

    Ok(())
}

/// 出力用に全バルーンを個別色を考慮して合成して返す。
/// individual_colors が設定されているバルーンはそれぞれ個別色で合成する。
pub fn build_all_balloons_for_export(
    state: &AppState,
    asset_dir: &Path,
) -> anyhow::Result<HashMap<String, image::RgbaImage>> {
    let mut result: HashMap<String, image::RgbaImage> = HashMap::new();

    if state.direct_image_mode {
        // 画像編集なしモード: 元ファイルをそのまま使う
        for name in &state.preview_balloons {
            let stem = name.trim_end_matches(".png");
            let path = asset_dir.join(name);
            let img = if path.exists() {
                open_png_rgba(&path)?
            } else {
                let even = even_name_of(stem);
                let even_path = asset_dir.join(format!("{}.png", even));
                if even_path.exists() {
                    crate::core::composer::flip_horizontal(&open_png_rgba(&even_path)?)
                } else {
                    continue;
                }
            };
            result.insert(name.clone(), img);
        }
    } else {
        let layout = &state.balloon_layout;
        for name in &state.preview_balloons {
            let stem = name.trim_end_matches(".png");
            // 個別色があればそちらを優先、なければ共通色
            let cs = state.color_set_for(Some(stem));
            let even_stem = even_name_of(stem);
            let needs_flip = even_stem != stem && !layout.contains_key(stem);

            let img = if needs_flip {
                if let Some(layers) = layout.get(&even_stem) {
                    let even_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                    crate::core::composer::flip_horizontal(&even_img)
                } else {
                    continue;
                }
            } else if let Some(layers) = layout.get(stem) {
                crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?
            } else {
                continue;
            };
            result.insert(name.clone(), img);
        }
    }

    Ok(result)
}

/// asset_dir からパーツファイル（arrow*, marker*, online*, sstp*, clickwait*, ok_*, cancel_*, mode_*）を収集する
fn collect_part_files(asset_dir: &Path) -> Vec<PathBuf> {
    let patterns = [
        "arrow", "marker", "online", "sstp", "clickwait",
        "ok_", "cancel_", "mode_",
    ];
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(asset_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("png") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if patterns.iter().any(|p| stem.starts_with(p)) {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

fn read_utf8(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// パーツ・バルーン画像サイズから動的デフォルト値を計算して state に格納する
pub fn rebuild_dynamic_defaults(state: &mut AppState) {
    let mut d = std::collections::HashMap::new();

    // 代表バルーンのサイズ（オンラインマーカー位置計算用）
    let balloon_size = state.preview_balloons.first()
        .and_then(|name| state.balloon_cache.get(name))
        .map(|img| (img.width() as i32, img.height() as i32));

    // parts_cache は全バルーン共通なので最初の1エントリから取る
    let parts = state.preview_balloons.first()
        .and_then(|name| state.parts_cache.get(name));

    let get = |key: &str| -> Option<(u32, u32)> {
        parts.and_then(|p| p.get(key)).map(|img| (img.width(), img.height()))
    };

    if let Some((aw, _)) = get("arrow0.png") {
        d.insert("arrow0.x".to_string(), format!("-{}", 5 + aw));
    }
    if let Some((aw, ah)) = get("arrow1.png") {
        d.insert("arrow1.x".to_string(), format!("-{}", 5 + aw));
        d.insert("arrow1.y".to_string(), format!("-{}", 5 + ah));
    }

    // clickwait: 専用画像があれば -(10+size)、なければ arrow1 デフォルトと同じ
    if let Some((cw, ch)) = get("clickwait.png") {
        d.insert("clickwaitmarker.x".to_string(), format!("-{}", 10 + cw));
        d.insert("clickwaitmarker.y".to_string(), format!("-{}", 10 + ch));
    } else {
        if let Some(v) = d.get("arrow1.x").cloned() { d.insert("clickwaitmarker.x".to_string(), v); }
        if let Some(v) = d.get("arrow1.y").cloned() { d.insert("clickwaitmarker.y".to_string(), v); }
    }

    // sstp: sstp_new.png → sstp.png の順
    let sstp_size = get("sstp_new.png").or_else(|| get("sstp.png"));
    if let Some((_, sh)) = sstp_size {
        d.insert("sstpmarker.y".to_string(), format!("-{}", 5 + sh));
    }

    // online: バルーン下部中央
    if let (Some((ow, oh)), Some((iw, ih))) = (get("online0.png"), balloon_size) {
        d.insert("onlinemarker.x".to_string(), ((iw - ow as i32) / 2).to_string());
        d.insert("onlinemarker.y".to_string(), (ih - oh as i32).to_string());
    }

    state.dynamic_defaults = d;
}

/// "balloonk1" → "balloonk0" のように、奇数番→偶数番名を返す
fn even_name_of(stem: &str) -> String {
    for prefix in &["balloonk", "balloons"] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            if let Ok(n) = rest.parse::<u32>() {
                if n % 2 == 1 {
                    return format!("{}{}", prefix, n - 1);
                }
            }
        }
    }
    stem.to_string()
}
