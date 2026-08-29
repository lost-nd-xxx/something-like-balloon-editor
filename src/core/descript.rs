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
        // 共通側と同じ値なら差分ではないので除去（別名表記の違いも同一とみなす）
        if get_aliased(&base, key).map(|v| v.as_str()) == Some(value) {
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
///
/// raw は外部ファイル（descript.txt）由来で i32 の全域を取りうるため、
/// 加算は飽和演算で行う（debug ビルドでの overflow panic を避ける）。
pub fn pos_str(raw: &str, size: i32) -> i32 {
    let negative = raw.starts_with('-');
    let val: i32 = raw.parse().unwrap_or(0);
    if negative { size.saturating_add(val) } else { val }
}

// ---------------------------------------------------------------------------
// キーの別名（エイリアス）
// ---------------------------------------------------------------------------

/// 同一設定を指す別表記のグループ。先頭を既定表記とし、新規書き込み時に選ぶ。
/// 既定表記には対応ベースウェアの多い古くからの書き方を置く。
///
/// SSP 2.8.84 で実機確認済み: それぞれ完全に同一の設定を指す。
/// `number.x` は左起点表記用、`number.yb` は下起点であることを明示する用の別名で、
/// 値の解釈（`pos_str` 規則）はどちらの表記でも変わらない。
///
/// `sstpmessage.xr` / `sstpmessage.yb` は別名ではないのでここには入れない
/// （`yb` は `y` と対になる「描画終了位置」の別設定）。
pub const KEY_ALIASES: &[&[&str]] = &[
    &["number.xr", "number.x"],
    &["number.y",  "number.yb"],
];

/// key が属する別名グループを返す。別名を持たないキーは None。
pub fn alias_group(key: &str) -> Option<&'static [&'static str]> {
    KEY_ALIASES.iter().find(|g| g.contains(&key)).copied()
}

/// 別名を考慮して値を取得する。
///
/// 探索順は「指定キー自身 → 別名グループの並び順」。
///
/// SSP 実機は同一ファイル内に別表記が同居した場合「後勝ち」（記述順が後の行）だが、
/// `parse_descript` は HashMap へ上書き挿入するため同名キーの重複は既に後勝ちで解決済みで、
/// 異なる別名の同居時のみ探索順が問題になる。厳密な再現には行番号の保持
/// （`parse_descript` の戻り値型変更）が必要になるため、ここでは「指定キー自身を優先」の
/// 単純規則とする。同居は異常ケースであり、`set_descript_value_aliased` の書き込み時に解消される。
pub fn get_aliased<'a>(parsed: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
    if let Some(v) = parsed.get(key) {
        return Some(v);
    }
    alias_group(key)?.iter().find_map(|k| parsed.get(*k))
}

/// `key` にコメントアウトされた行（`//key,...`）が存在するか
fn has_commented_line(text: &str, key: &str) -> bool {
    Regex::new(&format!(r"(?m)^//\s*{},", regex::escape(key)))
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// 別名を考慮して値を書き込む。
///
/// 書き込み先は「素材に既に書かれている表記」を優先し、ユーザーの書き方を尊重する。
/// 決定順は 有効行を持つキー → コメントアウト行を持つキー → グループ先頭（既定表記）。
///
/// 書き込み先以外の別名に有効行が残っていた場合はコメントアウトする。
/// 放置すると書かなかった側が古い値のまま残り、どちらが有効か不定になるため。
pub fn set_descript_value_aliased(text: &str, key: &str, value: &str) -> String {
    let Some(group) = alias_group(key) else {
        return set_descript_value(text, key, value);
    };

    // 値が空＝無効化。グループ内の全キーをコメントアウトする
    if value.is_empty() {
        let mut t = text.to_string();
        for k in group {
            t = set_descript_value(&t, k, "");
        }
        return t;
    }

    let parsed = parse_descript(text);
    let target = group.iter().find(|k| parsed.contains_key(**k))
        .or_else(|| group.iter().find(|k| has_commented_line(text, k)))
        .copied()
        .unwrap_or(group[0]);

    let mut t = set_descript_value(text, target, value);
    // 書き込み先以外に有効行が残っていれば無効化して矛盾を防ぐ
    for k in group.iter().filter(|k| **k != target) {
        if parsed.contains_key(*k) {
            t = set_descript_value(&t, k, "");
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    /// どちらの表記で書かれていても引ける
    #[test]
    fn get_aliased_resolves_alias() {
        let p = parse_descript("number.x,-20\nnumber.yb,-5\n");
        assert_eq!(get_aliased(&p, "number.xr").map(|s| s.as_str()), Some("-20"));
        assert_eq!(get_aliased(&p, "number.y").map(|s| s.as_str()),  Some("-5"));
        // 逆向き（既定表記で書かれていて別名で引く）も解決する
        let p = parse_descript("number.xr,-20\nnumber.y,-5\n");
        assert_eq!(get_aliased(&p, "number.x").map(|s| s.as_str()),  Some("-20"));
        assert_eq!(get_aliased(&p, "number.yb").map(|s| s.as_str()), Some("-5"));
    }

    /// 別名を持たないキーは素通し
    #[test]
    fn get_aliased_passes_through_normal_key() {
        let p = parse_descript("arrow0.x,-5\n");
        assert_eq!(get_aliased(&p, "arrow0.x").map(|s| s.as_str()), Some("-5"));
        assert_eq!(get_aliased(&p, "arrow0.y"), None);
    }

    /// 素材が別名表記なら、その表記のまま更新される（既定表記の行を増やさない）
    #[test]
    fn set_aliased_keeps_existing_notation() {
        let out = set_descript_value_aliased("number.x,-20\n", "number.xr", "-30");
        assert!(out.contains("number.x,-30"), "actual: {out}");
        assert!(!out.contains("number.xr,"), "既定表記の行が増えている: {out}");
    }

    /// どちらの表記も無ければ既定表記で追記される
    #[test]
    fn set_aliased_adds_canonical_when_absent() {
        let out = set_descript_value_aliased("type,balloon\n", "number.xr", "-30");
        assert!(out.contains("number.xr,-30"), "actual: {out}");
        assert!(!out.contains("number.x,-30"), "別名で書かれている: {out}");
    }

    /// 両表記が同居していたら片方を更新し、もう片方はコメントアウトして矛盾を消す
    #[test]
    fn set_aliased_resolves_duplicate() {
        let out = set_descript_value_aliased("number.xr,-20\nnumber.x,-99\n", "number.xr", "-30");
        assert!(out.contains("number.xr,-30"), "actual: {out}");
        assert!(out.contains("//number.x,-99"), "別名側が無効化されていない: {out}");
        // 有効行として number.x が残っていないこと
        let p = parse_descript(&out);
        assert_eq!(p.get("number.x"), None);
        assert_eq!(p.get("number.xr").map(|s| s.as_str()), Some("-30"));
    }

    /// コメントアウト行があればそれを復活させる（既定表記を新規追加しない）
    #[test]
    fn set_aliased_revives_commented_line() {
        let out = set_descript_value_aliased("//number.x,-20\n", "number.xr", "-30");
        assert!(out.contains("number.x,-30"), "actual: {out}");
        assert!(!out.contains("number.xr,"), "既定表記の行が増えている: {out}");
    }

    /// 空値ならグループ内の全キーが無効化される
    #[test]
    fn set_aliased_empty_comments_out_whole_group() {
        let out = set_descript_value_aliased("number.xr,-20\nnumber.x,-99\n", "number.xr", "");
        let p = parse_descript(&out);
        assert_eq!(p.get("number.xr"), None);
        assert_eq!(p.get("number.x"), None);
    }

    /// pos_str の符号規則（別名対応の前提となる挙動）
    #[test]
    fn pos_str_sign_rule() {
        assert_eq!(pos_str("100", 506), 100);    // 正値＝左/上起点
        assert_eq!(pos_str("-35", 506), 471);    // 負値＝右/下起点
        assert_eq!(pos_str("", 506), 0);         // 不正値は 0
        // sstpmessage.xr/.yb の初期値 -1 は、画像サイズが変わっても
        // それぞれの右端/下端付近に解決される（正値のような固定座標にならない）
        assert_eq!(pos_str("-1", 506), 505);
        assert_eq!(pos_str("-1", 136), 135);
    }
}
