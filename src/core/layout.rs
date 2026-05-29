/// files.txt のパースと奇数番バルーンの反転元解決を担う
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;
use regex::Regex;

fn re_pdef_group() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(balloonp\d+def)(\d+)$").unwrap())
}

fn re_direct() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(balloonk|balloons|balloonp\d+def)(\d+)$").unwrap())
}

fn re_sort_pdef() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^balloonp(\d+)def(\d+)$").unwrap())
}

/// 色種別
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorType {
    Base,
    Edge,
    Text,
    None,
}

impl ColorType {
    fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "base" => Ok(Self::Base),
            "edge" => Ok(Self::Edge),
            "text" => Ok(Self::Text),
            "none" => Ok(Self::None),
            other  => anyhow::bail!("不明な color_type 「{}」。使用可能: base, edge, text, none", other),
        }
    }
}

/// 1バルーン分のレイヤー定義（ファイル名 + 色種別 のリスト）
pub type LayerList = Vec<(String, ColorType)>;


/// files.txt テキストをパースして {バルーン名: LayerList} を返す
///
/// balloon_dir はファイル存在チェックに使う。
/// エラーがあれば全エラーをまとめた anyhow::Error を返す。
pub fn parse_balloon_layout(
    text: &str,
    balloon_dir: &Path,
) -> anyhow::Result<HashMap<String, LayerList>> {
    let mut layout: HashMap<String, LayerList> = HashMap::new();
    let mut errors: Vec<String> = Vec::new();

    for (lineno, raw_line) in text.lines().enumerate() {
        let lineno = lineno + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        let balloon_name = parts[0].trim().to_string();
        if balloon_name.is_empty() {
            errors.push(format!("行{}: バルーン名が空です。", lineno));
            continue;
        }

        if parts.len() < 2 {
            errors.push(format!(
                "行{} [{}]: レイヤー指定がありません。書式: balloonNAME,file.png:color_type,...",
                lineno, balloon_name
            ));
            continue;
        }

        let mut layers: LayerList = Vec::new();
        let mut has_error = false;

        for token in &parts[1..] {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let Some(colon_pos) = token.find(':') else {
                errors.push(format!(
                    "行{} [{}]: 「{}」に color_type がありません。書式: ファイル名.png:color_type",
                    lineno, balloon_name, token
                ));
                has_error = true;
                continue;
            };
            let fname = token[..colon_pos].trim();
            let ctype_str = token[colon_pos + 1..].trim();

            // パスセパレータや上位参照を拒否
            if fname.contains('/') || fname.contains('\\') || Path::new(fname).components().any(|c| {
                matches!(c, std::path::Component::ParentDir)
            }) {
                errors.push(format!("行{} [{}]: 不正なファイル名 → {}", lineno, balloon_name, fname));
                has_error = true;
                continue;
            }

            let ctype = match ColorType::parse(ctype_str) {
                Ok(t) => t,
                Err(e) => {
                    errors.push(format!("行{} [{}]: {}", lineno, balloon_name, e));
                    has_error = true;
                    continue;
                }
            };

            if !balloon_dir.join(fname).exists() {
                errors.push(format!(
                    "行{} [{}]: ファイルが見つかりません → {}",
                    lineno, balloon_name, fname
                ));
                has_error = true;
                continue;
            }

            layers.push((fname.to_string(), ctype));
        }

        if !has_error || !layers.is_empty() {
            layout.insert(balloon_name, layers);
        }
    }


    if !errors.is_empty() {
        let msg = errors
            .iter()
            .map(|e| format!("  • {}", e))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!("slbe_files.txt にエラーがあります:\n\n{}", msg);
    }

    Ok(layout)
}

/// `asset_dir/profile/slbe_files.txt` のパスを返す（旧 `files.txt` からの改名）。
pub fn slbe_files_txt_path(asset_dir: &Path) -> std::path::PathBuf {
    asset_dir.join("profile").join("slbe_files.txt")
}

/// asset_dir/profile/slbe_files.txt を読んでパース結果を返す。
/// ファイルが存在しない場合は空の HashMap を返す（画像編集なしモード）。
///
/// 旧パス `asset_dir/files.txt` が存在する場合は `profile/slbe_files.txt` へ自動移行する。
pub fn load_balloon_layout(asset_dir: &Path) -> anyhow::Result<HashMap<String, LayerList>> {
    // フォルダ自体が存在しない場合は空レイアウトを返す
    if !asset_dir.is_dir() {
        return Ok(HashMap::new());
    }

    let new_path = slbe_files_txt_path(asset_dir);
    let old_path = asset_dir.join("files.txt");

    // 旧 files.txt が存在し新パスがまだない場合: 内容を検証してから自動移行
    // パース成功かつ空でない場合のみ移行する（このアプリ以外の files.txt は移行しない）
    if old_path.exists() && !new_path.exists() {
        let old_text = std::fs::read_to_string(&old_path)?;
        let is_valid_slbe = match parse_balloon_layout(&old_text, asset_dir) {
            Ok(layout) => !layout.is_empty(),
            Err(_) => false,
        };
        if is_valid_slbe {
            if let Some(parent) = new_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&old_path, &new_path)?;
        }
    }

    if !new_path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(&new_path)?;
    parse_balloon_layout(&text, asset_dir)
}

/// layout が空のとき「画像編集なしモード」と判定する
pub fn is_direct_image_mode(layout: &HashMap<String, LayerList>) -> bool {
    layout.is_empty()
}

/// layout のキーから自動補完対象の反転元辞書を動的に生成する
///
/// auto_flip=true の場合、偶数番→奇数番、奇数番→偶数番の両方向を補完する。
/// 返り値: {補完先バルーン名: 反転元バルーン名}
pub fn make_odd_flip_sources(layout: &HashMap<String, LayerList>, auto_flip: bool) -> HashMap<String, String> {
    if !auto_flip {
        return HashMap::new();
    }

    let mut result: HashMap<String, String> = HashMap::new();

    // balloonk / balloons 系
    for prefix in &["balloonk", "balloons"] {
        let nums: Vec<u32> = layout
            .keys()
            .filter_map(|k| {
                k.strip_prefix(prefix)
                    .and_then(|rest| rest.parse::<u32>().ok())
            })
            .collect();
        if nums.is_empty() {
            continue;
        }
        let max_num = nums.iter().copied().max().unwrap_or(0);
        // 偶数→奇数 と 奇数→偶数 の両方向を補完
        let pairs: Vec<(u32, u32)> = (0..=max_num)
            .step_by(2)
            .map(|even| (even, even + 1))
            .collect();
        for (even, odd) in pairs {
            let even_name = format!("{}{}", prefix, even);
            let odd_name  = format!("{}{}", prefix, odd);
            // 偶数あり・奇数なし → 奇数を補完
            if layout.contains_key(&even_name) && !layout.contains_key(&odd_name) {
                result.insert(odd_name, even_name);
            }
            // 奇数あり・偶数なし → 偶数を補完
            else if layout.contains_key(&odd_name) && !layout.contains_key(&even_name) {
                result.insert(even_name, odd_name);
            }
        }
    }

    // balloonpPdef 系
    let pdef_re = re_pdef_group();
    let mut pdef_groups: HashMap<String, Vec<u32>> = HashMap::new();
    for key in layout.keys() {
        if let Some(caps) = pdef_re.captures(key) {
            let base = caps[1].to_string();
            let num: u32 = caps[2].parse().unwrap_or(0);
            pdef_groups.entry(base).or_default().push(num);
        }
    }
    for (base, nums) in &pdef_groups {
        let max_num = nums.iter().copied().max().unwrap_or(0);
        let pairs: Vec<(u32, u32)> = (0..=max_num)
            .step_by(2)
            .map(|even| (even, even + 1))
            .collect();
        for (even, odd) in pairs {
            let even_name = format!("{}{}", base, even);
            let odd_name  = format!("{}{}", base, odd);
            if layout.contains_key(&even_name) && !layout.contains_key(&odd_name) {
                result.insert(odd_name, even_name);
            } else if layout.contains_key(&odd_name) && !layout.contains_key(&even_name) {
                result.insert(even_name, odd_name);
            }
        }
    }

    result
}

/// 画像編集なしモード時に asset_dir から balloon*.png を収集し、
/// 自動補完対象も含めたバルーン名リストを返す
pub fn detect_direct_balloons(asset_dir: &Path, auto_flip: bool) -> Vec<String> {
    let pdef_re = re_direct();

    let mut found: HashSet<String> = HashSet::new();
    if let Ok(entries) = std::fs::read_dir(asset_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("png") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.starts_with("balloon") {
                    found.insert(stem.to_string());
                }
            }
        }
    }

    let mut result = found.clone();

    if auto_flip {
        // balloonk / balloons 系：偶数→奇数、奇数→偶数の両方向を補完
        for prefix in &["balloonk", "balloons"] {
            let nums: Vec<u32> = found
                .iter()
                .filter_map(|s| s.strip_prefix(prefix).and_then(|r| r.parse().ok()))
                .collect();
            if nums.is_empty() { continue; }
            let max_num = nums.iter().copied().max().unwrap_or(0);
            for even in (0..=max_num).step_by(2) {
                let even_name = format!("{}{}", prefix, even);
                let odd_name  = format!("{}{}", prefix, even + 1);
                if found.contains(&even_name) && !found.contains(&odd_name) {
                    result.insert(odd_name);
                } else if found.contains(&odd_name) && !found.contains(&even_name) {
                    result.insert(even_name);
                }
            }
        }

        // pdef 系
        let mut pdef_groups: HashMap<String, Vec<u32>> = HashMap::new();
        for stem in &found {
            if let Some(caps) = pdef_re.captures(stem) {
                let base = caps[1].to_string();
                let num: u32 = caps[2].parse().unwrap_or(0);
                pdef_groups.entry(base).or_default().push(num);
            }
        }
        for (base, nums) in &pdef_groups {
            let max_num = nums.iter().copied().max().unwrap_or(0);
            for even in (0..=max_num).step_by(2) {
                let even_name = format!("{}{}", base, even);
                let odd_name  = format!("{}{}", base, even + 1);
                if found.contains(&even_name) && !found.contains(&odd_name) {
                    result.insert(odd_name);
                } else if found.contains(&odd_name) && !found.contains(&even_name) {
                    result.insert(even_name);
                }
            }
        }
    }

    let mut sorted: Vec<String> = result.into_iter().collect();
    sorted.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    sorted
}

/// バルーン名のソートキー（balloons → balloonk → balloonpdef → balloonc の順）
pub fn sort_key(name: &str) -> (u8, u32, u32, u32) {
    let pdef_re = re_sort_pdef();
    for (idx, prefix) in ["balloons", "balloonk"].iter().enumerate() {
        if let Some(rest) = name.strip_prefix(prefix) {
            let n: u32 = rest.parse().unwrap_or(999);
            return (idx as u8, 0, n, 0);
        }
    }
    if let Some(caps) = pdef_re.captures(name) {
        let p: u32 = caps[1].parse().unwrap_or(0);
        let d: u32 = caps[2].parse().unwrap_or(0);
        return (2, p, 0, d);
    }
    if let Some(rest) = name.strip_prefix("balloonc") {
        let n: u32 = rest.parse().unwrap_or(999);
        return (3, 0, n, 0);
    }
    (4, 0, 0, 0)
}
