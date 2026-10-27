//! Tray: five states, each with its own icon and tooltip. Menu texts come from the UI layer.

use crate::state::{AppState, TrayState};
use crate::{commands, icons};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "main";

pub struct TrayItems {
    status: MenuItem<Wry>,
    open: MenuItem<Wry>,
    diagnostics: MenuItem<Wry>,
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
    let diagnostics = MenuItem::with_id(app, "diagnostics", &texts.diagnostics, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status, &open, &diagnostics, &quit])?;

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

    app.manage(TrayItems { status, open, diagnostics, quit });
    refresh(app);
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => show_main_window(app),
        "diagnostics" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = commands::copy_diagnostics_impl(&app).await;
            });
        }
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
    let (state, texts) = {
        let s = app.state::<AppState>();
        let g = s.0.lock().unwrap();
        (g.tray, g.texts.clone())
    };
    let name = texts.state_name(state).to_string();
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(Image::new_owned(icons::rgba(state), icons::SIZE, icons::SIZE)));
        let _ = tray.set_tooltip(Some(texts.tooltip.replace("{state}", &name)));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.status.set_text(texts.status_line.replace("{state}", &name));
        let _ = items.open.set_text(&texts.open);
        let _ = items.diagnostics.set_text(&texts.diagnostics);
        let _ = items.quit.set_text(&texts.quit);
    }
}
