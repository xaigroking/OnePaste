// 发布版在 Windows 上不弹出控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod diag;
mod hotkey;
mod icons;
mod state;
mod tray;
mod watchers;

use state::AppState;
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_recents,
            commands::copy_back,
            commands::get_log,
            commands::get_hotkey_state,
            commands::get_tray_state,
            commands::set_texts,
            commands::secure_action,
            commands::copy_diagnostics,
        ])
        .setup(|app| {
            // 不显示 Dock 图标，只留菜单栏图标。
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            tray::build(&handle)?;
            hotkey::register(&handle);
            let watchers = watchers::start(&handle);
            app.manage(watchers);
            handle.state::<AppState>().0.lock().unwrap().push_log("started", None);
            Ok(())
        })
        // 关闭窗口只是隐藏到托盘；退出只能从托盘菜单。
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the application");
}
