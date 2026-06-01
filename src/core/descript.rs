/// descript.txt / install.txt のパースと編集を担う
use std::collections::HashMap;
use regex::Regex;
use crate::core::color::Rgb;

/// コメント行を除いてキー→値の HashMap を返す
pub fn parse_descript(text: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(pos) = line.find(',') {
            let key   = line[..pos].trim().to_string();
            // 値の先頭空白のみトリム（末尾空白はユーザー入力で意味を持つ場合があるため残す）
            let value = line[pos + 1..].trim_start().to_string();
            result.insert(key, value);
        }
    }
    result
}

/// `key,xxx` 行を `key,value` に置換する。行がなければ末尾に追加。コメント行は保持。
///
/// value が空文字のときは該当行をコメントアウト（`//key,old_value`）する。
pub fn set_descript_value(text: &str, key: &str, value: &str) -> String {
    let pattern = Regex::new(&format!(r"(?m)^({key_esc}),(.*)$", key_esc = regex::escape(key)))
        .unwrap();
    let commented = Regex::new(&format!(r"(?m)^//\s*{key_esc},(.*)$", key_esc = regex::escape(key)))
        .unwrap();

    if value.is_empty() {
        // 既存の有効行をコメントアウト（なければ何もしない）
        if pattern.is_match(text) {
            return pattern.replace_all(text, |caps: &regex::Captures| {
                format!("//{},{}", &caps[1], &caps[2])
            }).into_owned();
        }
        return text.to_string();
    }

    if pattern.is_match(text) {
        return pattern.replace_all(text, |caps: &regex::Captures| {
            format!("{},{}", &caps[1], value)
        }).into_owned();
    }

    // コメントアウト済みの行があれば復活
    if commented.is_match(text) {
        return commented.replace_all(text, |_caps: &regex::Captures| {
            format!("{},{}", key, value)
        }).into_owned();
    }

    // なければ末尾に追加
    format!("{}\n{},{}\n", text.trim_end(), key, value)
}

/// `{prefix}.r` / `.g` / `.b` の3キーから Rgb を読む。
/// `.r` が "none" または存在しなければ None を返す。
pub fn get_color_from_descript(parsed: &HashMap<String, String>, prefix: &str) -> Option<Rgb> {
    let r_str = parsed.get(&format!("{}.r", prefix))?;
    if r_str.to_lowercase() == "none" {
        return None;
    }
    let r: u8 = r_str.parse().ok()?;
    let g: u8 = parsed.get(&format!("{}.g", prefix))?.parse().ok()?;
    let b: u8 = parsed.get(&format!("{}.b", prefix))?.parse().ok()?;
    Some(Rgb(r, g, b))
}

/// `{prefix}.r/.g/.b` の3キーに Rgb を書き込む（None なら "none" を設定）
pub fn set_color_in_descript(text: &str, prefix: &str, color: Option<Rgb>) -> String {
    let mut t = text.to_string();
    match color {
        None => {
            for ch in &["r", "g", "b"] {
                t = set_descript_value(&t, &format!("{}.{}", prefix, ch), "none");
            }
        }
        Some(c) => {
            t = set_descript_value(&t, &format!("{}.r", prefix), &c.0.to_string());
            t = set_descript_value(&t, &format!("{}.g", prefix), &c.1.to_string());
            t = set_descript_value(&t, &format!("{}.b", prefix), &c.2.to_string());
        }
    }
    t
}

/// 個別設定テキスト（個別バルーン用）から、共通 descript と同一の値を持つ行を
/// 取り除いて「差分のみ」のテキストを返す。
///
/// 個別設定ファイル（{stem}s.txt）は本来 descript.txt からの差分のみを保持する仕様。
/// 編集時は利便性のため descript 全体のコピーを保持しているため、保存前にここで
/// 差分（共通と値が異なる／共通に無い有効キー行）だけに圧縮する。
///
/// descript からコピーされたコメント・空行をそのまま残すと冗長で人間が混乱するため、
/// コメント行・空行・キーを持たない行は出力しない。結果は「有効な差分キー行のみ」。
pub fn diff_against_descript(individual_text: &str, descript_text: &str) -> String {
    let base = parse_descript(descript_text);
    let mut out: Vec<&str> = Vec::new();
    for line in individual_text.lines() {
        let trimmed = line.trim();
        // コメント行・空行は出力しない
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let Some(pos) = trimmed.find(',') else {
            // キー,値 形式でない行は差分として扱えないのでスキップ
            continue;
        };
        let key = trimmed[..pos].trim();
        let value = trimmed[pos + 1..].trim_start();
        // 共通側と同じ値なら差分ではないので除去
        if base.get(key).map(|v| v.as_str()) == Some(value) {
            continue;
        }
        out.push(line);
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{}\n", out.join("\n"))
    }
}

/// マイナス値座標の変換。
/// raw が "-" 始まりのとき `size + val`（右下基点）、そうでなければ `val`（左上基点）。
pub fn pos_str(raw: &str, size: i32) -> i32 {
    let negative = raw.starts_with('-');
    let val: i32 = raw.parse().unwrap_or(0);
    if negative { size + val } else { val }
}
