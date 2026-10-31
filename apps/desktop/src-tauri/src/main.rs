// Release builds do not open a console window on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adapters;
mod commands;
mod diag;
mod hotkey;
mod icons;
mod lifecycle;
mod quick;
mod lang;
mod state;
mod tray;
mod watchers;
#[cfg(any(windows, test))]
mod webview2;

use onepaste_app::{App, AppConfig};
use state::{AppState, TrayState};
use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};

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
    // One program per data directory. A second start asks the running one to show its window (or, with --quit, to quit)
    // and ends at once, before it touches a database.
    let quit_flag = std::env::args().skip(1).any(|a| a == "--quit");
    let lock_dir = settings_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("dev.onepaste.spike"));
    let _instance_lock = match onepaste_app::instance::InstanceLock::acquire(&lock_dir) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            let _ = onepaste_app::instance::send_request(&lock_dir, if quit_flag { onepaste_app::instance::Request::Quit } else { onepaste_app::instance::Request::Show });
            return;
        }
        // Without a lock the program still works; it just cannot keep a second copy out
        Err(_) => None,
    };
    if quit_flag {
        // Nothing was running
        return;
    }
    let preference = settings_dir.as_deref().map(lang::load).unwrap_or(lang::Preference::System);
    // The databases live next to the language setting: no new place on disk
    let data_dir = settings_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("dev.onepaste.spike"));
    let core = App::new(
        data_dir,
        Arc::new(adapters::PlatformSecretStore::new(adapters::MASTER_KEY_ACCOUNT)),
        Arc::new(adapters::PlatformClipboard),
        AppConfig {
            update_checks: true,
            update_version: env!("ONEPASTE_UPDATE_VERSION").to_string(),
            built_at: env!("ONEPASTE_BUILD_UNIX").parse().unwrap_or(0),
            ..AppConfig::default()
        },
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
            commands::app_devices,
            commands::app_current_server_url,
            commands::app_change_server,
            commands::close_to_tray,
            commands::app_item_image,
            commands::app_item_text,
            commands::app_save_image,
            commands::hide_quick_panel,
            commands::set_device_count,
            commands::app_check_update,
            commands::app_available_update,
            commands::app_skip_update,
            commands::app_open_release,
            commands::app_open_server_help,
            commands::app_revoke_device,
            commands::app_new_connection_code,
            commands::app_version,
            commands::app_wipe_local_data,
            commands::app_status,
            commands::app_search,
            commands::app_copy,
            commands::app_set_flags,
            commands::app_delete,
            commands::app_copy_items,
            commands::app_favorite_items,
            commands::app_stage_deletion,
            commands::app_pending_deletions,
            commands::app_undo_deletion,
            commands::app_get_settings,
            commands::app_set_settings,
            commands::get_log,
            commands::get_hotkey_state,
            commands::set_hotkey,
            commands::get_tray_state,
            commands::set_texts,
            commands::copy_diagnostics,
            commands::get_language_state,
            commands::set_language,
        ])
        .setup(move |app| {
            // The window is shown at start, so the Dock icon is there; hiding the window takes it away again
            // (lifecycle::hide_main_window) and showing the window brings it back.
            lifecycle::watch_requests(app.handle(), lock_dir.clone());
            let handle = app.handle().clone();
            tray::build(&handle)?;
            quick::create(&handle)?;
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
            // A newer version found by a background check is announced to the window; a failed check says nothing
            let for_update = handle.clone();
            core.on_update_available(move |info| {
                let _ = for_update.emit("onepaste://update-available", info);
            });
            let updater = core.clone();
            tauri::async_runtime::spawn(async move { updater.start_update_checks() });
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
        // Closing the window only hides it to the tray; quitting is from the tray menu, the application menu (Command-Q on
        // macOS) or `OnePaste --quit`.
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if window.label() == quick::LABEL {
                        let _ = window.hide();
                    } else {
                        lifecycle::on_close_requested(window.app_handle());
                    }
                }
                // The quick panel goes away when the user clicks elsewhere
                WindowEvent::Focused(false) if window.label() == quick::LABEL => {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build the application")
        .run(|app, event| {
            if matches!(&event, tauri::RunEvent::Exit) { app.state::<App>().shutdown(); }
            // Clicking the Dock icon, or opening the program again, while the window is hidden brings the window back
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
