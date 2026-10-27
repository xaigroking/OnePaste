//! The quick panel: a small window that the global hotkey and the tray menu bring up. It is created hidden at start so
//! that showing it never has to build a window from a menu or hotkey callback.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "quick";

/// Creates the hidden window. The page decides what to draw from the window label.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OnePaste")
        .inner_size(560.0, 520.0)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .center()
        .visible(false)
        .build()?;
    Ok(())
}

/// Shows the panel and tells the page, so it clears its search and focuses the search box.
pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.center();
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = app.emit_to(LABEL, "onepaste://quick-opened", ());
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// The hotkey: show the panel, or hide it if it is already in front.
pub fn toggle(app: &AppHandle) {
    let visible = app.get_webview_window(LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}
