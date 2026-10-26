//! 托盘：4 种状态各有不同的图标和提示文字；菜单文案由界面层提供。

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
    toggle: MenuItem<Wry>,
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

/// 通知界面：Rust 端状态有变化，需要重新拉取。
pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit("onepaste://changed", ());
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let texts = state.0.lock().unwrap().texts.clone();
    let status = MenuItem::with_id(app, "status", &texts.status_line, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", &texts.open, true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", &texts.toggle, true, None::<&str>)?;
    let diagnostics = MenuItem::with_id(app, "diagnostics", &texts.diagnostics, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status, &open, &toggle, &diagnostics, &quit])?;

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

    app.manage(TrayItems { status, open, toggle, diagnostics, quit });
    refresh(app);
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => show_main_window(app),
        "toggle" => {
            let next = app.state::<AppState>().0.lock().unwrap().tray.next();
            set_state(app, next);
        }
        "diagnostics" => {
            let _ = commands::copy_diagnostics_impl(app);
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

/// 切换托盘状态：换图标、换提示文字、更新状态行。
pub fn set_state(app: &AppHandle, next: TrayState) {
    app.state::<AppState>().0.lock().unwrap().tray = next;
    refresh(app);
    notify_changed(app);
}

/// 按当前状态和文案刷新托盘图标、提示文字和菜单。
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
        let _ = items.toggle.set_text(&texts.toggle);
        let _ = items.diagnostics.set_text(&texts.diagnostics);
        let _ = items.quit.set_text(&texts.quit);
    }
}
