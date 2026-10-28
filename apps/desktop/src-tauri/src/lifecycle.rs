//! How the program starts, hides and ends. One program per data directory; closing the window hides it to the tray (the
//! first time the window says so); a second start brings the window of the running one to the front; the running one can
//! be asked to quit with `OnePaste --quit`.

use crate::state::AppState;
use crate::tray;
use onepaste_app::instance::{take_request, Request};
use onepaste_app::App;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Hides the window. On macOS the Dock icon goes with it, so the program is only in the menu bar; the window comes back
/// from the menu bar icon, from the Dock (while it is there), or when the program is started again.
pub fn hide_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(false);
}

/// The window asked to close. The first time the user is told, in the window, that the program keeps running; after that
/// (or once they dismissed the note) the window is just hidden.
pub fn on_close_requested(app: &AppHandle) {
    let dismissed = app.try_state::<App>().map(|c| c.settings().tray_notice_dismissed).unwrap_or(true);
    if dismissed {
        hide_main_window(app);
    } else {
        // The page shows the note and then calls `close_to_tray`
        show_note(app);
    }
}

fn show_note(app: &AppHandle) {
    if app.emit_to("main", "onepaste://close-notice", ()).is_err() {
        hide_main_window(app);
    }
}

/// Looks a few times a second for a request from a second start of the program.
pub fn watch_requests(app: &AppHandle, dir: PathBuf) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(400));
        match take_request(&dir) {
            Some(Request::Show) => {
                tray::show_main_window(&app);
                tray::notify_changed(&app);
            }
            Some(Request::Quit) => {
                app.state::<AppState>().0.lock().unwrap().push_log("quit_requested", None);
                app.exit(0);
            }
            None => {}
        }
    });
}
