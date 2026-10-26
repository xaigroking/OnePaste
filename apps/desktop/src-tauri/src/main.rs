// Release builds do not open a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod diag;
mod hotkey;
mod icons;
mod lang;
mod state;
mod tray;
mod watchers;
#[cfg(any(windows, test))]
mod webview2;

use state::AppState;
use tauri::{Manager, WindowEvent};

fn main() {
    // Without the WebView2 runtime nothing can be shown, so say so and exit with code 2.
    #[cfg(windows)]
    webview2::ensure_or_exit();

    let settings_dir = lang::settings_dir();
    let preference = settings_dir.as_deref().map(lang::load).unwrap_or(lang::Preference::System);
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new(preference, lang::system_locale(), settings_dir))
        .invoke_handler(tauri::generate_handler![
            commands::get_recents,
            commands::copy_back,
            commands::get_log,
            commands::get_hotkey_state,
            commands::get_tray_state,
            commands::set_texts,
            commands::secure_action,
            commands::copy_diagnostics,
            commands::get_language_state,
            commands::set_language,
        ])
        .setup(|app| {
            // No Dock icon, only the menu bar icon.
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
        // Closing the window only hides it to the tray; quitting is only possible from the tray menu.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run the application");
}
