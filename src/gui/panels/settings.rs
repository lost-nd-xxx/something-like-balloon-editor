use egui::{Context, ScrollArea, Ui};
use crate::gui::app::BalloonEditorApp;
use crate::gui::field_def::{
    ACCORDION_GROUPS, FieldType, GroupVisibility,
};
use crate::core::descript::{
    parse_descript, set_descript_value, get_color_from_descript, set_color_in_descript,
};
use crate::core::color::Rgb;
use crate::gui::state::DragEditTarget;

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    ui.strong("バルーン設定");
    ui.separator();

    ScrollArea::vertical().show(ui, |ui| {
        show_basic_info_section(ui, app);
        ui.separator();
        egui::CollapsingHeader::new("バルーン設定詳細")
            .default_open(true)
            .show(ui, |ui| {
                show_accordion_settings(ui, app, ctx);
            });
        ui.separator();
        show_drag_edit_section(ui, app);
    });
}

fn show_basic_info_section(ui: &mut Ui, app: &mut BalloonEditorApp) {
    egui::CollapsingHeader::new("基本情報")
        .default_open(true)
        .show(ui, |ui| {
            let fields: &[(&str, &str)] = &[
                ("name",        "バルーン名"),
                ("craftman",    "作者名(半角英数)"),
                ("craftmanw",   "作者名"),
                ("craftmanurl", "作者URL"),
                ("homeurl",     "更新URL"),
                ("directory",   "インストール先フォルダ名"),
            ];

            egui::Grid::new("basic_info_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (key, label) in fields {
                        ui.label(*label);
                        let val = app.state.basic_info.entry(key.to_string()).or_default();
                        let mut text = val.clone();
                        let resp = egui::TextEdit::singleline(&mut text)
                            .lock_focus(true)
                            .show(ui)
                            .response;
                        if resp.gained_focus() {
                            app.state.push_undo();
                        }
                        if resp.changed() {
                            app.state.basic_info.insert(key.to_string(), text.clone());
                            if *key == "directory" {
                                app.state.install_text = set_descript_value(
                                    &app.state.install_text, key, &text);
                            } else {
                                app.state.descript_text = set_descript_value(
                                    &app.state.descript_text, key, &text);
                            }
                            if *key == "name" {
                                app.state.install_text = set_descript_value(
                                    &app.state.install_text, key, &text);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

fn show_accordion_settings(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    let is_c = app.state.is_balloonc();

    // 選択バルーンに対応するdescriptテキストを取得する
    // 個別設定ファイル名は「バルーン名 + s.txt」（例: balloonk0s.txt）
    let balloon_name = app.state.selected_balloon.trim_end_matches(".png").to_string();
    let cfg_key = format!("{}s.txt", balloon_name);

    // 個別設定を持てるバルーンかどうか判定
    let can_have_individual = can_have_individual_config(&balloon_name);

    // descが存在する場合は個別、なければ共通を使う
    let use_individual = !balloon_name.is_empty()
        && app.state.individual_texts.contains_key(&cfg_key);

    // 個別設定 追加/削除 UI
    if can_have_individual {
        ui.horizontal(|ui| {
            if use_individual {
                ui.label(format!("個別設定: {}", cfg_key));
                if ui.button("削除").clicked() {
                    app.state.push_undo();
                    app.state.individual_texts.remove(&cfg_key);
                    app.state.individual_colors.remove(&balloon_name);
                    app.rebuild_selected_and_refresh(ctx);
                }
            } else {
                ui.label("個別設定: なし");
                if ui.button("追加").clicked() {
                    app.state.push_undo();
                    // グローバルdescriptをコピーして個別設定を初期化
                    app.state.individual_texts.insert(cfg_key.clone(), app.state.descript_text.clone());
                    // 個別色はグローバル色のコピーで初期化
                    let base_colors = app.state.individual_colors
                        .get(&balloon_name).cloned()
                        .unwrap_or_else(|| crate::gui::state::IndivColors {
                            base:        app.state.layer_colors.get("base").copied(),
                            edge:        app.state.layer_colors.get("edge").copied(),
                            text:        app.state.layer_colors.get("text").copied(),
                            parts_colors: app.state.parts_colors.clone(),
                        });
                    app.state.individual_colors.insert(balloon_name.clone(), base_colors);
                    app.rebuild_selected_and_refresh(ctx);
                }
            }
        });
        ui.separator();
    }

    // 表示可否フラグ
    let show_c  = is_c;
    let show_ks = !is_c;

    for group in ACCORDION_GROUPS {
        let visible = match group.visibility {
            GroupVisibility::C   => show_c,
            GroupVisibility::Ks  => show_ks,
        };
        if !visible {
            continue;
        }

        egui::CollapsingHeader::new(group.name)
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new(group.name)
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        for field in group.fields {
                            // 影スタイルは対応する影色が none のときグレーアウト
                            let enabled = if field.key.ends_with(".shadowstyle") {
                                let shadow_color_key = field.key.trim_end_matches("style").to_string() + "color.r";
                                let v = resolve_field_value(app, &shadow_color_key, &cfg_key, use_individual);
                                v.to_lowercase() != "none"
                            } else {
                                true
                            };
                            ui.label(field.label);
                            ui.add_enabled_ui(enabled, |ui| {
                                show_field_widget(ui, app, ctx, field, &cfg_key, use_individual);
                            });
                            ui.end_row();
                        }
                    });
            });
    }
}

/// 個別設定→共通設定→dynamic_defaults→field.default の優先順でキーの値を解決する。
/// 個別設定にキーがなければ共通設定にフォールバックする（差分設定ファイル仕様への対応）。
fn resolve_field_value(
    app: &BalloonEditorApp,
    field_key: &str,
    cfg_key: &str,
    use_individual: bool,
) -> String {
    if use_individual {
        // 個別設定にキーがあればそちらを優先
        let indiv_text = app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default();
        let indiv_parsed = parse_descript(&indiv_text);
        if let Some(v) = indiv_parsed.get(field_key) {
            return v.clone();
        }
    }
    // 共通設定にフォールバック
    let global_parsed = parse_descript(&app.state.descript_text);
    if let Some(v) = global_parsed.get(field_key) {
        return v.clone();
    }
    // dynamic_defaults → field.default
    app.state.dynamic_defaults.get(field_key).cloned()
        .unwrap_or_default()
}

fn show_field_widget(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &egui::Context,
    field: &crate::gui::field_def::FieldDef,
    cfg_key: &str,
    use_individual: bool,
) {
    // 現在値を優先順位に従って解決:
    // 個別設定 → 共通設定(descript_text) → dynamic_defaults → field.default
    let current_str = {
        let v = resolve_field_value(app, field.key, cfg_key, use_individual);
        if v.is_empty() { field.default.to_string() } else { v }
    };

    match field.field_type {
        FieldType::Color | FieldType::ColorNone => {
            // 色は .r/.g/.b 形式で解決する。個別設定 → 共通設定の順で parsed を合成。
            let indiv_parsed = if use_individual {
                parse_descript(&app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default())
            } else {
                std::collections::HashMap::new()
            };
            let global_parsed = parse_descript(&app.state.descript_text);

            // キーの参照: 個別設定になければ共通設定から取る
            let get_key = |k: &str| -> Option<String> {
                indiv_parsed.get(k).cloned().or_else(|| global_parsed.get(k).cloned())
            };

            let r_key = format!("{}.r", field.key);
            let r_val = get_key(&r_key).map(|s| s.to_lowercase());
            let color_str = if r_val.as_deref() == Some("none") {
                "none".to_string()
            } else {
                // .r/.g/.b を合成した一時マップで get_color_from_descript を呼ぶ
                let mut merged = global_parsed.clone();
                for (k, v) in &indiv_parsed { merged.insert(k.clone(), v.clone()); }
                if let Some(rgb) = get_color_from_descript(&merged, field.key) {
                    format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2)
                } else {
                    field.default.to_string()
                }
            };
            let allow_none = field.field_type == FieldType::ColorNone;
            show_color_widget(ui, app, ctx, field.key, &color_str, cfg_key, use_individual, allow_none);
        }
        FieldType::Text => {
            show_text_widget(ui, app, ctx, field, &current_str, cfg_key, use_individual);
        }
        FieldType::Int => {
            show_int_widget(ui, app, ctx, field, &current_str, cfg_key, use_individual);
        }
        FieldType::Dropdown => {
            show_dropdown_widget(ui, app, ctx, field, &current_str, cfg_key, use_individual);
        }
    }
}

/// 色ピッカーウィジェット（noneチェックボックス付き）
/// current_str: 呼び出し元で解決済みの "#RRGGBB" または "none" または field.default
fn show_color_widget(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &egui::Context,
    key: &str,
    current_str: &str,
    cfg_key: &str,
    use_individual: bool,
    allow_none: bool,
) {
    let descript_text = if use_individual {
        app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default()
    } else {
        app.state.descript_text.clone()
    };

    // none 判定（ColorNone フィールドで .r が "none" のとき current_str == "none"）
    let is_none = allow_none && current_str.to_lowercase() == "none";

    // 色を解析（current_str は #RRGGBB または field.default の #RRGGBB）
    let rgb_opt = if is_none {
        None
    } else {
        parse_hex_color(current_str)
    };

    let mut changed = false;
    let mut new_rgb = rgb_opt.unwrap_or(Rgb(0, 0, 0));
    let mut new_none = is_none;

    // テキストボックス用の編集バッファ（hex入力中の一時文字列）
    let hex_key = format!("__color_hex__{}", key);
    // hex_buf: 現在の表示用 #RRGGBB 文字列
    let committed_hex = if new_none {
        String::new()
    } else {
        format!("#{:02X}{:02X}{:02X}", new_rgb.0, new_rgb.1, new_rgb.2)
    };

    ui.horizontal(|ui| {
        if allow_none {
            if ui.checkbox(&mut new_none, "none").changed() {
                changed = true;
            }
        }

        if !new_none {
            let mut color32 = rgb_to_color32(new_rgb);
            if egui::color_picker::color_edit_button_srgba(
                ui, &mut color32, egui::color_picker::Alpha::Opaque,
            ).changed() {
                new_rgb = color32_to_rgb(color32);
                changed = true;
            }

            // hex テキストボックス（編集中はバッファ、それ以外は committed_hex を表示）
            let is_editing = ui.memory(|m| m.has_focus(egui::Id::new(&hex_key)));
            let mut hex_text = if is_editing {
                ui.data(|d| d.get_temp::<String>(egui::Id::new(&hex_key)))
                    .unwrap_or_else(|| committed_hex.clone())
            } else {
                committed_hex.clone()
            };

            let resp = ui.add(
                egui::TextEdit::singleline(&mut hex_text)
                    .id(egui::Id::new(&hex_key))
                    .desired_width(58.0)
                    .font(egui::TextStyle::Monospace),
            );

            if resp.gained_focus() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(&hex_key), committed_hex.clone()));
            }
            if resp.changed() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(&hex_key), hex_text.clone()));
            }
            if resp.lost_focus() {
                // フォーカス喪失時にパースしてコミット
                let s = hex_text.trim().trim_start_matches('#');
                if s.len() == 6 {
                    if let Some(rgb) = parse_hex_color(&format!("#{}", s)) {
                        new_rgb = rgb;
                        changed = true;
                    }
                }
                ui.data_mut(|d| d.remove::<String>(egui::Id::new(&hex_key)));
            }
        }
    });

    if changed {
        app.state.push_undo();
        // 仕様通り .r/.g/.b 形式で書き込む（none の場合は .r に "none" を設定）
        let color_opt = if new_none { None } else { Some(new_rgb) };
        let new_text = set_color_in_descript(&descript_text, key, color_opt);
        write_back(app, ctx, cfg_key, use_individual, new_text);
    }
}

/// テキスト入力ウィジェット
/// フォーカス取得時の値を記憶し、Enter押下またはフォーカス喪失時のみプレビュー更新する。
fn show_text_widget(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &egui::Context,
    field: &crate::gui::field_def::FieldDef,
    current_str: &str,
    cfg_key: &str,
    use_individual: bool,
) {
    use crate::gui::state::EditingBuf;

    // 自分のフィールドが編集中ならバッファの値を表示、そうでなければ descript の値
    let is_mine = app.state.editing_buf.as_ref()
        .map(|b| b.field_key == field.key)
        .unwrap_or(false);
    let mut text = if is_mine {
        app.state.editing_buf.as_ref().unwrap().current.clone()
    } else {
        current_str.to_string()
    };

    let resp = egui::TextEdit::singleline(&mut text)
        .lock_focus(true)
        .show(ui)
        .response;

    if resp.gained_focus() {
        app.state.push_undo();
        app.state.editing_buf = Some(EditingBuf {
            field_key: field.key.to_string(),
            committed: current_str.to_string(),
            current:   current_str.to_string(),
        });
    }

    if resp.changed() {
        if let Some(buf) = &mut app.state.editing_buf {
            if buf.field_key == field.key {
                buf.current = text.clone();
            }
        }
    }

    // Enter押下またはフォーカス喪失でコミット
    let commit = resp.lost_focus()
        || ui.input(|i| i.key_pressed(egui::Key::Enter));
    if commit {
        // 自分のバッファのみを取り出す
        let mine = app.state.editing_buf.take_if(|b| b.field_key == field.key);
        let current  = mine.as_ref().map(|b| b.current.as_str()).unwrap_or(&text);
        let committed = mine.as_ref().map(|b| b.committed.as_str()).unwrap_or(current_str);
        if current != committed {
            let descript_text = if use_individual {
                app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default()
            } else {
                app.state.descript_text.clone()
            };
            let new_text = set_descript_value(&descript_text, field.key, current);
            write_back(app, ctx, cfg_key, use_individual, new_text);
        }
    }
}

/// 整数（Spinbox相当）ウィジェット
/// ドラッグ操作はフォーカス喪失時にコミット、Enter押下でもコミット。
fn show_int_widget(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &egui::Context,
    field: &crate::gui::field_def::FieldDef,
    current_str: &str,
    cfg_key: &str,
    use_individual: bool,
) {
    use crate::gui::state::EditingBuf;

    let mut val: i32 = current_str.parse().unwrap_or(0);
    let response = ui.add(egui::DragValue::new(&mut val).speed(1.0));

    if response.gained_focus() {
        app.state.push_undo();
        app.state.editing_buf = Some(EditingBuf {
            field_key: field.key.to_string(),
            committed: current_str.to_string(),
            current:   current_str.to_string(),
        });
    }

    if response.changed() {
        if let Some(buf) = &mut app.state.editing_buf {
            if buf.field_key == field.key {
                buf.current = val.to_string();
            }
        }
    }

    let commit = response.lost_focus()
        || ui.input(|i| i.key_pressed(egui::Key::Enter));
    if commit {
        let mine = app.state.editing_buf.take_if(|b| b.field_key == field.key);
        let val_str = val.to_string();
        let current  = mine.as_ref().map(|b| b.current.as_str()).unwrap_or(&val_str);
        let committed = mine.as_ref().map(|b| b.committed.as_str()).unwrap_or(current_str);
        if current != committed {
            let descript_text = if use_individual {
                app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default()
            } else {
                app.state.descript_text.clone()
            };
            let new_text = set_descript_value(&descript_text, field.key, current);
            write_back(app, ctx, cfg_key, use_individual, new_text);
        }
    }
}

/// ドロップダウンウィジェット
fn show_dropdown_widget(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &egui::Context,
    field: &crate::gui::field_def::FieldDef,
    current_str: &str,
    cfg_key: &str,
    use_individual: bool,
) {
    let mut selected = current_str.to_string();
    let choices = field.choices;

    egui::ComboBox::from_id_salt(field.key)
        .selected_text(selected.clone())
        .show_ui(ui, |ui| {
            for &choice in choices {
                ui.selectable_value(&mut selected, choice.to_string(), choice);
            }
        });

    if selected != current_str {
        app.state.push_undo();
        let descript_text = if use_individual {
            app.state.individual_texts.get(cfg_key).cloned().unwrap_or_default()
        } else {
            app.state.descript_text.clone()
        };
        let new_text = set_descript_value(&descript_text, field.key, &selected);
        write_back(app, ctx, cfg_key, use_individual, new_text);
    }
}

/// descript テキストを書き戻してプレビューを更新する
fn write_back(app: &mut BalloonEditorApp, ctx: &egui::Context, cfg_key: &str, use_individual: bool, new_text: String) {
    if use_individual {
        app.state.individual_texts.insert(cfg_key.to_string(), new_text);
    } else {
        app.state.descript_text = new_text;
    }
    app.refresh_preview_texture(ctx);
}

// --- ヘルパー ---

fn parse_hex_color(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Rgb(r, g, b))
}

fn rgb_to_color32(rgb: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

fn color32_to_rgb(c: egui::Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}

// ---------------------------------------------------------------------------
// 位置編集セクション
// ---------------------------------------------------------------------------

fn show_drag_edit_section(ui: &mut Ui, app: &mut BalloonEditorApp) {
    egui::CollapsingHeader::new("位置編集")
        .default_open(false)
        .show(ui, |ui| {
            // 常時表示のガイド文言
            ui.label(
                egui::RichText::new("プレビュー上でドラッグして位置を調整してください")
                    .color(ui.visuals().text_color())
                    .small(),
            );
            ui.add_space(4.0);

            let is_c = app.state.is_balloonc();
            let sel = app.state.selected_balloon.clone();
            let parts = app.state.parts_cache.get(&sel).cloned().unwrap_or_default();

            let has_sstp = parts.contains_key("sstp_new.png") || parts.contains_key("sstp.png");

            // ---- 全項目を1つのGridにまとめて列幅を統一 ----
            let mut all_entries: Vec<(&str, DragEditTarget)> = Vec::new();
            let area_label = if is_c { "入力域" } else { "テキスト域" };
            let area_target = if is_c { DragEditTarget::CommunicateBox } else { DragEditTarget::ValidRect };
            all_entries.push((area_label, area_target));
            if !is_c {
                all_entries.push(("折り返し X", DragEditTarget::WordWrap));
                if parts.contains_key("arrow0.png")    { all_entries.push(("矢印(上)",     DragEditTarget::Arrow0));       }
                if parts.contains_key("arrow1.png")    { all_entries.push(("矢印(下)",     DragEditTarget::Arrow1));       }
                if parts.contains_key("clickwait.png") { all_entries.push(("クリック待ち",   DragEditTarget::ClickWait));    }
                if has_sstp                             { all_entries.push(("SSTPマーカー",   DragEditTarget::SstpMarker));   }
                if has_sstp                             { all_entries.push(("SSTPメッセージ", DragEditTarget::SstpMessage));  }
                                                          all_entries.push(("カウンタ数値",   DragEditTarget::Counter));
                if parts.contains_key("online0.png")   { all_entries.push(("オンライン",     DragEditTarget::OnlineMarker)); }
            }
            show_drag_entry_list(ui, app, all_entries, &parts);
        });
}

fn show_drag_entry_list(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    entries: Vec<(&str, DragEditTarget)>,
    _parts: &std::collections::HashMap<String, image::RgbaImage>,
) {
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([6.0, 3.0])
        .show(ui, |ui| {
            for (label, target) in entries {
                let is_active = app.state.drag_edit_target.as_ref() == Some(&target);
                ui.label(label);
                if is_active {
                    if ui.button(egui::RichText::new("✏ 編集終了").color(egui::Color32::from_rgb(255, 160, 50))).clicked() {
                        app.state.drag_edit_target = None;
                        app.state.drag_state = None;
                    }
                } else {
                    if ui.button("編集開始").clicked() {
                        app.state.drag_edit_target = Some(target);
                        app.state.drag_state = None;
                    }
                }
                ui.end_row();
            }
        });
}

/// 選択中バルーンが個別設定を持てるかどうかを判定する。
/// balloonc* / balloonk* / balloons* / balloonp*def* を対象（奇数反転生成も含む）。
fn can_have_individual_config(balloon_name: &str) -> bool {
    if balloon_name.is_empty() { return false; }
    balloon_name.starts_with("balloonc")
        || balloon_name.starts_with("balloonk")
        || balloon_name.starts_with("balloons")
        || (balloon_name.starts_with("balloonp") && balloon_name.contains("def"))
}
