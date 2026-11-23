pub mod anilist;
pub mod bangumi;
pub mod db;
pub mod server;

use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    let data_dir = server::resolve_data_dir();
    server::apply_pending_restore(&data_dir);
    let port = server::resolve_port();
    println!("[app] data dir = {}", data_dir.display());

    let state = Arc::new(
        server::AppState::new(data_dir, port).expect("初始化数据目录 / 数据库失败"),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .setup(move |app| {
            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                // 绑 0.0.0.0：手机要能连进来；本机访问走 loopback 免口令
                if let Err(e) = server::serve(st, "0.0.0.0", port).await {
                    eprintln!("[server] 启动失败: {e:#}");
                }
            });

            // 恢复上次窗口的大小和位置
            if let Some(win) = app.get_webview_window("main") {
                if let Some(cfg) = server::load_window_cfg(&state) {
                    if cfg.w >= 400 && cfg.h >= 300 && cfg.x > -2000 && cfg.y > -2000 {
                        let _ = win.set_position(tauri::PhysicalPosition::new(cfg.x, cfg.y));
                        let _ = win.set_size(tauri::PhysicalSize::new(cfg.w, cfg.h));
                    }
                    if cfg.maximized {
                        let _ = win.maximize();
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关窗前把窗口状态写进 settings，下次启动恢复
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "main" {
                    let st = window.app_handle().state::<Arc<server::AppState>>();
                    server::save_window_state(&st, window);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}
