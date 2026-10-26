//! 把平台事件（剪贴板、睡眠唤醒、网络变化）接到应用状态和事件日志。

use crate::state::AppState;
use crate::tray;
use onepaste_platform::clipboard::{Event, Monitor};
use onepaste_platform::network::{NetworkState, NetworkWatcher};
use onepaste_platform::power::WakeWatcher;
use std::any::Any;
use tauri::{AppHandle, Manager};

/// 持有各个监听器，应用退出时一起停止。
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
