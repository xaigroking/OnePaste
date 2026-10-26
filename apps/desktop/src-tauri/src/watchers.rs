//! Connects platform events (clipboard, wake from sleep, network changes) to the app state and the event log.

use crate::state::AppState;
use crate::tray;
use onepaste_platform::clipboard::{Event, Monitor};
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

pub fn start(app: &AppHandle) -> Watchers {
    let mut held: Vec<Box<dyn Any + Send + Sync>> = Vec::new();

    let a = app.clone();
    match Monitor::start(move |event| match event {
        Event::Captured(content) => {
            a.state::<AppState>().0.lock().unwrap().add_recent(content);
            tray::notify_changed(&a);
        }
        Event::SkippedSensitive => log(&a, "sensitive_skipped", None),
        Event::SkippedSelf => {}
    }) {
        Ok(m) => held.push(Box::new(m)),
        Err(e) => log(app, "watcher_failed", Some(format!("clipboard {}", e.code()))),
    }

    let a = app.clone();
    match WakeWatcher::start(move || log(&a, "wake", None)) {
        Ok(w) => held.push(Box::new(w)),
        Err(e) => log(app, "watcher_failed", Some(format!("wake {}", e.code()))),
    }

    let a = app.clone();
    match NetworkWatcher::start(move |s| {
        let kind = if s == NetworkState::Online { "network_online" } else { "network_offline" };
        log(&a, kind, None);
    }) {
        Ok(w) => held.push(Box::new(w)),
        Err(e) => log(app, "watcher_failed", Some(format!("network {}", e.code()))),
    }

    Watchers { _held: held }
}
