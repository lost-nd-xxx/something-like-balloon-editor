#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod gui;

fn main() -> eframe::Result<()> {
    // アイコンを読み込む（実行ファイルと同じディレクトリの appicon.ico）
    let icon = load_icon();

    // 保存済みウィンドウサイズを復元する
    let initial_size = load_window_size().unwrap_or([1400.0, 720.0]);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(gui::state::APP_NAME)
        .with_inner_size(initial_size);
    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        gui::state::APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(gui::app::BalloonEditorApp::new(cc)))),
    )
}

fn load_window_size() -> Option<[f32; 2]> {
    let root = std::env::current_exe().ok()?;
    let state_path = root.parent()?.join("state.json");
    let profile = crate::core::profile::load_state(&state_path)?;
    let [w, h] = profile.window_size;
    if w > 0.0 && h > 0.0 { Some([w, h]) } else { None }
}

fn load_icon() -> Option<egui::IconData> {
    static ICON_BYTES: &[u8] = include_bytes!("../resource/icon/appicon.ico");
    let img = image::load_from_memory(ICON_BYTES).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}
