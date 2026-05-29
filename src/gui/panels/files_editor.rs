/// profile/slbe/files.txt のGUIエディタウィンドウ（別ウィンドウ）
use egui::Context;
use crate::gui::app::BalloonEditorApp;
use crate::core::layout::sort_key;

// ---------------------------------------------------------------------------
// バルーン名のスコープ/番号表現
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum BalloonScope {
    Normal { scope: i32, id: u32 },
    BalloonC { id: usize },
}

impl BalloonScope {
    pub fn to_name(&self) -> String {
        match self {
            BalloonScope::Normal { scope: 0, id } => format!("balloons{}", id),
            BalloonScope::Normal { scope: 1, id } => format!("balloonk{}", id),
            BalloonScope::Normal { scope, id }    => format!("balloonp{}def{}", scope, id),
            BalloonScope::BalloonC { id }         => format!("balloonc{}", id),
        }
    }

    fn parse(name: &str) -> Self {
        if let Some(rest) = name.strip_prefix("balloonc") {
            return BalloonScope::BalloonC { id: rest.parse().unwrap_or(0).min(4) };
        }
        if let Some(rest) = name.strip_prefix("balloons") {
            return BalloonScope::Normal { scope: 0, id: rest.parse().unwrap_or(0) };
        }
        if let Some(rest) = name.strip_prefix("balloonk") {
            return BalloonScope::Normal { scope: 1, id: rest.parse().unwrap_or(0) };
        }
        if let Some(rest) = name.strip_prefix("balloonp") {
            if let Some(def_pos) = rest.find("def") {
                let scope: i32 = rest[..def_pos].parse().unwrap_or(2);
                let id: u32    = rest[def_pos + 3..].parse().unwrap_or(0);
                return BalloonScope::Normal { scope, id };
            }
        }
        BalloonScope::Normal { scope: 0, id: 0 }
    }
}

// ---------------------------------------------------------------------------
// 内部表現
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum FilesRow {
    Comment(String),
    Balloon { scope: BalloonScope, layers: Vec<LayerEntry> },
}

#[derive(Debug, Clone)]
pub struct LayerEntry {
    pub filename: String,
    pub color_type: String,
}
impl LayerEntry {
    fn new() -> Self { Self { filename: String::new(), color_type: "base".to_string() } }
}

// ---------------------------------------------------------------------------
// テキスト ⇔ 内部表現 変換
// ---------------------------------------------------------------------------

pub fn parse_rows(text: &str) -> Vec<FilesRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            // r対策: 空行も含めコメント行は // 始まりに正規化
            let normalized = if trimmed.is_empty() {
                "// ".to_string()
            } else {
                line.to_string()
            };
            rows.push(FilesRow::Comment(normalized));
        } else {
            let parts: Vec<&str> = line.split(',').collect();
            let balloon_name = parts[0].trim().to_string();
            let mut layers = Vec::new();
            for token in parts.iter().skip(1) {
                let token = token.trim();
                if token.is_empty() { continue; }
                if let Some(colon) = token.find(':') {
                    layers.push(LayerEntry {
                        filename: token[..colon].trim().to_string(),
                        color_type: token[colon + 1..].trim().to_string(),
                    });
                } else {
                    layers.push(LayerEntry { filename: token.to_string(), color_type: "base".to_string() });
                }
            }
            rows.push(FilesRow::Balloon { scope: BalloonScope::parse(&balloon_name), layers });
        }
    }
    rows
}

pub fn rows_to_text(rows: &[FilesRow]) -> String {
    let lines: Vec<String> = rows.iter().map(|row| match row {
        FilesRow::Comment(s) => {
            // テキスト部分が空の "//" 行はファイル書き込み時に空行として出力する
            let trimmed = s.trim();
            if trimmed == "//" || trimmed.is_empty() {
                String::new()
            } else {
                s.clone()
            }
        }
        FilesRow::Balloon { scope, layers } => {
            let mut parts = vec![scope.to_name()];
            for l in layers { parts.push(format!("{}:{}", l.filename, l.color_type)); }
            parts.join(",")
        }
    }).collect();
    if lines.is_empty() { String::new() } else { format!("{}\n", lines.join("\n")) }
}

fn sort_balloon_rows(rows: &[FilesRow]) -> Vec<FilesRow> {
    let mut balloon_rows: Vec<FilesRow> = rows.iter()
        .filter(|r| matches!(r, FilesRow::Balloon { .. }))
        .cloned().collect();
    balloon_rows.sort_by(|a, b| {
        let na = match a { FilesRow::Balloon { scope, .. } => scope.to_name(), _ => String::new() };
        let nb = match b { FilesRow::Balloon { scope, .. } => scope.to_name(), _ => String::new() };
        sort_key(&na).cmp(&sort_key(&nb))
    });
    balloon_rows
}

fn balloon_to_comment_str(scope: &BalloonScope, layers: &[LayerEntry]) -> String {
    let mut parts = vec![scope.to_name()];
    for l in layers { parts.push(format!("{}:{}", l.filename, l.color_type)); }
    format!("// {}", parts.join(","))
}

fn comment_to_balloon(s: &str) -> FilesRow {
    let raw = s.trim().strip_prefix("//").unwrap_or(s.trim()).trim();
    let parts: Vec<&str> = raw.split(',').collect();
    let balloon_name = parts.first().map(|s| s.trim().to_string()).unwrap_or_default();
    let mut layers = Vec::new();
    for token in parts.iter().skip(1) {
        let token = token.trim();
        if token.is_empty() { continue; }
        if let Some(colon) = token.find(':') {
            layers.push(LayerEntry {
                filename: token[..colon].trim().to_string(),
                color_type: token[colon + 1..].trim().to_string(),
            });
        } else {
            layers.push(LayerEntry { filename: token.to_string(), color_type: "base".to_string() });
        }
    }
    if layers.is_empty() { layers.push(LayerEntry::new()); }
    FilesRow::Balloon { scope: BalloonScope::parse(&balloon_name), layers }
}

fn find_duplicates(rows: &[FilesRow]) -> std::collections::HashSet<String> {
    let mut seen = std::collections::HashSet::new();
    let mut dups = std::collections::HashSet::new();
    for row in rows {
        if let FilesRow::Balloon { scope, .. } = row {
            let name = scope.to_name();
            if !seen.insert(name.clone()) { dups.insert(name); }
        }
    }
    dups
}

// ---------------------------------------------------------------------------
// PNG一覧取得
// ---------------------------------------------------------------------------
fn list_png_files(asset_dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(asset_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("png")).unwrap_or(false)
            {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

const COLOR_TYPES: &[(&str, &str)] = &[
    ("base", "塗り色"),
    ("edge", "縁取り色"),
    ("text", "題字色"),
    ("none", "塗り替えなし"),
];

// ---------------------------------------------------------------------------
// メインUI描画関数
// ---------------------------------------------------------------------------

fn draw_editor_ui(
    ui: &mut egui::Ui,
    rows_in: &[FilesRow],
    png_files: &[String],
) -> (Vec<FilesRow>, bool) {
    let mut rows = rows_in.to_vec();
    let mut changed = false;

    // ---- 説明エリア ----
    egui::CollapsingHeader::new("📋 書式・色種別について")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("書式:  バルーン名,ファイル名:色種別,...  （奥レイヤーから順に記述）");
            ui.add_space(4.0);
            ui.label("色種別:");
            egui::Grid::new("color_type_help").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                for &(ct, desc) in COLOR_TYPES {
                    ui.monospace(ct);
                    ui.label(desc);
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            ui.label("コメント行:  先頭が // の行はパース時に無視されます。");
            ui.label("「→コメント」ボタンでコメント行に変換、「→定義行」ボタンで定義行に戻せます。");
        });

    ui.add_space(4.0);

    // ---- ツールバー ----
    let (do_sort, do_remove_comments) = ui.horizontal(|ui| {
        let sort = ui.button("⬆ バルーン順に並び替え").on_hover_text(
            "バルーン定義行を balloons → balloonk → balloonpdef → balloonc 順。\nコメント行は除外されます。"
        ).clicked();
        let remove = ui.button("🗑 コメント行を一括削除").on_hover_text("すべてのコメント行を削除します。").clicked();
        (sort, remove)
    }).inner;

    if do_sort            { rows = sort_balloon_rows(&rows); changed = true; }
    if do_remove_comments { rows.retain(|r| matches!(r, FilesRow::Balloon { .. })); changed = true; }

    ui.separator();

    let duplicates = find_duplicates(&rows);

    // g対策: ScrollArea に入る前に幅を確定させる。
    // ScrollArea 内で available_width() を使うとスクロールバーの出現で毎フレーム変動し累積誤差が出る。
    // 左固定カラム幅(148) + スクロールバー余白(16) + padding(8) を引いた値を TextEdit 幅として全行共通で使う。
    let comment_left_w   = 148.0_f32;
    let scrollbar_margin = 24.0_f32;
    let comment_text_w   = (ui.available_width() - comment_left_w - scrollbar_margin).max(20.0);

    // ---- 行リスト ----
    let mut to_delete: Option<usize>          = None;
    let mut to_convert_comment: Option<usize> = None;
    let mut to_convert_balloon: Option<usize> = None;
    let mut insert_balloon_after: Option<usize> = None;
    let mut insert_comment_after: Option<usize> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, row) in rows.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                match row {
                    // ---- コメント行 ----
                    FilesRow::Comment(text) => {
                        // r対策: // プレフィックスを編集不可ラベルとして表示し、
                        //        それ以降のテキストのみ編集可能にする
                        // g対策: ScrollArea 外で確定した comment_text_w を使い幅ブレを防ぐ
                        //   左: [→定義行][+↓][🗑][//] 固定幅  右: TextEdit 固定幅
                        // textは常に "// " で始まることが parse_rows で保証されている

                        let prefix = "// ";
                        let editable_start = if text.starts_with(prefix) { prefix.len() } else { 0 };
                        let mut editable = text[editable_start..].to_string();

                        ui.horizontal(|ui| {
                            if ui.small_button("→定義行")
                                .on_hover_text("バルーン定義行に変換").clicked()
                            {
                                to_convert_balloon = Some(i);
                            }
                            if ui.small_button("+↓")
                                .on_hover_text("直下にコメント行を追加").clicked()
                            {
                                insert_comment_after = Some(i);
                            }
                            if ui.small_button("🗑")
                                .on_hover_text("この行を削除").clicked()
                            {
                                to_delete = Some(i);
                            }
                            ui.colored_label(egui::Color32::from_rgb(100, 180, 100), "//");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut editable)
                                    .desired_width(comment_text_w)
                                    .text_color(egui::Color32::from_rgb(100, 180, 100))
                            );
                            if resp.changed() {
                                *text = format!("// {}", editable);
                                changed = true;
                            }
                        });
                    }

                    // ---- バルーン定義行 ----
                    FilesRow::Balloon { scope, layers } => {
                        let name = scope.to_name();
                        let is_dup = duplicates.contains(&name);
                        let frame = if is_dup {
                            egui::Frame::group(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(180, 60, 60, 40))
                                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 80, 80)))
                        } else {
                            egui::Frame::group(ui.style())
                        };

                        frame.show(ui, |ui| {
                            // q: group内を横幅100%に
                            ui.set_min_width(ui.available_width());

                            // --- ヘッダ行 ---
                            ui.horizontal(|ui| {
                                if ui.small_button("→コメント").on_hover_text("コメント行（// ）に変換").clicked() {
                                    to_convert_comment = Some(i);
                                }

                                match scope {
                                    BalloonScope::BalloonC { id } => {
                                        ui.label("スコープ: c系");
                                        ui.label("番号(0-4):");
                                        let mut v = *id as i32;
                                        if ui.add(egui::DragValue::new(&mut v).range(0..=4)).changed() {
                                            *id = v as usize; changed = true;
                                        }
                                    }
                                    BalloonScope::Normal { scope: scope_num, id } => {
                                        ui.label("スコープ:");
                                        if ui.add(egui::DragValue::new(scope_num).range(0..=99))
                                            .on_hover_text("0=sakura(s)  1=kero(k)  2以上=p*def")
                                            .changed() { changed = true; }
                                        ui.label("番号:");
                                        if ui.add(egui::DragValue::new(id).range(0..=9999)).changed() {
                                            changed = true;
                                        }
                                    }
                                }

                                // バルーン名プレビュー
                                let preview = scope.to_name();
                                if is_dup {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 80, 80),
                                        format!("⚠ {} (重複)", preview),
                                    );
                                } else {
                                    ui.label(format!("→ {}", preview));
                                }

                                // u: c系切り替えボタンをバルーン名プレビューの右隣に配置
                                let is_c = matches!(scope, BalloonScope::BalloonC { .. });
                                let toggle_label = if is_c { "→通常系" } else { "→c系" };
                                let toggle_hint  = if is_c {
                                    "balloons/k/pdef 系に切り替え"
                                } else {
                                    "balloonc（入力ボックス）系に切り替え"
                                };
                                if ui.small_button(toggle_label).on_hover_text(toggle_hint).clicked() {
                                    *scope = if is_c {
                                        BalloonScope::Normal { scope: 0, id: 0 }
                                    } else {
                                        BalloonScope::BalloonC { id: 0 }
                                    };
                                    changed = true;
                                }
                            });

                            // --- レイヤーリスト ---
                            let mut layer_to_delete = None;
                            for (li, layer) in layers.iter_mut().enumerate() {
                                ui.push_id(li, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("  L{}:", li + 1));
                                        egui::ComboBox::from_id_salt(("fname", i, li))
                                            .selected_text(if layer.filename.is_empty() { "（未選択）".to_string() } else { layer.filename.clone() })
                                            .width(200.0)
                                            .show_ui(ui, |ui| {
                                                for fname in png_files {
                                                    if ui.selectable_label(&layer.filename == fname, fname).clicked() {
                                                        layer.filename = fname.clone(); changed = true;
                                                    }
                                                }
                                            });
                                        let ctype_label = COLOR_TYPES.iter()
                                            .find(|&&(k, _)| k == layer.color_type)
                                            .map(|&(k, desc)| format!("{} ({})", k, desc))
                                            .unwrap_or_else(|| layer.color_type.clone());
                                        egui::ComboBox::from_id_salt(("ctype", i, li))
                                            .selected_text(ctype_label)
                                            .width(155.0)
                                            .show_ui(ui, |ui| {
                                                for &(ct, desc) in COLOR_TYPES {
                                                    if ui.selectable_label(layer.color_type == ct, format!("{} ({})", ct, desc)).clicked() {
                                                        layer.color_type = ct.to_string(); changed = true;
                                                    }
                                                }
                                            });
                                        if ui.small_button("🗑").on_hover_text("このレイヤーを削除").clicked() {
                                            layer_to_delete = Some(li);
                                        }
                                    });
                                });
                            }
                            if let Some(li) = layer_to_delete { layers.remove(li); changed = true; }

                            // s: 重複警告・行操作ボタンをレイヤー追加ボタンの下に配置
                            ui.horizontal(|ui| {
                                if ui.small_button("+ レイヤーを追加").clicked() {
                                    layers.push(LayerEntry::new()); changed = true;
                                }
                                if ui.small_button("+定義↓").on_hover_text("直下にバルーン定義行を追加").clicked() {
                                    insert_balloon_after = Some(i);
                                }
                                if ui.small_button("+コメ↓").on_hover_text("直下にコメント行を追加").clicked() {
                                    insert_comment_after = Some(i);
                                }
                                if ui.small_button("🗑 削除").on_hover_text("このバルーン定義行を削除").clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        });
                    }
                }
            });
            ui.add_space(2.0);
        }

        // 末尾追加ボタン
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("+ バルーン定義行を追加").clicked() {
                rows.push(FilesRow::Balloon {
                    scope: BalloonScope::Normal { scope: 0, id: 0 },
                    layers: vec![LayerEntry::new()],
                });
                changed = true;
            }
            if ui.button("+ コメント行を追加").clicked() {
                rows.push(FilesRow::Comment("// ".to_string()));
                changed = true;
            }
        });
    });

    // 行操作の適用（ScrollArea外）
    if let Some(idx) = to_delete {
        rows.remove(idx); changed = true;
    } else if let Some(idx) = to_convert_comment {
        let s = match &rows[idx] {
            FilesRow::Balloon { scope, layers } => balloon_to_comment_str(scope, layers),
            _ => "// ".to_string(),
        };
        rows[idx] = FilesRow::Comment(s);
        changed = true;
    }
    if let Some(idx) = to_convert_balloon {
        let s = match &rows[idx] { FilesRow::Comment(s) => s.clone(), _ => String::new() };
        rows[idx] = comment_to_balloon(&s);
        changed = true;
    }
    if let Some(idx) = insert_balloon_after {
        rows.insert(idx + 1, FilesRow::Balloon {
            scope: BalloonScope::Normal { scope: 0, id: 0 },
            layers: vec![LayerEntry::new()],
        });
        changed = true;
    }
    if let Some(idx) = insert_comment_after {
        rows.insert(idx + 1, FilesRow::Comment("// ".to_string()));
        changed = true;
    }

    (rows, changed)
}

// ---------------------------------------------------------------------------
// ステータスバー
// ---------------------------------------------------------------------------
fn draw_status_bar(ui: &mut egui::Ui, rows: &[FilesRow], dirty: bool) -> (bool, bool) {
    let def_count = rows.iter().filter(|r| matches!(r, FilesRow::Balloon { .. })).count();
    let cmt_count = rows.iter().filter(|r| matches!(r, FilesRow::Comment(_))).count();
    let dups = find_duplicates(rows);
    let has_dup = !dups.is_empty();

    let (apply, close) = ui.horizontal(|ui| {
        ui.label(format!("定義行: {}  コメント行: {}", def_count, cmt_count));
        if dirty && !has_dup {
            ui.colored_label(egui::Color32::from_rgb(200, 140, 0), "● 未適用の変更あり");
        }
        if has_dup {
            let mut names: Vec<&str> = dups.iter().map(|s| s.as_str()).collect();
            names.sort();
            ui.colored_label(
                egui::Color32::from_rgb(220, 60, 60),
                format!("⚠ 重複: {}", names.join(", ")),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.button("閉じる").clicked();
            let apply = ui.add_enabled(dirty && !has_dup, egui::Button::new("適用")).clicked();
            (apply, close)
        }).inner
    }).inner;
    (apply, close)
}

// ---------------------------------------------------------------------------
// エントリポイント
// ---------------------------------------------------------------------------

pub fn show(app: &mut BalloonEditorApp, ctx: &Context) {
    if !app.state.show_files_editor_window { return; }
    let Some(asset_dir) = app.state.asset_dir() else { return };

    let png_files = list_png_files(&asset_dir);
    let rows_in   = parse_rows(&app.state.slbe_files_text);

    use std::sync::{Arc, Mutex};

    let result: Arc<Mutex<Option<(Vec<FilesRow>, bool, bool)>>> = Arc::new(Mutex::new(None));
    // o: 確認ダイアログを経由した「閉じる」要求
    let close_discard: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let result2        = Arc::clone(&result);
    let close_discard2 = Arc::clone(&close_discard);
    let rows_in2       = rows_in.clone();
    let png_files2     = png_files.clone();

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("files_editor"),
        egui::ViewportBuilder::default()
            .with_title("レイアウト定義を編集  (profile/slbe/files.txt)")
            .with_inner_size([700.0, 520.0])
            .with_min_inner_size([480.0, 300.0]),
        move |vp_ctx, _class| {
            // ×ボタン → dirty なら確認、そうでなければ即閉じ
            let x_close = vp_ctx.input(|i| i.viewport().close_requested());

            let mem_key = egui::Id::new("files_editor_working_text");
            let working_text: String = vp_ctx.memory_mut(|m| {
                m.data.get_temp::<String>(mem_key)
                    .unwrap_or_else(|| rows_to_text(&rows_in2))
            });
            let working_rows = parse_rows(&working_text);
            let original_text = rows_to_text(&rows_in2);
            let dirty = working_text != original_text;

            let mut apply_clicked = false;
            let mut close_clicked = false;
            let mut editor_rows: Option<Vec<FilesRow>> = None;
            let mut editor_changed = false;

            egui::TopBottomPanel::bottom("files_editor_status")
                .show(vp_ctx, |ui| {
                    ui.separator();
                    let (a, c) = draw_status_bar(ui, &working_rows, dirty);
                    apply_clicked = a;
                    close_clicked = c;
                });

            egui::CentralPanel::default().show(vp_ctx, |ui| {
                let (new_rows, ch) = draw_editor_ui(ui, &working_rows, &png_files2);
                editor_rows = Some(new_rows);
                editor_changed = ch;
            });

            if editor_changed {
                if let Some(ref new_rows) = editor_rows {
                    vp_ctx.memory_mut(|m| m.data.insert_temp(mem_key, rows_to_text(new_rows)));
                }
            }

            // o: 未適用変更がある状態で閉じる要求がきたら確認ダイアログを表示
            let confirm_key = egui::Id::new("files_editor_confirm_close");
            let confirm_open: bool = vp_ctx.memory(|m| m.data.get_temp(confirm_key).unwrap_or(false));

            let wants_close = close_clicked || x_close;
            if wants_close && dirty && !confirm_open {
                vp_ctx.memory_mut(|m| m.data.insert_temp(confirm_key, true));
            }

            if confirm_open {
                egui::Window::new("未適用の変更があります")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(vp_ctx, |ui| {
                        ui.label("変更が適用されていません。どうしますか？");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("適用して閉じる").clicked() {
                                apply_clicked = true;
                                close_clicked = true;
                                vp_ctx.memory_mut(|m| {
                                    m.data.remove::<bool>(confirm_key);
                                    m.data.remove::<String>(mem_key);
                                });
                            }
                            if ui.button("適用せず閉じる").clicked() {
                                // o: メモリをクリアしてから閉じる。
                                // Viewport は close_requested を cancel できないため
                                // close_discard フラグで親に伝える
                                vp_ctx.memory_mut(|m| {
                                    m.data.remove::<bool>(confirm_key);
                                    m.data.remove::<String>(mem_key);
                                });
                                *close_discard2.lock().unwrap() = true;
                            }
                            if ui.button("キャンセル").clicked() {
                                vp_ctx.memory_mut(|m| m.data.remove::<bool>(confirm_key));
                            }
                        });
                    });
            }

            // dirty がないとき / 適用が確定したとき に result へ書き込む
            let do_close_clean = wants_close && !dirty && !confirm_open;
            if apply_clicked || do_close_clean {
                let final_rows = editor_rows.clone().unwrap_or(working_rows);
                *result2.lock().unwrap() = Some((final_rows, apply_clicked, close_clicked || do_close_clean));
                if close_clicked || do_close_clean {
                    vp_ctx.memory_mut(|m| m.data.remove::<String>(mem_key));
                }
            }
        },
    );

    // 結果を AppState に反映
    let should_discard_close = *close_discard.lock().unwrap();
    if let Some((updated_rows, apply, close_req)) = result.lock().unwrap().take() {
        if apply {
            app.state.push_undo();
            app.state.slbe_files_text = rows_to_text(&updated_rows);
            match crate::core::layout::parse_balloon_layout(&app.state.slbe_files_text, &asset_dir) {
                Ok(layout) => {
                    app.state.balloon_layout = layout;
                    app.state.direct_image_mode =
                        crate::core::layout::is_direct_image_mode(&app.state.balloon_layout);
                }
                Err(_) => {}
            }
            app.reload_asset_folder_keep_texts(ctx);
            app.rebuild_and_refresh(ctx);
        }
        if close_req { app.state.show_files_editor_window = false; }
    }
    if should_discard_close {
        app.state.show_files_editor_window = false;
    }
}
