//! Global hotkey: Ctrl+Alt+V on Windows, ⌃⌥V (Control+Option+V) on macOS. It brings up the quick panel.

use crate::state::{AppState, HotkeyState};
use crate::quick;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub fn register(app: &AppHandle) {
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
    let result = app.global_shortcut().on_shortcut(shortcut, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            quick::toggle(app);
        }
    });
    let state = app.state::<AppState>();
    let mut g = state.0.lock().unwrap();
    match result {
        Ok(()) => {
            g.hotkey = HotkeyState::Registered;
            g.push_log("hotkey_registered", None);
        }
        Err(e) => {
            g.hotkey = HotkeyState::Busy;
            g.push_log("hotkey_busy", Some(e.to_string()));
        }
    }
}
