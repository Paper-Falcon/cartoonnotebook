pub mod anilist;
pub mod bangumi;
pub mod db;
pub mod kitsu;
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
        // 单实例：第二次启动只聚焦已有窗口，避免抢 8787 端口变空壳。
        // 必须第一个注册
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .setup(move |app| {
            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                // 绑 0.0.0.0：手机要能连进来；本机访问走 loopback 免口令
                if let Err(e) = server::serve(st.clone(), "0.0.0.0", port).await {
                    eprintln!("[server] 启动失败: {e:#}");
                }
            });

            // 启动片刻后补齐已有国漫作品的封面（Kitsu 图源）
            {
                let st2 = state.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    server::heal_bgm_covers(st2).await;
                });
            }

            // 等本机服务端口就绪再创建窗口（杜绝「网页视图先于服务加载」的白屏竞态）
            for _ in 0..80 {
                if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            // 窗口直接加载本机服务：不依赖 tauri:// 资产协议，桌面与手机同一套渲染路径
            let url: tauri::Url = format!("http://127.0.0.1:{port}/")
                .parse()
                .expect("解析本机地址失败");
            let win = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("动漫档案馆")
                .inner_size(1320.0, 860.0)
                .min_inner_size(980.0, 660.0)
                .resizable(true)
                .center()
                .decorations(false)
                .transparent(true)
                .build()
                .expect("创建主窗口失败");

            // 恢复上次窗口的大小和位置
            if let Some(cfg) = server::load_window_cfg(&state) {
                if cfg.w >= 400 && cfg.h >= 300 && cfg.x > -2000 && cfg.y > -2000 {
                    let _ = win.set_position(tauri::PhysicalPosition::new(cfg.x, cfg.y));
                    let _ = win.set_size(tauri::PhysicalSize::new(cfg.w, cfg.h));
                }
                if cfg.maximized {
                    let _ = win.maximize();
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
