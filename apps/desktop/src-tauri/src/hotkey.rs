//! A configurable global shortcut. Registration always precedes releasing the previous shortcut.

use crate::state::{AppState, HotkeyState};
use crate::{quick, tray};
use onepaste_app::{hotkeys::{self, Controller, Error, Port}, App};
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct Hotkeys(Mutex<Controller>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub state: HotkeyState,
    pub current: Option<String>,
    pub warning: Option<&'static str>,
}

pub fn info(state: &AppState) -> Info {
    let g = state.0.lock().unwrap();
    Info { state: g.hotkey, current: g.hotkey_binding.clone(), warning: g.hotkey_warning }
}

fn shortcut(binding: &str) -> Result<Shortcut, Error> {
    hotkeys::normalize(binding)?.parse().map_err(|_| Error::Invalid)
}

struct Native<'a>(&'a AppHandle);

impl Port for Native<'_> {
    fn register(&mut self, binding: &str) -> Result<(), Error> {
        self.0.global_shortcut().on_shortcut(shortcut(binding)?, |app, pressed, event| {
            if event.state != ShortcutState::Pressed { return; }
            let current = app.state::<AppState>().0.lock().unwrap().hotkey_binding.clone();
            if current.as_deref().and_then(|s| shortcut(s).ok()).is_some_and(|s| s.id() == pressed.id()) { quick::toggle(app); }
        }).map_err(|_| Error::Busy)
    }

    fn unregister(&mut self, binding: &str) -> Result<(), Error> {
        self.0.global_shortcut().unregister(shortcut(binding)?).map_err(|_| Error::Cleanup)
    }

    fn save(&mut self, binding: Option<&str>) -> Result<(), Error> {
        self.0.state::<App>().set_quick_hotkey(binding).map_err(|_| Error::Storage)
    }
}

fn publish(app: &AppHandle, controller: &Controller, error: Option<Error>) {
    let state = app.state::<AppState>();
    let mut g = state.0.lock().unwrap();
    g.hotkey_binding = controller.active().map(str::to_string);
    g.hotkey_warning = controller.warning();
    g.hotkey = if controller.active().is_none() || controller.warning() == Some("fallback") { HotkeyState::Busy } else { HotkeyState::Registered };
    if let Some(error) = error { g.push_log("hotkey_change_failed", Some(error.code().into())); }
    else if controller.warning().is_some() { g.push_log("hotkey_busy", Some("fallback".into())); }
    else { g.push_log("hotkey_registered", None); }
}

pub fn register(app: &AppHandle) {
    let saved = app.state::<App>().settings().quick_hotkey;
    let controller = Controller::start(saved.as_deref(), &mut Native(app));
    publish(app, &controller, None);
    app.manage(Hotkeys(Mutex::new(controller)));
}

/// Runs on a blocking worker: macOS registration dispatches to the main thread.
pub fn configure(app: &AppHandle, binding: Option<&str>) -> Result<Info, String> {
    let managed = app.state::<Hotkeys>();
    let mut controller = managed.0.lock().unwrap();
    let result = controller.configure(binding, &mut Native(app));
    publish(app, &controller, result.as_ref().err().copied());
    drop(controller);
    tray::notify_changed(app);
    result.map(|_| info(&app.state::<AppState>())).map_err(|e| e.code().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_parser_checks_key_codes_after_portable_modifier_validation() {
        for binding in ["Control+KeyK", "Alt+F24", "Super+Digit1", hotkeys::DEFAULT_BINDING] { assert!(shortcut(binding).is_ok()); }
        for binding in ["KeyV", "Control+ControlLeft", "Control+Escape", "Control+NotARealKey"] { assert!(shortcut(binding).is_err()); }
    }

    #[test]
    fn equivalent_modifier_orders_produce_the_same_native_registration() {
        assert_eq!(shortcut("Alt+Ctrl+KeyK").unwrap().id(), shortcut("Control+Alt+KeyK").unwrap().id());
    }
}
