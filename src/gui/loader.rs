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

/// Windows の \\?\ UNC long-path プレフィックスを除去して通常パスを返す。
fn strip_unc_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(&s[4..])
    } else {
        path.to_path_buf()
    }
}

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
    // Windows の \\?\ UNC プレフィックスを除去して通常パスに正規化する
    let asset_dir = strip_unc_prefix(&asset_dir);

    // フォルダが存在しない場合は何もしない（新規作成直後などのタイミング差を吸収）
    if !asset_dir.is_dir() {
        return Ok(());
    }

    // slbe_files.txt のパース（なければ画像編集なしモード）
    // load_balloon_layout は旧 files.txt があれば自動移行する
    // keep_texts のときはメモリ上の slbe_files_text からパースし、ディスクは読まない
    let layout = if keep_texts && !state.slbe_files_text.is_empty() {
        // メモリ上のテキストからパース（エラー時はディスクにフォールバック）
        match crate::core::layout::parse_balloon_layout(&state.slbe_files_text, &asset_dir) {
            Ok(l) => l,
            Err(_) => load_balloon_layout(&asset_dir)?,
        }
    } else {
        load_balloon_layout(&asset_dir)?
    };
    let direct = is_direct_image_mode(&layout);
    state.direct_image_mode = direct;
    state.balloon_layout = layout.clone();
    // slbe_files.txt のテキストをメモリに読み込む（undo対象）
    // keep_texts のときはメモリ上の編集内容を保持する
    if !keep_texts {
        use crate::core::layout::slbe_files_txt_path;
        let p = slbe_files_txt_path(&asset_dir);
        state.slbe_files_text = if p.exists() {
            std::fs::read_to_string(&p).unwrap_or_default()
        } else {
            String::new()
        };
    }

    // プレビューリスト生成
    state.preview_balloons = if direct {
        detect_direct_balloons(&asset_dir, state.auto_flip)
            .into_iter()
            .map(|n| format!("{}.png", n))
            .collect()
    } else {
        let odd_flip = make_odd_flip_sources(&layout, state.auto_flip);
        let mut all_names: std::collections::HashSet<String> = layout
            .keys()
            .chain(odd_flip.keys())
            .cloned()
            .collect();
        // slbe_files.txt に定義のない物理ファイルを補完追加（優先度: slbe_files.txt > 物理ファイル）
        // コメントアウトされた行も「定義なし」扱いとなるため、物理ファイルがあれば表示する
        if let Ok(entries) = std::fs::read_dir(&asset_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("png") { continue; }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.starts_with("balloon") && !all_names.contains(stem) {
                        all_names.insert(stem.to_string());
                    }
                }
            }
        }
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
    state.load_warnings.clear();
    if !keep_texts {
        state.descript_text = read_utf8(&asset_dir.join("descript.txt"));
        state.install_text  = read_utf8(&asset_dir.join("install.txt"));
        state.readme_text   = read_utf8(&asset_dir.join("readme.txt"));

        // 文字コードチェック
        // - NonUtf8Bytes（バイト列が壊れている）: 書き換えで直せないため警告
        // - NonUtf8Charset（UTF-8で読めるがcharset宣言が異なる）:
        //     プロジェクトフォルダなら charset を UTF-8 に自動書き換えして通知、
        //     それ以外（外部素材フォルダ）は触らず警告のみ
        let is_project = state.is_project_dir();
        for fname in ["descript.txt", "install.txt"] {
            let path = asset_dir.join(fname);
            if !path.exists() { continue; }
            match check_encoding(&path) {
                EncodingStatus::NonUtf8Bytes => {
                    state.load_warnings.push(format!(
                        "{fname} が UTF-8 として読み込めませんでした。\n\
                         このアプリは UTF-8 のみ対応しています。\n\
                         文字化けが発生している場合はファイルを UTF-8 に変換してください。"
                    ));
                }
                EncodingStatus::NonUtf8Charset(cs) => {
                    if is_project {
                        // プロジェクトフォルダ: charset を UTF-8 に自動書き換え
                        if let Ok(true) = crate::core::project::ensure_charset_utf8(&path) {
                            // 再読み込み
                            if fname == "descript.txt" {
                                state.descript_text = read_utf8(&path);
                            } else {
                                state.install_text = read_utf8(&path);
                            }
                            state.load_warnings.push(format!(
                                "{fname} の charset が \"{cs}\" でしたが、UTF-8 に修正しました。"
                            ));
                        }
                    } else {
                        state.load_warnings.push(format!(
                            "{fname} の charset が \"{cs}\" に設定されています。\n\
                             このアプリは UTF-8 のみ対応しています。\n\
                             文字化けが発生している場合はファイルを UTF-8 に変換してください。"
                        ));
                    }
                }
                EncodingStatus::Ok => {}
            }
        }

        // プロジェクトフォルダのとき: 装飾エントリが未定義なら初期値を補完してファイルに書き込む
        // ロード後に descript_text を再読み込みする
        if is_project {
            let descript_path = asset_dir.join("descript.txt");
            if descript_path.exists() {
                if let Ok(filled) = crate::core::project::ensure_decoration_entries(&descript_path) {
                    state.descript_text = read_utf8(&descript_path);
                    // 新規プロジェクト作成直後は空の descript.txt から全グループを補完するため、
                    // 補完通知は不要。フラグが立っていれば通知を抑制し、ここでリセットする（1回限り）。
                    let suppress = std::mem::take(&mut state.suppress_decoration_fill_notice);
                    if !filled.is_empty() && !suppress {
                        // 補完したグループをblendmethodキー名から読みやすい名前に変換
                        let group_names: Vec<&str> = filled.iter().map(|k| match k.as_str() {
                            "cursor.blendmethod"           => "選択肢(選択中)",
                            "cursor.notselect.blendmethod" => "選択肢(非選択)",
                            "anchor.blendmethod"           => "アンカー(選択中)",
                            "anchor.notselect.blendmethod" => "アンカー(非選択)",
                            "anchor.visited.blendmethod"   => "アンカー(訪問済み)",
                            other => other,
                        }).collect();
                        state.load_warnings.push(format!(
                            "descript.txt に以下のグループの装飾設定が見つからなかったため、初期値を補完しました。\n\
                             内容を確認し、必要に応じて設定を調整してください。\n\n{}",
                            group_names.join("\n")
                        ));
                    }
                }
            }
        }

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
    if !asset_dir.is_dir() {
        return Ok(());
    }
    if state.direct_image_mode {
        // 画像編集なしモード: balloon*.png をそのまま読む
        state.balloon_cache.clear();
        for name in &state.preview_balloons.clone() {
            let stem = name.trim_end_matches(".png");
            let path = asset_dir.join(name);
            // ファイルがない場合は自動補完（両方向）
            let img = if path.exists() {
                open_png_rgba(&path)?
            } else if let Some(src) = flip_source_of(stem) {
                let src_path = asset_dir.join(format!("{}.png", src));
                if src_path.exists() {
                    crate::core::composer::flip_horizontal(&open_png_rgba(&src_path)?)
                } else {
                    continue;
                }
            } else {
                continue;
            };
            state.balloon_cache.insert(name.clone(), img);
        }
    } else {
        let cs = state.color_set();
        let images = build_all_balloons(asset_dir, &cs, Some(&state.balloon_layout), state.auto_flip)?;
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
    if !asset_dir.is_dir() {
        return Ok(());
    }
    let selected = state.selected_balloon.clone();
    // 選択バルーンが空（画像なしモードなど）は何もしない
    if selected.is_empty() {
        return Ok(());
    }
    let selected_stem = selected.trim_end_matches(".png").to_string();

    // 他のバルーンのキャッシュはクリア（古い色のまま残らないように）
    state.balloon_cache.clear();

    if state.direct_image_mode {
        // 画像編集なしモード: 選択バルーンのみ読み込む
        let path = asset_dir.join(&selected);
        let img = if path.exists() {
            open_png_rgba(&path)?
        } else if let Some(src) = flip_source_of(&selected_stem) {
            let src_path = asset_dir.join(format!("{}.png", src));
            if src_path.exists() {
                crate::core::composer::flip_horizontal(&open_png_rgba(&src_path)?)
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };
        state.balloon_cache.insert(selected.clone(), img);
    } else {
        let cs = state.color_set_for(Some(&selected_stem));
        let layout = &state.balloon_layout;

        if let Some(layers) = layout.get(&selected_stem) {
            let img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
            state.balloon_cache.insert(selected.clone(), img);
        } else if let Some(src) = flip_source_of(&selected_stem) {
            if let Some(layers) = layout.get(&src) {
                let src_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                let flipped = crate::core::composer::flip_horizontal(&src_img);
                state.balloon_cache.insert(selected.clone(), flipped);
            }
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
    if !asset_dir.is_dir() {
        return Ok(());
    }
    let selected = state.selected_balloon.clone();
    // 選択バルーンが空（画像なしモードなど）は何もしない
    if selected.is_empty() {
        return Ok(());
    }
    if state.balloon_cache.contains_key(&selected) {
        return Ok(());
    }
    let selected_stem = selected.trim_end_matches(".png").to_string();

    if state.direct_image_mode {
        let path = asset_dir.join(&selected);
        let img = if path.exists() {
            open_png_rgba(&path)?
        } else if let Some(src) = flip_source_of(&selected_stem) {
            let src_path = asset_dir.join(format!("{}.png", src));
            if src_path.exists() {
                crate::core::composer::flip_horizontal(&open_png_rgba(&src_path)?)
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };
        state.balloon_cache.insert(selected, img);
    } else {
        let cs = state.color_set_for(Some(&selected_stem));
        let layout = &state.balloon_layout;

        if let Some(layers) = layout.get(&selected_stem) {
            let img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
            state.balloon_cache.insert(selected, img);
        } else if let Some(src) = flip_source_of(&selected_stem) {
            if let Some(layers) = layout.get(&src) {
                let src_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                let flipped = crate::core::composer::flip_horizontal(&src_img);
                state.balloon_cache.insert(selected, flipped);
            }
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
            } else if let Some(src) = flip_source_of(stem) {
                let src_path = asset_dir.join(format!("{}.png", src));
                if src_path.exists() {
                    crate::core::composer::flip_horizontal(&open_png_rgba(&src_path)?)
                } else {
                    continue;
                }
            } else {
                continue;
            };
            result.insert(name.clone(), img);
        }
    } else {
        let layout = &state.balloon_layout;
        for name in &state.preview_balloons {
            let stem = name.trim_end_matches(".png");
            // 個別色があればそちらを優先、なければ共通色
            let cs = state.color_set_for(Some(stem));

            let img = if let Some(layers) = layout.get(stem) {
                // files.txt に直接定義あり
                crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?
            } else if let Some(src) = flip_source_of(stem).filter(|s| layout.contains_key(s)) {
                if let Some(layers) = layout.get(&src) {
                    let src_img = crate::core::composer::build_balloon_from_layout(asset_dir, layers, &cs)?;
                    crate::core::composer::flip_horizontal(&src_img)
                } else {
                    continue;
                }
            } else {
                {
                    continue;
                }
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

enum EncodingStatus {
    Ok,
    /// バイト列が UTF-8 として不正
    NonUtf8Bytes,
    /// UTF-8 として読めたが charset 行が UTF-8 以外を宣言している
    NonUtf8Charset(String),
}

/// ファイルの文字コードを検査する。
/// 1. バイト列が UTF-8 として不正 → NonUtf8Bytes
/// 2. UTF-8 として読めたが charset 行が UTF-8 以外 → NonUtf8Charset
/// 3. 問題なし → Ok
fn check_encoding(path: &Path) -> EncodingStatus {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return EncodingStatus::Ok,
    };
    // BOM 付き UTF-8 (EF BB BF) はスキップして検査
    let check_bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    match std::str::from_utf8(check_bytes) {
        Err(_) => EncodingStatus::NonUtf8Bytes,
        Ok(text) => {
            // charset 行を探して UTF-8 以外なら警告
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with("//") { continue; }
                if let Some(rest) = line.strip_prefix("charset,") {
                    let cs = rest.trim().to_lowercase();
                    if !cs.is_empty() && cs != "utf-8" && cs != "utf8" {
                        return EncodingStatus::NonUtf8Charset(rest.trim().to_string());
                    }
                    break;
                }
            }
            EncodingStatus::Ok
        }
    }
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

/// 偶数番バルーン名から対応する奇数番バルーン名を返す（偶数でなければそのまま返す）
fn odd_name_of(stem: &str) -> String {
    for prefix in &["balloonk", "balloons"] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            if let Ok(n) = rest.parse::<u32>() {
                if n % 2 == 0 {
                    return format!("{}{}", prefix, n + 1);
                }
            }
        }
    }
    stem.to_string()
}

/// stem の自動補完反転元を返す（奇数→偶数、偶数→奇数の両方向）
/// 反転元が存在しない場合は None
fn flip_source_of(stem: &str) -> Option<String> {
    let even = even_name_of(stem);
    if even != stem { return Some(even); }
    let odd = odd_name_of(stem);
    if odd != stem { return Some(odd); }
    None
}

/// 指定された画像ファイルが 24bit PNG（透過色なし）であるか判定します。
pub fn is_24bit_png(path: &Path) -> bool {
    if let Ok(dyn_img) = image::open(path) {
        match dyn_img.color() {
            image::ColorType::Rgba8 | image::ColorType::Rgba16 | image::ColorType::Rgba32F => false,
            _ => true,
        }
    } else {
        false
    }
}

/// 選択された画像を物理的にプロジェクトフォルダへコピーします。
/// 24bit PNGの隣に同名 .pna が存在する場合、.pna も追従コピーします。
/// 競合（同名ファイル）が存在する場合、一時退避およびロールバックによる安全対策を行います。
pub fn import_image_file_safe(
    state: &AppState,
    src_png_path: &Path,
    target_filename: &str,
) -> anyhow::Result<()> {
    let asset_dir = state
        .asset_dir()
        .ok_or_else(|| anyhow::anyhow!("プロジェクトフォルダが選択されていません"))?;

    let dest_png_path = asset_dir.join(target_filename);
    let backup_png_path = dest_png_path.with_extension("png_backup");

    // pna のパス（PNGの拡張子を pna に変えたもの）
    let src_pna_path = src_png_path.with_extension("pna");
    let has_pna = is_24bit_png(src_png_path) && src_pna_path.exists();

    let dest_pna_path = dest_png_path.with_extension("pna");
    let backup_pna_path = dest_pna_path.with_extension("pna_backup");

    // 1. 既存競合ファイルの一時退避
    let png_existed = dest_png_path.exists();
    if png_existed {
        if backup_png_path.exists() {
            std::fs::remove_file(&backup_png_path)?;
        }
        std::fs::rename(&dest_png_path, &backup_png_path)?;
    }

    let pna_existed = dest_pna_path.exists();
    if pna_existed {
        if backup_pna_path.exists() {
            std::fs::remove_file(&backup_pna_path)?;
        }
        std::fs::rename(&dest_pna_path, &backup_pna_path)?;
    }

    // 2. 物理コピーの実行
    // 親ディレクトリの作成（念のため）
    if let Some(parent) = dest_png_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // PNGのコピー
    if let Err(e) = std::fs::copy(src_png_path, &dest_png_path) {
        // エラー時はロールバック
        rollback_import(&dest_png_path, png_existed, &backup_png_path)?;
        rollback_import(&dest_pna_path, pna_existed, &backup_pna_path)?;
        return Err(anyhow::anyhow!("PNGファイルのコピーに失敗しました: {}", e));
    }

    // PNAの追従コピー
    if has_pna {
        if let Err(e) = std::fs::copy(&src_pna_path, &dest_pna_path) {
            // エラー時はロールバック
            rollback_import(&dest_png_path, png_existed, &backup_png_path)?;
            rollback_import(&dest_pna_path, pna_existed, &backup_pna_path)?;
            return Err(anyhow::anyhow!("PNAファイルのコピーに失敗しました: {}", e));
        }
    }

    // 3. コピー成功: 退避していたバックアップをクリーンアップ
    if png_existed && backup_png_path.exists() {
        let _ = std::fs::remove_file(&backup_png_path);
    }
    if pna_existed && backup_pna_path.exists() {
        let _ = std::fs::remove_file(&backup_pna_path);
    }

    Ok(())
}

fn rollback_import(
    dest_path: &Path,
    existed: bool,
    backup_path: &Path,
) -> anyhow::Result<()> {
    if dest_path.exists() {
        let _ = std::fs::remove_file(dest_path);
    }
    if existed && backup_path.exists() {
        std::fs::rename(backup_path, dest_path)?;
    }
    Ok(())
}

