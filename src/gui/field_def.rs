/// 設定フィールドの定義データ（app.pyのACCORDION_GROUPS相当）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Color,     // 色（必須）
    ColorNone, // 色（none選択可）
    Text,      // テキスト
    Int,       // 整数（Spinbox）
    Dropdown,  // ドロップダウン選択
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub key:        &'static str,
    pub label:      &'static str,
    pub field_type: FieldType,
    pub choices:    &'static [&'static str],
    pub default:    &'static str,
}

pub const STYLE_CHOICES:  &[&str] = &["square", "underline", "square+underline", "none"];
pub const SHADOW_CHOICES: &[&str] = &["offset", "outline"];

/// 「装飾なし」チェックボックスが有効なグループの定義
/// (グループ名, blendmethodキー, &[(キー, 装飾あり時デフォルト値, 装飾なし時の値)])
/// 装飾なし = blendmethod,none + style,none + 全色,-1
/// 装飾あり = blendmethod,copypen + style/色はデフォルト値
pub const DECORATION_GROUPS: &[(&str, &str, &[(&str, &str, &str)])] = &[
    ("選択肢(非選択)", "cursor.notselect.blendmethod", &[
        ("cursor.notselect.style",        "none",  "none"),
        ("cursor.notselect.font.color.r", "0",     "-1"),
        ("cursor.notselect.font.color.g", "0",     "-1"),
        ("cursor.notselect.font.color.b", "255",   "-1"),
        ("cursor.notselect.pen.color.r",  "0",     "-1"),
        ("cursor.notselect.pen.color.g",  "0",     "-1"),
        ("cursor.notselect.pen.color.b",  "0",     "-1"),
        ("cursor.notselect.brush.color.r","0",     "-1"),
        ("cursor.notselect.brush.color.g","0",     "-1"),
        ("cursor.notselect.brush.color.b","0",     "-1"),
    ]),
    ("選択中の選択肢", "cursor.blendmethod", &[
        ("cursor.style",        "square", "none"),
        ("cursor.font.color.r", "255",    "-1"),
        ("cursor.font.color.g", "255",    "-1"),
        ("cursor.font.color.b", "255",    "-1"),
        ("cursor.pen.color.r",  "0",      "-1"),
        ("cursor.pen.color.g",  "0",      "-1"),
        ("cursor.pen.color.b",  "0",      "-1"),
        ("cursor.brush.color.r","0",      "-1"),
        ("cursor.brush.color.g","0",      "-1"),
        ("cursor.brush.color.b","255",    "-1"),
    ]),
    ("アンカー(非選択)", "anchor.notselect.blendmethod", &[
        ("anchor.notselect.style",        "none",  "none"),
        ("anchor.notselect.font.color.r", "0",     "-1"),
        ("anchor.notselect.font.color.g", "0",     "-1"),
        ("anchor.notselect.font.color.b", "255",   "-1"),
        ("anchor.notselect.pen.color.r",  "0",     "-1"),
        ("anchor.notselect.pen.color.g",  "0",     "-1"),
        ("anchor.notselect.pen.color.b",  "0",     "-1"),
        ("anchor.notselect.brush.color.r","0",     "-1"),
        ("anchor.notselect.brush.color.g","0",     "-1"),
        ("anchor.notselect.brush.color.b","0",     "-1"),
    ]),
    ("選択中のアンカー", "anchor.blendmethod", &[
        ("anchor.style",        "underline", "none"),
        ("anchor.font.color.r", "0",         "-1"),
        ("anchor.font.color.g", "0",         "-1"),
        ("anchor.font.color.b", "255",       "-1"),
        ("anchor.pen.color.r",  "0",         "-1"),
        ("anchor.pen.color.g",  "0",         "-1"),
        ("anchor.pen.color.b",  "255",       "-1"),
        ("anchor.brush.color.r","0",         "-1"),
        ("anchor.brush.color.g","0",         "-1"),
        ("anchor.brush.color.b","0",         "-1"),
    ]),
    ("訪問済みのアンカー", "anchor.visited.blendmethod", &[
        ("anchor.visited.style",        "none",  "none"),
        ("anchor.visited.font.color.r", "170",   "-1"),
        ("anchor.visited.font.color.g", "0",     "-1"),
        ("anchor.visited.font.color.b", "170",   "-1"),
        ("anchor.visited.pen.color.r",  "0",     "-1"),
        ("anchor.visited.pen.color.g",  "0",     "-1"),
        ("anchor.visited.pen.color.b",  "0",     "-1"),
        ("anchor.visited.brush.color.r","0",     "-1"),
        ("anchor.visited.brush.color.g","0",     "-1"),
        ("anchor.visited.brush.color.b","0",     "-1"),
    ]),
];

/// グループ表示対象: "c" = balloonc系のみ, "ks" = k/s/p系のみ, "all" = 両方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupVisibility { C, Ks }

pub struct AccordionGroup {
    pub name:       &'static str,
    pub visibility: GroupVisibility,
    pub fields:     &'static [FieldDef],
}

// FieldDef の static スライスを定義するためのマクロ
macro_rules! fd {
    ($key:expr, $label:expr, $ft:expr, $choices:expr, $default:expr) => {
        FieldDef { key: $key, label: $label, field_type: $ft, choices: $choices, default: $default }
    };
}

pub static ACCORDION_GROUPS: &[AccordionGroup] = &[
    AccordionGroup {
        name: "通常文字", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("font.name",        "フォント名",    FieldType::Text,       &[], "ＭＳ ゴシック"),
            fd!("font.height",      "フォントサイズ", FieldType::Int,        &[], "12"),
            fd!("font.color",       "文字色",        FieldType::Color,      &[], "#000000"),
            fd!("font.shadowcolor", "影色",          FieldType::ColorNone,  &[], "none"),
            fd!("font.shadowstyle", "影スタイル",     FieldType::Dropdown,   SHADOW_CHOICES, "offset"),
        ],
    },
    AccordionGroup {
        name: "操作無効の文字", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("disable.font.color", "文字色", FieldType::ColorNone, &[], "none"),
        ],
    },
    AccordionGroup {
        name: "選択肢(非選択)", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("cursor.notselect.style",       "強調形状", FieldType::Dropdown, STYLE_CHOICES, "none"),
            fd!("cursor.notselect.font.color",  "文字色",   FieldType::Color,    &[], "#0000ff"),
            fd!("cursor.notselect.pen.color",   "枠色",     FieldType::Color,    &[], "#000000"),
            fd!("cursor.notselect.brush.color", "塗り色",   FieldType::Color,    &[], "#000000"),
        ],
    },
    AccordionGroup {
        name: "選択中の選択肢", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("cursor.style",       "強調形状", FieldType::Dropdown, STYLE_CHOICES, "square"),
            fd!("cursor.font.color",  "文字色",   FieldType::Color,    &[], "#ffffff"),
            fd!("cursor.pen.color",   "枠色",     FieldType::Color,    &[], "#000000"),
            fd!("cursor.brush.color", "塗り色",   FieldType::Color,    &[], "#0000ff"),
        ],
    },
    AccordionGroup {
        name: "アンカー(非選択)", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("anchor.notselect.style",       "強調形状", FieldType::Dropdown, STYLE_CHOICES, "none"),
            fd!("anchor.notselect.font.color",  "文字色",   FieldType::Color,    &[], "#0000ff"),
            fd!("anchor.notselect.pen.color",   "枠色",     FieldType::Color,    &[], "#000000"),
            fd!("anchor.notselect.brush.color", "塗り色",   FieldType::Color,    &[], "#000000"),
        ],
    },
    AccordionGroup {
        name: "選択中のアンカー", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("anchor.style",       "強調形状", FieldType::Dropdown, STYLE_CHOICES, "underline"),
            fd!("anchor.font.color",  "文字色",   FieldType::Color,    &[], "#0000ff"),
            fd!("anchor.pen.color",   "枠色",     FieldType::Color,    &[], "#0000ff"),
            fd!("anchor.brush.color", "塗り色",   FieldType::Color,    &[], "#000000"),
        ],
    },
    AccordionGroup {
        name: "訪問済みのアンカー", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("anchor.visited.style",       "強調形状", FieldType::Dropdown, STYLE_CHOICES, "none"),
            fd!("anchor.visited.font.color",  "文字色",   FieldType::Color,    &[], "#aa00aa"),
            fd!("anchor.visited.pen.color",   "枠色",     FieldType::Color,    &[], "#000000"),
            fd!("anchor.visited.brush.color", "塗り色",   FieldType::Color,    &[], "#000000"),
        ],
    },
    AccordionGroup {
        name: "SSTPメッセージ", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("sstpmessage.font.name",   "フォント名",    FieldType::Text,  &[], "ＭＳ ゴシック"),
            fd!("sstpmessage.font.height", "フォントサイズ", FieldType::Int,   &[], "10"),
            fd!("sstpmessage.font.color",  "文字色",        FieldType::Color, &[], "#0000C0"),
        ],
    },
    AccordionGroup {
        name: "カウンタ数値", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("number.font.name",   "フォント名",    FieldType::Text,  &[], "ＭＳ ゴシック"),
            fd!("number.font.height", "フォントサイズ", FieldType::Int,   &[], "10"),
            fd!("number.font.color",  "文字色",        FieldType::Color, &[], "#000000"),
        ],
    },
    AccordionGroup {
        name: "入力ボックス", visibility: GroupVisibility::C,
        fields: &[
            fd!("communicatebox.font.name",        "フォント名", FieldType::Text,      &[], "ＭＳ ゴシック"),
            fd!("communicatebox.font.color",       "文字色",     FieldType::Color,     &[], "#000000"),
            fd!("communicatebox.background.color", "背景色",     FieldType::Color,     &[], "#FFFFFF"),
            fd!("communicatebox.x",      "入力域X", FieldType::Int, &[], "10"),
            fd!("communicatebox.y",      "入力域Y", FieldType::Int, &[], "10"),
            fd!("communicatebox.width",  "横幅",    FieldType::Int, &[], "100"),
            fd!("communicatebox.height", "高さ",    FieldType::Int, &[], "30"),
        ],
    },
    AccordionGroup {
        name: "UI位置", visibility: GroupVisibility::Ks,
        fields: &[
            fd!("validrect.left",    "テキスト域 左",    FieldType::Int, &[], "10"),
            fd!("validrect.top",     "テキスト域 上",    FieldType::Int, &[], "10"),
            fd!("validrect.right",   "テキスト域 右",    FieldType::Int, &[], "-10"),
            fd!("validrect.bottom",  "テキスト域 下",    FieldType::Int, &[], "-10"),
            fd!("wordwrappoint.x",   "折り返しX",        FieldType::Int, &[], "-20"),
            fd!("arrow0.x",          "矢印(上) X",       FieldType::Int, &[], "-5"),
            fd!("arrow0.y",          "矢印(上) Y",       FieldType::Int, &[], "5"),
            fd!("arrow1.x",          "矢印(下) X",       FieldType::Int, &[], "-5"),
            fd!("arrow1.y",          "矢印(下) Y",       FieldType::Int, &[], "-5"),
            fd!("clickwaitmarker.x", "クリック待ち X",   FieldType::Int, &[], "-10"),
            fd!("clickwaitmarker.y", "クリック待ち Y",   FieldType::Int, &[], "-10"),
            fd!("sstpmarker.x",      "SSTPマーカー X",   FieldType::Int, &[], "5"),
            fd!("sstpmarker.y",      "SSTPマーカー Y",   FieldType::Int, &[], "-5"),
            fd!("sstpmessage.x",     "SSTPメッセージ X", FieldType::Int, &[], "10"),
            fd!("sstpmessage.y",     "SSTPメッセージ Y", FieldType::Int, &[], "-5"),
            fd!("onlinemarker.x",    "オンライン X",     FieldType::Int, &[], "20"),
            fd!("onlinemarker.y",    "オンライン Y",     FieldType::Int, &[], "-10"),
            fd!("number.xr",         "カウンタ 右端X",   FieldType::Int, &[], "-20"),
            fd!("number.y",          "カウンタ Y",       FieldType::Int, &[], "-5"),
        ],
    },
];

