use egui::{Context, Ui};
use crate::core::color::Rgb;
use crate::gui::app::BalloonEditorApp;
use crate::gui::state::{LAYER_DEFS, IndivColors};

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    ui.horizontal(|ui| {
        let is_c = app.state.is_balloonc();
        let direct = app.state.direct_image_mode;
        // 画像編集なしモードでは色変更を常に無効扱い
        let no_color = direct || app.state.no_balloon_color;
        let bulk = app.state.bulk_color_mode;


        for &(key, label, _default) in LAYER_DEFS {
            // text レイヤー: balloonc 系のみ表示
            if key == "text" && !is_c {
                continue;
            }
            // 色変更なしモード／画像編集なしモードでは全ピッカーを無効化
            ui.add_enabled_ui(!no_color, |ui| {
                ui.label(format!("{}:", label));

                // individual_colors のキーは .png なしの stem で統一
                let sel_stem = app.state.selected_balloon.trim_end_matches(".png").to_string();

                // ピッカーに表示する色：個別モードなら個別色優先
                let display_color = if !bulk {
                    app.state.individual_colors.get(&sel_stem).and_then(|ic| match key {
                        "base"  => ic.base,
                        "edge"  => ic.edge,
                        "text"  => ic.text,
                        "parts" => ic.parts_colors.get("all").copied(),
                        _       => None,
                    }).unwrap_or_else(|| app.state.layer_colors.get(key).copied().unwrap_or(Rgb(0,0,0)))
                } else {
                    app.state.layer_colors.get(key).copied().unwrap_or(Rgb(0, 0, 0))
                };

                let mut egui_color = rgb_to_egui(display_color);
                let picker_changed = egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut egui_color,
                    egui::color_picker::Alpha::Opaque,
                ).changed();

                // hex テキストボックス
                let hex_id = egui::Id::new("__toolbar_hex__").with(key);
                let committed_hex = format!("#{:02X}{:02X}{:02X}", egui_color.r(), egui_color.g(), egui_color.b());
                let is_editing = ui.memory(|m| m.has_focus(hex_id));
                let mut hex_text = if is_editing {
                    ui.data(|d| d.get_temp::<String>(hex_id)).unwrap_or_else(|| committed_hex.clone())
                } else {
                    committed_hex.clone()
                };
                let hex_resp = ui.add(
                    egui::TextEdit::singleline(&mut hex_text)
                        .id(hex_id)
                        .desired_width(90.0)
                        .font(egui::TextStyle::Monospace),
                );
                if hex_resp.gained_focus() {
                    ui.data_mut(|d| d.insert_temp(hex_id, committed_hex.clone()));
                }
                if hex_resp.changed() {
                    ui.data_mut(|d| d.insert_temp(hex_id, hex_text.clone()));
                }
                let hex_committed = if hex_resp.lost_focus() {
                    let s = hex_text.trim().trim_start_matches('#');
                    if s.len() == 6 {
                        if let (Ok(r), Ok(g), Ok(b)) = (
                            u8::from_str_radix(&s[0..2], 16),
                            u8::from_str_radix(&s[2..4], 16),
                            u8::from_str_radix(&s[4..6], 16),
                        ) {
                            egui_color = egui::Color32::from_rgb(r, g, b);
                            ui.data_mut(|d| d.remove::<String>(hex_id));
                            true
                        } else { false }
                    } else { false }
                } else { false };

                if picker_changed || hex_committed {
                    let new_rgb = egui_to_rgb(egui_color);
                    app.state.push_undo();
                    if bulk {
                        app.state.layer_colors.insert(key.to_string(), new_rgb);
                        if key == "parts" {
                            app.state.parts_colors.insert("all".to_string(), new_rgb);
                        }
                        app.rebuild_and_refresh(ctx);
                    } else {
                        let ic = app.state.individual_colors.entry(sel_stem).or_insert_with(IndivColors::default);
                        match key {
                            "base"  => ic.base  = Some(new_rgb),
                            "edge"  => ic.edge  = Some(new_rgb),
                            "text"  => ic.text  = Some(new_rgb),
                            "parts" => { ic.parts_colors.insert("all".to_string(), new_rgb); }
                            _       => {}
                        }
                        app.rebuild_selected_and_refresh(ctx);
                    }
                }
                ui.separator();
            });
        }

        // 「色を一括適用」チェックボックス（バルーン画像色を変更しない時はグレーアウト）
        ui.add_enabled_ui(!no_color, |ui| {
            let mut bulk = app.state.bulk_color_mode;
            if ui.checkbox(&mut bulk, "色を一括適用").changed() {
                app.state.bulk_color_mode = bulk;
            }
        });

    });
}

pub fn rgb_to_egui(c: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(c.0, c.1, c.2)
}

pub fn egui_to_rgb(c: egui::Color32) -> Rgb {
    Rgb(c.r(), c.g(), c.b())
}
