// Release builds do not open a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapters;
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

use onepaste_app::{App, AppConfig};
use state::{AppState, TrayState};
use std::sync::Arc;
use tauri::{Manager, WindowEvent};

fn main() {
    // `OnePaste --version` prints the version and the build id, and nothing else starts
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("OnePaste {}", env!("ONEPASTE_VERSION_LONG"));
        return;
    }
    // Without the WebView2 runtime nothing can be shown, so say so and exit with code 2.
    #[cfg(windows)]
    webview2::ensure_or_exit();

    let settings_dir = lang::settings_dir();
    let preference = settings_dir.as_deref().map(lang::load).unwrap_or(lang::Preference::System);
    // The databases live next to the language setting: no new place on disk
    let data_dir = settings_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("dev.onepaste.spike"));
    let core = App::new(
        data_dir,
        Arc::new(adapters::PlatformSecretStore::new(adapters::MASTER_KEY_ACCOUNT)),
        Arc::new(adapters::PlatformClipboard),
        AppConfig::default(),
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::new(preference, lang::system_locale(), settings_dir))
        .manage(core.clone())
        .invoke_handler(tauri::generate_handler![
            commands::app_join,
            commands::app_first_run_state,
            commands::app_choose_local_only,
            commands::app_probe_server,
            commands::app_generate_passphrase,
            commands::app_create_vault,
            commands::app_confirm_kit_saved,
            commands::app_cancel_setup,
            commands::app_join_with_kit,
            commands::app_show_kit,
            commands::app_copy_secret,
            commands::app_version,
            commands::app_wipe_local_data,
            commands::app_status,
            commands::app_search,
            commands::app_copy,
            commands::app_set_flags,
            commands::app_delete,
            commands::app_get_settings,
            commands::app_set_settings,
            commands::get_log,
            commands::get_hotkey_state,
            commands::get_tray_state,
            commands::set_texts,
            commands::copy_diagnostics,
            commands::get_language_state,
            commands::set_language,
        ])
        .setup(move |app| {
            // No Dock icon, only the menu bar icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            tray::build(&handle)?;
            hotkey::register(&handle);
            let watchers = watchers::start(&handle, &core);
            app.manage(watchers);
            // The tray shows what the sync is really doing
            let for_tray = handle.clone();
            core.on_status_change(move |st| {
                let tray_state = match st.tray {
                    onepaste_app::TrayState::Synced => TrayState::Synced,
                    onepaste_app::TrayState::Syncing => TrayState::Syncing,
                    onepaste_app::TrayState::Offline => TrayState::Offline,
                    onepaste_app::TrayState::Error => TrayState::Error,
                    onepaste_app::TrayState::Local => TrayState::Local,
                };
                tray::set_state(&for_tray, tray_state);
            });
            // Open the vault with the key from the secure store (no passphrase) and start syncing
            let starter = core.clone();
            let for_log = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = starter.start().await {
                    for_log.state::<AppState>().0.lock().unwrap().push_log("start_failed", Some(e.code()));
                }
            });
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
