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

/// 必須バルーン名
const REQUIRED_BALLOONS: &[&str] = &[
    "balloonc0", "balloonc1", "balloonc2", "balloonc3", "balloons0",
];

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

    // 必須バルーンの存在チェック
    let mut required: Vec<&str> = REQUIRED_BALLOONS.to_vec();
    required.sort_unstable();
    for req in required {
        if !layout.contains_key(req) {
            errors.push(format!("必須バルーン 「{}」 が定義されていません。", req));
        }
    }

    if !errors.is_empty() {
        let msg = errors
            .iter()
            .map(|e| format!("  • {}", e))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!("files.txt にエラーがあります:\n\n{}", msg);
    }

    Ok(layout)
}

/// asset_dir/files.txt を読んでパース結果を返す。
/// files.txt が存在しない場合は空の HashMap を返す（画像編集なしモード）。
pub fn load_balloon_layout(asset_dir: &Path) -> anyhow::Result<HashMap<String, LayerList>> {
    // フォルダ自体が存在しない場合は空レイアウトを返す
    if !asset_dir.is_dir() {
        return Ok(HashMap::new());
    }
    let files_txt = asset_dir.join("files.txt");
    if !files_txt.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(&files_txt)?;
    parse_balloon_layout(&text, asset_dir)
}

/// layout が空のとき「画像編集なしモード」と判定する
pub fn is_direct_image_mode(layout: &HashMap<String, LayerList>) -> bool {
    layout.is_empty()
}

/// layout のキーから奇数番バルーンの反転元辞書を動的に生成する
///
/// 返り値: {奇数番バルーン名: 反転元偶数番バルーン名}
pub fn make_odd_flip_sources(layout: &HashMap<String, LayerList>) -> HashMap<String, String> {
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
        let max_even = nums.iter().filter(|&&n| n % 2 == 0).copied().max().unwrap_or(0);
        for even in (0..=max_even).step_by(2) {
            let odd = even + 1;
            let odd_name  = format!("{}{}", prefix, odd);
            let even_name = format!("{}{}", prefix, even);
            if !layout.contains_key(&odd_name) && layout.contains_key(&even_name) {
                result.insert(odd_name, even_name);
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
        let max_even = nums.iter().filter(|&&n| n % 2 == 0).copied().max().unwrap_or(0);
        for even in (0..=max_even).step_by(2) {
            let odd = even + 1;
            let odd_name  = format!("{}{}", base, odd);
            let even_name = format!("{}{}", base, even);
            if !layout.contains_key(&odd_name) && layout.contains_key(&even_name) {
                result.insert(odd_name, even_name);
            }
        }
    }

    result
}

/// 画像編集なしモード時に asset_dir から balloon*.png を収集し、
/// 奇数番の自動反転対象も含めたバルーン名リストを返す
pub fn detect_direct_balloons(asset_dir: &Path) -> Vec<String> {
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

    // balloonk / balloons 系の奇数反転対象を追加
    for prefix in &["balloonk", "balloons"] {
        let nums: Vec<u32> = found
            .iter()
            .filter_map(|s| s.strip_prefix(prefix).and_then(|r| r.parse().ok()))
            .collect();
        if nums.is_empty() { continue; }
        let max_even = nums.iter().filter(|&&n| n % 2 == 0).copied().max().unwrap_or(0);
        for even in (0..=max_even).step_by(2) {
            let even_name = format!("{}{}", prefix, even);
            let odd_name  = format!("{}{}", prefix, even + 1);
            if found.contains(&even_name) && !found.contains(&odd_name) {
                result.insert(odd_name);
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
        let max_even = nums.iter().filter(|&&n| n % 2 == 0).copied().max().unwrap_or(0);
        for even in (0..=max_even).step_by(2) {
            let even_name = format!("{}{}", base, even);
            let odd_name  = format!("{}{}", base, even + 1);
            if found.contains(&even_name) && !found.contains(&odd_name) {
                result.insert(odd_name);
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
