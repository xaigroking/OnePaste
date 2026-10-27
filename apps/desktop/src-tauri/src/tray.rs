//! Tray: six states, each with its own icon and tooltip, and the menu: status line, open, quick panel, pause listening,
//! sync now, devices, settings, quit. Menu texts come from the UI layer.

use crate::state::{AppState, TrayState};
use crate::{icons, quick};
use onepaste_app::App;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "main";

pub struct TrayItems {
    status: MenuItem<Wry>,
    open: MenuItem<Wry>,
    quick: MenuItem<Wry>,
    pause: MenuItem<Wry>,
    sync: MenuItem<Wry>,
    devices: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tells the UI that Rust-side state changed and needs to be fetched again.
pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit("onepaste://changed", ());
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let texts = state.0.lock().unwrap().texts.clone();
    let status = MenuItem::with_id(app, "status", &texts.status_line, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", &texts.open, true, None::<&str>)?;
    let quick_item = MenuItem::with_id(app, "quick", &texts.quick_panel, true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", &texts.pause, true, None::<&str>)?;
    let sync = MenuItem::with_id(app, "sync", &texts.sync_now, true, None::<&str>)?;
    let devices = MenuItem::with_id(app, "devices", &texts.devices, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", &texts.settings, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let sep4 = PredefinedMenuItem::separator(app)?;
    let items: Vec<&dyn IsMenuItem<Wry>> =
        vec![&status, &sep1, &open, &quick_item, &sep2, &pause, &sync, &sep3, &devices, &settings, &sep4, &quit];
    let menu = Menu::with_items(app, &items)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::new_owned(icons::rgba(TrayState::Synced), icons::SIZE, icons::SIZE))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayItems { status, open, quick: quick_item, pause, sync, devices, settings, quit });
    refresh(app);
    Ok(())
}

/// Shows the main window on one of its pages.
fn open_page(app: &AppHandle, page: &str) {
    show_main_window(app);
    let _ = app.emit_to("main", "onepaste://navigate", page.to_string());
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => show_main_window(app),
        "quick" => quick::show(app),
        "pause" => {
            // Listening is the "record what I copy" setting; the tray and the settings page share it
            let core = app.state::<App>().inner().clone();
            let mut s = core.settings();
            s.record_clipboard = !s.record_clipboard;
            if core.set_settings(s).is_err() {
                app.state::<AppState>().0.lock().unwrap().push_log("settings_save_failed", None);
            }
            refresh(app);
            notify_changed(app);
        }
        "sync" => {
            let core = app.state::<App>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ = core.sync_now().await;
            });
        }
        "devices" => open_page(app, "devices"),
        "settings" => open_page(app, "settings"),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Switches the tray state: new icon, new tooltip, updated status line.
pub fn set_state(app: &AppHandle, next: TrayState) {
    app.state::<AppState>().0.lock().unwrap().tray = next;
    refresh(app);
    notify_changed(app);
}

/// Refreshes the tray icon, tooltip and menu from the current state and texts.
pub fn refresh(app: &AppHandle) {
    let listening = app.try_state::<App>().map(|c| c.settings().record_clipboard).unwrap_or(true);
    let (state, texts, devices) = {
        let s = app.state::<AppState>();
        let g = s.0.lock().unwrap();
        // Switching listening off shows as its own state, whatever the sync is doing
        (if listening { g.tray } else { TrayState::Paused }, g.texts.clone(), g.device_count)
    };
    let name = texts.state_name(state).to_string();
    let mut line = texts.status_line.replace("{state}", &name);
    if let Some(n) = devices {
        line.push_str(&format!(" · {}", texts.devices_count.replace("{count}", &n.to_string())));
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(Image::new_owned(icons::rgba(state), icons::SIZE, icons::SIZE)));
        let _ = tray.set_tooltip(Some(texts.tooltip.replace("{state}", &name)));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.status.set_text(line);
        let _ = items.open.set_text(&texts.open);
        let _ = items.quick.set_text(&texts.quick_panel);
        let _ = items.pause.set_text(if listening { &texts.pause } else { &texts.resume });
        let _ = items.sync.set_text(&texts.sync_now);
        let _ = items.devices.set_text(&texts.devices);
        let _ = items.settings.set_text(&texts.settings);
        let _ = items.quit.set_text(&texts.quit);
    }
}
