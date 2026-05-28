use egui::{Context, Ui, Vec2};
use crate::gui::app::BalloonEditorApp;
use crate::gui::state::{CanvasBg, DragEditTarget, DragState, RectEdge};
use crate::core::descript::{parse_descript, set_descript_value, pos_str, get_color_from_descript};
use crate::core::color::{contrast_ratio, wcag_level, apca_lc, apca_bronze, Rgb};
use crate::core::composer::sample_center_color;
use image::RgbaImage;

/// field_def の ACCORDION_GROUPS からキー→デフォルト値のマップを構築する
fn field_defaults() -> std::collections::HashMap<&'static str, &'static str> {
    use crate::gui::field_def::ACCORDION_GROUPS;
    ACCORDION_GROUPS.iter()
        .flat_map(|g| g.fields.iter())
        .map(|f| (f.key, f.default))
        .collect()
}

pub fn show(ui: &mut Ui, app: &mut BalloonEditorApp, ctx: &Context) {
    // ヘッダー行：「プレビュー」ラベル ＋ コントラスト比
    ui.horizontal(|ui| {
        ui.strong("プレビュー");
        if let Some(contrast_text) = calc_contrast_label(app) {
            ui.separator();
            ui.label(contrast_text);
        }
    });
    ui.separator();

    // プレビューテクスチャが未作成なら生成
    if app.preview_texture.is_none() {
        app.refresh_preview_texture(ctx);
    }

    let available = ui.available_size();

    let texture_info = app.preview_texture.as_ref().map(|t| (t.id(), t.size_vec2()));

    match texture_info {
        Some((tex_id, img_size)) => {
            let canvas_rect = ui.available_rect_before_wrap();
            let offset_x = (canvas_rect.width()  - img_size.x).max(0.0) / 2.0;
            let offset_y = (canvas_rect.height() - img_size.y).max(0.0) / 2.0;

            let img_rect = egui::Rect::from_min_size(
                canvas_rect.min + Vec2::new(offset_x, offset_y),
                img_size,
            );

            let painter = ui.painter();

            // canvas_bg に応じた背景を描画
            match &app.state.canvas_bg {
                CanvasBg::Checker => draw_checker(painter, canvas_rect),
                CanvasBg::Solid(r, g, b) => {
                    painter.rect_filled(canvas_rect, 0.0, egui::Color32::from_rgb(*r, *g, *b));
                }
            }

            // バルーン画像を描画
            painter.image(
                tex_id,
                img_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            // ドラッグ位置編集モードの処理
            if app.state.drag_edit_target.is_some() {
                let iw = img_size.x as i32;
                let ih = img_size.y as i32;
                // parts_cache から選択バルーンのパーツ画像を取得
                let sel = app.state.selected_balloon.clone();
                let parts_snap: std::collections::HashMap<String, RgbaImage> =
                    app.state.parts_cache.get(&sel).cloned().unwrap_or_default();
                handle_drag_edit(ui, app, ctx, img_rect, iw, ih, &parts_snap);
            }
        }
        None => {
            ui.allocate_space(available);
        }
    }

    // プレビュー生成中オーバーレイ
    if app.state.preview_generating {
        let rect = ui.ctx().screen_rect();
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("preview_generating_overlay"),
        ));
        let label = "プレビュー生成中";
        let font_id = egui::FontId::proportional(16.0);
        let galley = ui.ctx().fonts(|f| {
            f.layout_no_wrap(label.to_string(), font_id.clone(), egui::Color32::WHITE)
        });
        let text_size = galley.size();
        let pad = egui::vec2(12.0, 6.0);
        let bg_size = text_size + pad * 2.0;
        let bg_pos = egui::pos2(
            rect.center().x - bg_size.x / 2.0,
            rect.center().y - bg_size.y / 2.0,
        );
        let bg_rect = egui::Rect::from_min_size(bg_pos, bg_size);
        painter.rect_filled(bg_rect, 6.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180));
        painter.galley(bg_pos + pad, galley, egui::Color32::WHITE);
    }
}

// ---------------------------------------------------------------------------
// ドラッグ位置編集
// ---------------------------------------------------------------------------

fn handle_drag_edit(
    ui: &mut Ui,
    app: &mut BalloonEditorApp,
    ctx: &Context,
    img_rect: egui::Rect,
    iw: i32,
    ih: i32,
    parts: &std::collections::HashMap<String, RgbaImage>,
) {
    let target = match &app.state.drag_edit_target {
        Some(t) => t.clone(),
        None => return,
    };

    // descript テキストを取得
    let descript_text = app.state.descript_text.clone();
    let parsed = parse_descript(&descript_text);

    // field_def のデフォルト値を dynamic_defaults にマージ（parsed になければ dynamic_defaults、
    // それもなければ field_def デフォルト、という優先順位にする）
    let fd = field_defaults();
    let mut merged_defaults = app.state.dynamic_defaults.clone();
    for (k, v) in &fd {
        merged_defaults.entry(k.to_string()).or_insert_with(|| v.to_string());
    }

    // ターゲットの現在値（画像座標系）と操作軸を取得
    let info = target_info(&target, &parsed, &merged_defaults, iw, ih);

    // インタラクション領域（img_rect全体にsense_drag）
    let interact = ui.interact(
        img_rect,
        egui::Id::new("drag_edit_area"),
        egui::Sense::drag(),
    );

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());

    // ValidRect/CommunicateBox: ホバー中の辺を検出してカーソルを変える
    let hovered_edge = if matches!(target, DragEditTarget::ValidRect | DragEditTarget::CommunicateBox) {
        pointer_pos.and_then(|pos| {
            let ip = pos - img_rect.min;
            detect_edge(&target, &parsed, &merged_defaults, iw, ih,
                        egui::pos2(ip.x, ip.y), 8.0)
        })
    } else {
        None
    };

    // カーソル設定: プレビュー領域内にいる場合、またはドラッグ中のみ変更する
    let is_in_img = pointer_pos.map(|p| img_rect.contains(p)).unwrap_or(false);
    let is_dragging = app.state.drag_state.is_some();
    if is_in_img || is_dragging {
        let cursor = match &target {
            DragEditTarget::ValidRect | DragEditTarget::CommunicateBox => {
                let edge = app.state.drag_state.as_ref()
                    .and_then(|ds| ds.active_edge)
                    .or(hovered_edge);
                match edge {
                    Some(RectEdge::Top) | Some(RectEdge::Bottom) => egui::CursorIcon::ResizeVertical,
                    Some(RectEdge::Left) | Some(RectEdge::Right)  => egui::CursorIcon::ResizeHorizontal,
                    None => egui::CursorIcon::Default,
                }
            }
            _ => egui::CursorIcon::Crosshair,
        };
        ui.ctx().set_cursor_icon(cursor);
    }

    // ドラッグ開始
    if interact.drag_started() {
        if let Some(pos) = pointer_pos {
            let img_pos = pos - img_rect.min;
            let start_img = egui::pos2(img_pos.x, img_pos.y);

            // ValidRect/CommunicateBox: 辺を検出して start_val をその辺の現在値にセット
            // start_img も辺の座標に揃えることで、ドラッグ開始直後の dx=0 が
            // 正しく「辺の現在座標」を指すようにする
            let (start_val, start_img, active_edge) = if matches!(target,
                DragEditTarget::ValidRect | DragEditTarget::CommunicateBox)
            {
                let edge = detect_edge(&target, &parsed, &merged_defaults,
                                       iw, ih, start_img, 8.0);
                let val = match edge {
                    Some(e @ (RectEdge::Left | RectEdge::Right)) => {
                        let v = if matches!(target, DragEditTarget::CommunicateBox) {
                            cb_edge_current_val(e, &parsed, ih)
                        } else {
                            edge_current_val(e, &parsed, &merged_defaults, iw, ih)
                        };
                        v
                    }
                    Some(e @ (RectEdge::Top | RectEdge::Bottom)) => {
                        let v = if matches!(target, DragEditTarget::CommunicateBox) {
                            cb_edge_current_val(e, &parsed, ih)
                        } else {
                            edge_current_val(e, &parsed, &merged_defaults, iw, ih)
                        };
                        v
                    }
                    None => (0, 0),
                };
                // start_img を辺の座標に合わせる（x軸辺ならx成分、y軸辺ならy成分を揃える）
                let aligned_start_img = match edge {
                    Some(RectEdge::Left) | Some(RectEdge::Right)
                        => egui::pos2(val.0 as f32, start_img.y),
                    Some(RectEdge::Top) | Some(RectEdge::Bottom)
                        => egui::pos2(start_img.x, val.1 as f32),
                    None => start_img,
                };
                (val, aligned_start_img, edge)
            } else {
                (info.current_val, start_img, None)
            };

            // 辺が検出できなかった場合はドラッグ開始しない
            if matches!(target, DragEditTarget::ValidRect | DragEditTarget::CommunicateBox)
                && active_edge.is_none()
            {
                // no-op
            } else {
                let was_negative = was_negative_in_descript(&target, &parsed, &merged_defaults, active_edge);
                app.state.push_undo();
                app.state.drag_state = Some(DragState {
                    start_img,
                    start_val,
                    current_val: start_val,
                    active_edge,
                    was_negative,
                });
            }
        }
    }

    // ドラッグ中
    if interact.dragged() {
        if let (Some(pos), Some(ds)) = (pointer_pos, &mut app.state.drag_state) {
            let img_pos = pos - img_rect.min;
            let dx = (img_pos.x - ds.start_img.x) as i32;
            let dy = (img_pos.y - ds.start_img.y) as i32;
            ds.current_val = compute_new_val_with_edge(&target, ds.active_edge, ds.start_val, dx, dy);
        }
    }

    // ドラッグ終了 → クランプ補正してから descript に書き込みプレビュー再生成
    if interact.drag_stopped() {
        if let Some(ds) = app.state.drag_state.take() {
            let clamped = clamp_val(&target, ds.current_val, ds.active_edge,
                                    &descript_text, iw, ih, parts);
            let new_text = write_val_to_descript(&target, &descript_text, clamped,
                                                  ds.active_edge, ds.was_negative, iw, ih);
            app.state.descript_text = new_text;
            app.refresh_preview_texture(ctx);
        }
    }

    // オーバーレイ描画
    let draw_state = app.state.drag_state.as_ref().map(|ds| (ds.current_val, ds.active_edge));
    draw_drag_overlay(ui, ctx, &target, &parsed, &merged_defaults,
                      img_rect, draw_state, iw, ih, parts);
}

// ---------------------------------------------------------------------------
// ターゲット情報（現在値・操作軸）
// ---------------------------------------------------------------------------

struct TargetInfo {
    /// 現在値 (x, y) — 画像座標系（正値）
    current_val: (i32, i32),
}

fn get_descript_val(
    key: &str,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    iw: i32,
    ih: i32,
) -> i32 {
    let raw = parsed.get(key)
        .or_else(|| defaults.get(key))
        .map(|s| s.as_str())
        .unwrap_or("0");
    let is_x_axis = key.ends_with(".x") || key.ends_with(".xr")
        || key.ends_with(".left") || key.ends_with(".right");
    pos_str(raw, if is_x_axis { iw } else { ih })
}

/// ValidRect/CommunicateBox の各辺の現在座標を返す（画像座標系）
fn edge_current_val(
    edge: RectEdge,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    iw: i32,
    ih: i32,
) -> (i32, i32) {
    let g = |k: &str| get_descript_val(k, parsed, defaults, iw, ih);
    match edge {
        RectEdge::Top    => (0, g("validrect.top")),
        RectEdge::Bottom => (0, g("validrect.bottom")),
        RectEdge::Left   => (g("validrect.left"), 0),
        RectEdge::Right  => (g("validrect.right"), 0),
    }
}

fn cb_edge_current_val(
    edge: RectEdge,
    parsed: &std::collections::HashMap<String, String>,
    ih: i32,
) -> (i32, i32) {
    let cb_x: i32 = parsed.get("communicatebox.x").and_then(|s| s.parse().ok()).unwrap_or(10);
    let cb_y: i32 = pos_str(parsed.get("communicatebox.y").map(|s| s.as_str()).unwrap_or("10"), ih);
    let cb_w: i32 = parsed.get("communicatebox.width").and_then(|s| s.parse().ok()).unwrap_or(100);
    let cb_h: i32 = parsed.get("communicatebox.height").and_then(|s| s.parse().ok()).unwrap_or(30);
    match edge {
        RectEdge::Left   => (cb_x, 0),
        RectEdge::Top    => (0, cb_y),
        RectEdge::Right  => (cb_x + cb_w, 0),
        RectEdge::Bottom => (0, cb_y + cb_h),
    }
}

/// マウス位置から最も近い辺を検出する（threshold px 以内）
fn detect_edge(
    target: &DragEditTarget,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    iw: i32,
    ih: i32,
    img_pos: egui::Pos2,
    threshold: f32,
) -> Option<RectEdge> {
    let mx = img_pos.x;
    let my = img_pos.y;

    let candidates: Vec<(RectEdge, f32)> = match target {
        DragEditTarget::ValidRect => {
            let g = |k: &str| get_descript_val(k, parsed, defaults, iw, ih) as f32;
            vec![
                (RectEdge::Top,    (my - g("validrect.top")).abs()),
                (RectEdge::Bottom, (my - g("validrect.bottom")).abs()),
                (RectEdge::Left,   (mx - g("validrect.left")).abs()),
                (RectEdge::Right,  (mx - g("validrect.right")).abs()),
            ]
        }
        DragEditTarget::CommunicateBox => {
            let cb_x = parsed.get("communicatebox.x").and_then(|s| s.parse::<i32>().ok()).unwrap_or(10) as f32;
            let cb_y = pos_str(parsed.get("communicatebox.y").map(|s| s.as_str()).unwrap_or("10"), ih) as f32;
            let cb_w = parsed.get("communicatebox.width").and_then(|s| s.parse::<i32>().ok()).unwrap_or(100) as f32;
            let cb_h = parsed.get("communicatebox.height").and_then(|s| s.parse::<i32>().ok()).unwrap_or(30) as f32;
            vec![
                (RectEdge::Left,   (mx - cb_x).abs()),
                (RectEdge::Top,    (my - cb_y).abs()),
                (RectEdge::Right,  (mx - (cb_x + cb_w)).abs()),
                (RectEdge::Bottom, (my - (cb_y + cb_h)).abs()),
            ]
        }
        _ => return None,
    };

    candidates.into_iter()
        .filter(|(_, d)| *d <= threshold)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(e, _)| e)
}

fn target_info(
    target: &DragEditTarget,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    iw: i32,
    ih: i32,
) -> TargetInfo {
    let g = |k: &str| get_descript_val(k, parsed, defaults, iw, ih);
    let get_xy = |xk: &str, yk: &str| -> (i32, i32) { (g(xk), g(yk)) };

    let val = match target {
        DragEditTarget::Arrow0       => get_xy("arrow0.x", "arrow0.y"),
        DragEditTarget::Arrow1       => get_xy("arrow1.x", "arrow1.y"),
        DragEditTarget::ClickWait    => get_xy("clickwaitmarker.x", "clickwaitmarker.y"),
        DragEditTarget::SstpMarker   => get_xy("sstpmarker.x", "sstpmarker.y"),
        DragEditTarget::SstpMessage  => get_xy("sstpmessage.x", "sstpmessage.y"),
        DragEditTarget::OnlineMarker => get_xy("onlinemarker.x", "onlinemarker.y"),
        DragEditTarget::Counter      => {
            let xr: i32 = parsed.get("number.xr").and_then(|s| s.parse().ok()).unwrap_or(-20);
            (iw + xr, g("number.y"))
        }
        // ValidRect/CommunicateBox は辺ごとに値が違うので (0,0) を返す（draw_drag_overlay で個別に読む）
        DragEditTarget::ValidRect | DragEditTarget::CommunicateBox => (0, 0),
        DragEditTarget::WordWrap => (g("wordwrappoint.x"), 0),
    };
    TargetInfo { current_val: val }
}

// ---------------------------------------------------------------------------
// ドラッグ後の新しい値を計算
// ---------------------------------------------------------------------------

fn compute_new_val_with_edge(
    target: &DragEditTarget,
    edge: Option<RectEdge>,
    start: (i32, i32),
    dx: i32,
    dy: i32,
) -> (i32, i32) {
    match target {
        // 自由ドラッグ
        DragEditTarget::Arrow0
        | DragEditTarget::Arrow1
        | DragEditTarget::ClickWait
        | DragEditTarget::SstpMarker
        | DragEditTarget::SstpMessage
        | DragEditTarget::OnlineMarker
        | DragEditTarget::Counter => (start.0 + dx, start.1 + dy),
        // x のみ
        DragEditTarget::WordWrap => (start.0 + dx, start.1),
        // ValidRect/CommunicateBox は辺に応じて軸を決定
        DragEditTarget::ValidRect | DragEditTarget::CommunicateBox => {
            match edge {
                Some(RectEdge::Left) | Some(RectEdge::Right)  => (start.0 + dx, start.1),
                Some(RectEdge::Top)  | Some(RectEdge::Bottom) => (start.0, start.1 + dy),
                None => start,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// クランプ補正（ドラッグ終了時）
// ---------------------------------------------------------------------------

/// ドラッグ終了時に値を画像範囲内にクランプする。
/// パーツ画像がある場合はパーツが完全に収まる範囲（右端 = iw - パーツ幅）にクランプする。
fn clamp_val(
    target: &DragEditTarget,
    val: (i32, i32),
    edge: Option<RectEdge>,
    descript_text: &str,
    iw: i32,
    ih: i32,
    parts: &std::collections::HashMap<String, RgbaImage>,
) -> (i32, i32) {
    let (vx, vy) = val;

    match target {
        // 自由ドラッグ系: パーツが完全に収まる範囲にクランプ
        DragEditTarget::Arrow0 => {
            let (pw, ph) = parts.get("arrow0.png").map(|p| (p.width() as i32, p.height() as i32)).unwrap_or((0, 0));
            (vx.clamp(0, (iw - pw).max(0)), vy.clamp(0, (ih - ph).max(0)))
        }
        DragEditTarget::Arrow1 => {
            let (pw, ph) = parts.get("arrow1.png").map(|p| (p.width() as i32, p.height() as i32)).unwrap_or((0, 0));
            (vx.clamp(0, (iw - pw).max(0)), vy.clamp(0, (ih - ph).max(0)))
        }
        DragEditTarget::ClickWait => {
            let (pw, ph) = parts.get("clickwait.png").map(|p| (p.width() as i32, p.height() as i32)).unwrap_or((0, 0));
            (vx.clamp(0, (iw - pw).max(0)), vy.clamp(0, (ih - ph).max(0)))
        }
        DragEditTarget::SstpMarker => {
            let img = parts.get("sstp_new.png").or_else(|| parts.get("sstp.png"));
            let (pw, ph) = img.map(|p| (p.width() as i32, p.height() as i32)).unwrap_or((0, 0));
            (vx.clamp(0, (iw - pw).max(0)), vy.clamp(0, (ih - ph).max(0)))
        }
        DragEditTarget::OnlineMarker => {
            let (pw, ph) = parts.get("online0.png").map(|p| (p.width() as i32, p.height() as i32)).unwrap_or((0, 0));
            (vx.clamp(0, (iw - pw).max(0)), vy.clamp(0, (ih - ph).max(0)))
        }
        // テキスト系: 開始座標のみ画像内に収める（テキスト幅は可変なので左上座標だけクランプ）
        DragEditTarget::SstpMessage => (vx.clamp(0, iw), vy.clamp(0, ih)),
        DragEditTarget::Counter => (vx.clamp(0, iw), vy.clamp(0, ih)),
        // WordWrap: x のみ [0, iw]
        DragEditTarget::WordWrap => (vx.clamp(0, iw), vy),
        // ValidRect: 各辺を [0, iw/ih] に収め、対辺との間隔を最低 MIN_GAP px 確保する
        DragEditTarget::ValidRect => {
            const MIN_GAP: i32 = 10;
            let parsed = parse_descript(descript_text);
            let fd = field_defaults();
            let mut def = std::collections::HashMap::new();
            for (k, v) in &fd { def.insert(k.to_string(), v.to_string()); }
            let g = |k: &str| get_descript_val(k, &parsed, &def, iw, ih);
            match edge {
                Some(RectEdge::Left) => {
                    let right = g("validrect.right");
                    (vx.clamp(0, (right - MIN_GAP).max(0)), vy)
                }
                Some(RectEdge::Right) => {
                    let left = g("validrect.left");
                    (vx.clamp(left + MIN_GAP, iw), vy)
                }
                Some(RectEdge::Top) => {
                    let bottom = g("validrect.bottom");
                    (vx, vy.clamp(0, (bottom - MIN_GAP).max(0)))
                }
                Some(RectEdge::Bottom) => {
                    let top = g("validrect.top");
                    (vx, vy.clamp(top + MIN_GAP, ih))
                }
                None => val,
            }
        }
        // CommunicateBox: 各辺を [0, iw/ih] に収め、対辺との間隔を最低 MIN_GAP px 確保する
        DragEditTarget::CommunicateBox => {
            const MIN_GAP: i32 = 10;
            let parsed = parse_descript(descript_text);
            let cb_x: i32 = parsed.get("communicatebox.x").and_then(|s| s.parse().ok()).unwrap_or(10);
            let cb_y = pos_str(parsed.get("communicatebox.y").map(|s| s.as_str()).unwrap_or("10"), ih);
            let cb_w: i32 = parsed.get("communicatebox.width").and_then(|s| s.parse().ok()).unwrap_or(100);
            let cb_h: i32 = parsed.get("communicatebox.height").and_then(|s| s.parse().ok()).unwrap_or(30);
            match edge {
                Some(RectEdge::Left) => {
                    let right = cb_x + cb_w;
                    (vx.clamp(0, (right - MIN_GAP).max(0)), vy)
                }
                Some(RectEdge::Right) => {
                    (vx.clamp(cb_x + MIN_GAP, iw), vy)
                }
                Some(RectEdge::Top) => {
                    let bottom = cb_y + cb_h;
                    (vx, vy.clamp(0, (bottom - MIN_GAP).max(0)))
                }
                Some(RectEdge::Bottom) => {
                    (vx, vy.clamp(cb_y + MIN_GAP, ih))
                }
                None => val,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// descript への書き戻し
// ---------------------------------------------------------------------------

/// img座標の値を descript 書き込み用の値に変換する。
/// was_neg が true の場合は画像サイズからの負値オフセットとして書き込む。
fn to_descript_x(img_x: i32, iw: i32, was_neg: bool) -> String {
    if was_neg { (img_x - iw).to_string() } else { img_x.to_string() }
}
fn to_descript_y(img_y: i32, ih: i32, was_neg: bool) -> String {
    if was_neg { (img_y - ih).to_string() } else { img_y.to_string() }
}

/// ドラッグ開始前の descript 生値が負値だったかを返す (x軸, y軸)
/// ValidRect/CommunicateBox の場合は active_edge の軸のみ意味を持つ
fn was_negative_in_descript(
    target: &DragEditTarget,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    active_edge: Option<RectEdge>,
) -> (bool, bool) {
    let raw = |key: &str| -> bool {
        parsed.get(key)
            .or_else(|| defaults.get(key))
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map(|v| v < 0)
            .unwrap_or(false)
    };
    match target {
        DragEditTarget::Arrow0       => (raw("arrow0.x"),         raw("arrow0.y")),
        DragEditTarget::Arrow1       => (raw("arrow1.x"),         raw("arrow1.y")),
        DragEditTarget::ClickWait    => (raw("clickwaitmarker.x"), raw("clickwaitmarker.y")),
        DragEditTarget::SstpMarker   => (raw("sstpmarker.x"),     raw("sstpmarker.y")),
        DragEditTarget::SstpMessage  => (raw("sstpmessage.x"),    raw("sstpmessage.y")),
        DragEditTarget::OnlineMarker => (raw("onlinemarker.x"),   raw("onlinemarker.y")),
        DragEditTarget::Counter      => (false, raw("number.y")), // xr は常に iw からの相対値
        DragEditTarget::WordWrap     => (raw("wordwrappoint.x"),  false),
        DragEditTarget::ValidRect => match active_edge {
            Some(RectEdge::Left)   => (raw("validrect.left"),   false),
            Some(RectEdge::Right)  => (raw("validrect.right"),  false),
            Some(RectEdge::Top)    => (false, raw("validrect.top")),
            Some(RectEdge::Bottom) => (false, raw("validrect.bottom")),
            None => (false, false),
        },
        DragEditTarget::CommunicateBox => (false, false), // 絶対値指定のため対象外
    }
}

fn write_val_to_descript(
    target: &DragEditTarget,
    descript_text: &str,
    val: (i32, i32),
    edge: Option<RectEdge>,
    was_negative: (bool, bool),
    iw: i32,
    ih: i32,
) -> String {
    let mut text = descript_text.to_string();
    let (neg_x, neg_y) = was_negative;
    let vx = to_descript_x(val.0, iw, neg_x);
    let vy = to_descript_y(val.1, ih, neg_y);

    match target {
        DragEditTarget::Arrow0 => {
            text = set_descript_value(&text, "arrow0.x", &vx);
            text = set_descript_value(&text, "arrow0.y", &vy);
        }
        DragEditTarget::Arrow1 => {
            text = set_descript_value(&text, "arrow1.x", &vx);
            text = set_descript_value(&text, "arrow1.y", &vy);
        }
        DragEditTarget::ClickWait => {
            text = set_descript_value(&text, "clickwaitmarker.x", &vx);
            text = set_descript_value(&text, "clickwaitmarker.y", &vy);
        }
        DragEditTarget::SstpMarker => {
            text = set_descript_value(&text, "sstpmarker.x", &vx);
            text = set_descript_value(&text, "sstpmarker.y", &vy);
        }
        DragEditTarget::SstpMessage => {
            text = set_descript_value(&text, "sstpmessage.x", &vx);
            text = set_descript_value(&text, "sstpmessage.y", &vy);
        }
        DragEditTarget::OnlineMarker => {
            text = set_descript_value(&text, "onlinemarker.x", &vx);
            text = set_descript_value(&text, "onlinemarker.y", &vy);
        }
        DragEditTarget::Counter => {
            text = set_descript_value(&text, "number.xr", &(val.0 - iw).to_string());
            text = set_descript_value(&text, "number.y", &vy);
        }
        DragEditTarget::WordWrap => {
            text = set_descript_value(&text, "wordwrappoint.x", &vx);
        }
        DragEditTarget::ValidRect => {
            match edge {
                Some(RectEdge::Top)    => { text = set_descript_value(&text, "validrect.top",    &vy); }
                Some(RectEdge::Bottom) => { text = set_descript_value(&text, "validrect.bottom", &vy); }
                Some(RectEdge::Left)   => { text = set_descript_value(&text, "validrect.left",   &vx); }
                Some(RectEdge::Right)  => { text = set_descript_value(&text, "validrect.right",  &vx); }
                None => {}
            }
        }
        DragEditTarget::CommunicateBox => {
            let p = parse_descript(&text);
            let cb_x: i32 = p.get("communicatebox.x").and_then(|s| s.parse().ok()).unwrap_or(10);
            let cb_y_raw = p.get("communicatebox.y").cloned().unwrap_or_else(|| "10".to_string());
            let cb_y = pos_str(&cb_y_raw, ih);
            let cb_w: i32 = p.get("communicatebox.width").and_then(|s| s.parse().ok()).unwrap_or(100);
            let cb_h: i32 = p.get("communicatebox.height").and_then(|s| s.parse().ok()).unwrap_or(30);
            match edge {
                Some(RectEdge::Left) => {
                    let new_w = (cb_x + cb_w - val.0).max(1);
                    text = set_descript_value(&text, "communicatebox.x", &val.0.to_string());
                    text = set_descript_value(&text, "communicatebox.width", &new_w.to_string());
                }
                Some(RectEdge::Top) => {
                    let new_h = (cb_y + cb_h - val.1).max(1);
                    text = set_descript_value(&text, "communicatebox.y", &val.1.to_string());
                    text = set_descript_value(&text, "communicatebox.height", &new_h.to_string());
                }
                Some(RectEdge::Right) => {
                    let new_w = (val.0 - cb_x).max(1);
                    text = set_descript_value(&text, "communicatebox.width", &new_w.to_string());
                }
                Some(RectEdge::Bottom) => {
                    let new_h = (val.1 - cb_y).max(1);
                    text = set_descript_value(&text, "communicatebox.height", &new_h.to_string());
                }
                None => {}
            }
        }
    }
    text
}

// ---------------------------------------------------------------------------
// オーバーレイ描画（ドラッグ中のガイドライン）
// ---------------------------------------------------------------------------

fn draw_drag_overlay(
    ui: &mut Ui,
    ctx: &Context,
    target: &DragEditTarget,
    parsed: &std::collections::HashMap<String, String>,
    defaults: &std::collections::HashMap<String, String>,
    img_rect: egui::Rect,
    drag_state: Option<((i32, i32), Option<RectEdge>)>,
    iw: i32,
    ih: i32,
    parts: &std::collections::HashMap<String, RgbaImage>,
) {
    let painter = ui.painter();
    let origin = img_rect.min;
    let col_outline = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180);
    let col_line    = egui::Color32::from_rgba_unmultiplied(255, 220, 0, 230);
    let col_active  = egui::Color32::from_rgba_unmultiplied(255, 80,  80, 230);
    let col_passive = egui::Color32::from_rgba_unmultiplied(255, 220, 0, 110);

    let stroke_out     = egui::Stroke::new(3.5, col_outline);
    let stroke_active  = egui::Stroke::new(2.0, col_active);
    let stroke_passive = egui::Stroke::new(1.5, col_passive);
    let stroke_line    = egui::Stroke::new(1.5, col_line);

    let draw_hline_s = |painter: &egui::Painter, y: f32, s: egui::Stroke| {
        let l = egui::pos2(origin.x, y);
        let r = egui::pos2(origin.x + iw as f32, y);
        painter.line_segment([l, r], stroke_out);
        painter.line_segment([l, r], s);
    };
    let draw_vline_s = |painter: &egui::Painter, x: f32, s: egui::Stroke| {
        let t = egui::pos2(x, origin.y);
        let b = egui::pos2(x, origin.y + ih as f32);
        painter.line_segment([t, b], stroke_out);
        painter.line_segment([t, b], s);
    };
    let draw_handle = |painter: &egui::Painter, pos: egui::Pos2, active: bool| {
        painter.circle_filled(pos, 6.0, col_outline);
        painter.circle_filled(pos, 4.0, if active { col_active } else { col_passive });
    };
    let draw_cross = |painter: &egui::Painter, x: f32, y: f32| {
        let r = 7.0;
        painter.line_segment([egui::pos2(x-r, y), egui::pos2(x+r, y)], stroke_out);
        painter.line_segment([egui::pos2(x, y-r), egui::pos2(x, y+r)], stroke_out);
        painter.line_segment([egui::pos2(x-r, y), egui::pos2(x+r, y)], stroke_line);
        painter.line_segment([egui::pos2(x, y-r), egui::pos2(x, y+r)], stroke_line);
    };

    let px = |x: i32| origin.x + x as f32;
    let py = |y: i32| origin.y + y as f32;
    let mid_x = origin.x + iw as f32 / 2.0;
    let mid_y = origin.y + ih as f32 / 2.0;
    let g = |k: &str| get_descript_val(k, parsed, defaults, iw, ih);

    // 操作中の辺（ドラッグ中 or ホバー）
    let active_edge = drag_state.and_then(|(_, e)| e);
    let current_val = drag_state.map(|(v, _)| v);

    let mut label_text = String::new();

    match target {
        // 自由ドラッグ系
        DragEditTarget::Arrow0 | DragEditTarget::Arrow1
        | DragEditTarget::ClickWait | DragEditTarget::SstpMarker
        | DragEditTarget::SstpMessage | DragEditTarget::OnlineMarker
        | DragEditTarget::Counter => {
            let (vx, vy) = current_val.unwrap_or_else(|| {
                // drag前: target_infoと同等の値を直接計算
                match target {
                    DragEditTarget::Arrow0       => (g("arrow0.x"), g("arrow0.y")),
                    DragEditTarget::Arrow1       => (g("arrow1.x"), g("arrow1.y")),
                    DragEditTarget::ClickWait    => (g("clickwaitmarker.x"), g("clickwaitmarker.y")),
                    DragEditTarget::SstpMarker   => (g("sstpmarker.x"), g("sstpmarker.y")),
                    DragEditTarget::SstpMessage  => (g("sstpmessage.x"), g("sstpmessage.y")),
                    DragEditTarget::OnlineMarker => (g("onlinemarker.x"), g("onlinemarker.y")),
                    DragEditTarget::Counter      => {
                        let xr: i32 = parsed.get("number.xr").and_then(|s| s.parse().ok()).unwrap_or(-20);
                        (iw + xr, g("number.y"))
                    }
                    _ => (0, 0),
                }
            });
            let x = px(vx);
            let y = py(vy);
            draw_hline_s(&painter, y, stroke_line);
            draw_vline_s(&painter, x, stroke_line);
            let img_key = match target {
                DragEditTarget::Arrow0       => Some("arrow0.png"),
                DragEditTarget::Arrow1       => Some("arrow1.png"),
                DragEditTarget::ClickWait    => Some("clickwait.png"),
                DragEditTarget::OnlineMarker => Some("online0.png"),
                _ => None,
            };
            let sstp_img = if matches!(target, DragEditTarget::SstpMarker) {
                parts.get("sstp_new.png").or_else(|| parts.get("sstp.png"))
            } else { None };
            let overlay_img = img_key.and_then(|k| parts.get(k)).or(sstp_img);
            if let Some(rgba) = overlay_img {
                paste_rgba_overlay(ctx, &painter, rgba, egui::pos2(x, y));
            } else {
                draw_cross(&painter, x, y);
            }
            label_text = format!("x:{} y:{}", vx, vy);
        }

        // WordWrap — 縦線1本
        DragEditTarget::WordWrap => {
            let vx = current_val.map(|(x, _)| x).unwrap_or_else(|| g("wordwrappoint.x"));
            let x = px(vx);
            draw_vline_s(&painter, x, stroke_active);
            draw_handle(&painter, egui::pos2(x, mid_y), true);
            label_text = format!("x: {}", vx);
        }

        // ValidRect — 4辺を表示、操作中の辺を強調
        DragEditTarget::ValidRect => {
            let edges = [RectEdge::Top, RectEdge::Bottom, RectEdge::Left, RectEdge::Right];
            for edge in edges {
                let is_active = active_edge == Some(edge);
                let val = if is_active {
                    current_val.unwrap_or_else(|| edge_current_val(edge, parsed, defaults, iw, ih))
                } else {
                    edge_current_val(edge, parsed, defaults, iw, ih)
                };
                let s = if is_active { stroke_active } else { stroke_passive };
                match edge {
                    RectEdge::Top | RectEdge::Bottom => {
                        let y = py(val.1);
                        draw_hline_s(&painter, y, s);
                        draw_handle(&painter, egui::pos2(mid_x, y), is_active);
                        if is_active { label_text = format!("y: {}", val.1); }
                    }
                    RectEdge::Left | RectEdge::Right => {
                        let x = px(val.0);
                        draw_vline_s(&painter, x, s);
                        draw_handle(&painter, egui::pos2(x, mid_y), is_active);
                        if is_active { label_text = format!("x: {}", val.0); }
                    }
                }
            }
        }

        // CommunicateBox — 4辺を表示、操作中の辺を強調
        DragEditTarget::CommunicateBox => {
            let edges = [RectEdge::Top, RectEdge::Bottom, RectEdge::Left, RectEdge::Right];
            for edge in edges {
                let is_active = active_edge == Some(edge);
                let val = if is_active {
                    current_val.unwrap_or_else(|| cb_edge_current_val(edge, parsed, ih))
                } else {
                    cb_edge_current_val(edge, parsed, ih)
                };
                let s = if is_active { stroke_active } else { stroke_passive };
                match edge {
                    RectEdge::Top | RectEdge::Bottom => {
                        let y = py(val.1);
                        draw_hline_s(&painter, y, s);
                        draw_handle(&painter, egui::pos2(mid_x, y), is_active);
                        if is_active { label_text = format!("y: {}", val.1); }
                    }
                    RectEdge::Left | RectEdge::Right => {
                        let x = px(val.0);
                        draw_vline_s(&painter, x, s);
                        draw_handle(&painter, egui::pos2(x, mid_y), is_active);
                        if is_active { label_text = format!("x: {}", val.0); }
                    }
                }
            }
        }
    }

    // 座標ラベル（縁取り付き）
    if !label_text.is_empty() {
        let font = egui::FontId::monospace(12.0);
        let lpos = egui::pos2(origin.x + 4.0, origin.y + 4.0);
        for (ddx, ddy) in [(-1.0f32,-1.0f32),(0.0,-1.0),(1.0,-1.0),(-1.0,0.0),(1.0,0.0),(-1.0,1.0),(0.0,1.0),(1.0,1.0)] {
            painter.text(egui::pos2(lpos.x+ddx, lpos.y+ddy), egui::Align2::LEFT_TOP,
                &label_text, font.clone(), egui::Color32::from_rgba_unmultiplied(0,0,0,200));
        }
        painter.text(lpos, egui::Align2::LEFT_TOP, &label_text, font,
            egui::Color32::from_rgba_unmultiplied(255, 255, 80, 255));
    }
}

/// RgbaImage をその場で egui テクスチャに変換して painter で描画する（ドラッグ中オーバーレイ用）
fn paste_rgba_overlay(ctx: &Context, painter: &egui::Painter, img: &RgbaImage, top_left: egui::Pos2) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let pixels: Vec<egui::Color32> = img.pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();
    let color_image = egui::ColorImage { size: [w, h], pixels };
    let texture = ctx.load_texture(
        "__drag_overlay_part__",
        color_image,
        egui::TextureOptions::NEAREST,
    );
    let rect = egui::Rect::from_min_size(top_left, egui::vec2(w as f32, h as f32));
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

// ---------------------------------------------------------------------------
// チェッカー柄
// ---------------------------------------------------------------------------

/// チェッカー柄（透明背景の表示用）を描画する
fn draw_checker(painter: &egui::Painter, rect: egui::Rect) {
    let tile = 12.0_f32;
    let light = egui::Color32::from_gray(200);
    let dark  = egui::Color32::from_gray(160);

    let cols = ((rect.width()  / tile).ceil() as i32) + 1;
    let rows = ((rect.height() / tile).ceil() as i32) + 1;

    for row in 0..rows {
        for col in 0..cols {
            let color = if (row + col) % 2 == 0 { light } else { dark };
            let min = egui::pos2(
                rect.min.x + col as f32 * tile,
                rect.min.y + row as f32 * tile,
            );
            let max = egui::pos2(
                (min.x + tile).min(rect.max.x),
                (min.y + tile).min(rect.max.y),
            );
            painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, color);
        }
    }
}

/// コントラスト比ラベル文字列を計算する。
/// k/s系: バルーン画像中央サンプリング色 vs font.color
/// c*系 : communicatebox.background.color vs communicatebox.font.color
fn calc_contrast_label(app: &BalloonEditorApp) -> Option<String> {
    let balloon_name = app.state.selected_balloon.trim_end_matches(".png");
    let cfg_key = format!("{}s.txt", balloon_name);
    // 個別設定と共通設定をマージ（個別設定が優先、差分ファイル仕様に対応）
    let parsed = {
        let mut merged = parse_descript(&app.state.descript_text);
        if let Some(indiv) = app.state.individual_texts.get(&cfg_key) {
            for (k, v) in parse_descript(indiv) { merged.insert(k, v); }
        }
        merged
    };

    if app.state.is_balloonc() {
        // c*系: communicatebox の背景色 vs 文字色
        let bg = get_color_from_descript(&parsed, "communicatebox.background.color")
            .unwrap_or(Rgb(255, 255, 255));
        let fg = get_color_from_descript(&parsed, "communicatebox.font.color")
            .unwrap_or(Rgb(0, 0, 0));
        let ratio = contrast_ratio(bg, fg);
        let lc    = apca_lc(fg, bg);
        Some(format!(
            "コントラスト比  WCAG2: {:.2}:1 {}  /  APCA: Lc {:.1} {}",
            ratio, wcag_level(ratio), lc, apca_bronze(lc)
        ))
    } else {
        // k/s系: バルーン画像中央サンプリング色 vs font.color
        let img  = app.state.balloon_cache.get(&app.state.selected_balloon)?;
        let base = sample_center_color(img, 0.25);
        let fg   = get_color_from_descript(&parsed, "font.color")
            .unwrap_or(Rgb(0, 0, 0));
        let ratio = contrast_ratio(base, fg);
        let lc    = apca_lc(fg, base);
        Some(format!(
            "コントラスト比  WCAG2: {:.2}:1 {}  /  APCA: Lc {:.1} {}",
            ratio, wcag_level(ratio), lc, apca_bronze(lc)
        ))
    }
}
