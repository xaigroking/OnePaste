//! Connects wake-from-sleep and network events to the application logic. The clipboard is watched by the application
//! logic itself, through the platform adapters.

use crate::state::AppState;
use crate::tray;
use onepaste_app::App;
use onepaste_platform::network::{NetworkState, NetworkWatcher};
use onepaste_platform::power::WakeWatcher;
use std::any::Any;
use tauri::{AppHandle, Manager};

/// Holds the watchers; they all stop when the app exits.
pub struct Watchers {
    _held: Vec<Box<dyn Any + Send + Sync>>,
}

fn log(app: &AppHandle, kind: &str, detail: Option<String>) {
    app.state::<AppState>().0.lock().unwrap().push_log(kind, detail);
    tray::notify_changed(app);
}

pub fn start(app: &AppHandle, core: &App) -> Watchers {
    let mut held: Vec<Box<dyn Any + Send + Sync>> = Vec::new();

    let c = core.clone();
    match WakeWatcher::start(move || c.notify_wake()) {
        Ok(w) => held.push(Box::new(w)),
        Err(e) => log(app, "watcher_failed", Some(format!("wake {}", e.code()))),
    }

    let c = core.clone();
    match NetworkWatcher::start(move |s| c.notify_network(s == NetworkState::Online)) {
        Ok(w) => held.push(Box::new(w)),
        Err(e) => log(app, "watcher_failed", Some(format!("network {}", e.code()))),
    }

    Watchers { _held: held }
}
