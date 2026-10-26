//! 应用状态：最近记录（只放内存）、事件日志、托盘状态、热键状态、托盘文案。

use onepaste_platform::clipboard::Content;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_RECENTS: usize = 20;
pub const MAX_LOG: usize = 200;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayState {
    Synced,
    Syncing,
    Offline,
    Error,
}

impl TrayState {
    pub fn next(self) -> TrayState {
        match self {
            TrayState::Synced => TrayState::Syncing,
            TrayState::Syncing => TrayState::Offline,
            TrayState::Offline => TrayState::Error,
            TrayState::Error => TrayState::Synced,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TrayState::Synced => "synced",
            TrayState::Syncing => "syncing",
            TrayState::Offline => "offline",
            TrayState::Error => "error",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HotkeyState {
    Unknown,
    Registered,
    Busy,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts_ms: u64,
    pub kind: String,
    pub detail: Option<String>,
}

pub struct Recent {
    pub id: u64,
    pub content: Content,
    pub ts_ms: u64,
}

/// 托盘文案，由界面层的 i18next 提供；模板里的 `{state}` 由 Rust 替换。
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayTexts {
    pub open: String,
    pub toggle: String,
    pub diagnostics: String,
    pub quit: String,
    pub status_line: String,
    pub tooltip: String,
    pub synced: String,
    pub syncing: String,
    pub offline: String,
    pub error: String,
}

impl TrayTexts {
    pub fn state_name(&self, s: TrayState) -> &str {
        match s {
            TrayState::Synced => &self.synced,
            TrayState::Syncing => &self.syncing,
            TrayState::Offline => &self.offline,
            TrayState::Error => &self.error,
        }
    }
}

pub struct Inner {
    pub recents: VecDeque<Recent>,
    pub log: VecDeque<LogEntry>,
    pub tray: TrayState,
    pub hotkey: HotkeyState,
    pub texts: TrayTexts,
    next_id: u64,
}

impl Inner {
    /// 新记录放在最前面；内容相同的旧记录先移除，超过上限的丢弃最旧的。
    pub fn add_recent(&mut self, content: Content) {
        self.recents.retain(|r| r.content != content);
        self.next_id += 1;
        self.recents.push_front(Recent { id: self.next_id, content, ts_ms: now_ms() });
        self.recents.truncate(MAX_RECENTS);
    }

    pub fn push_log(&mut self, kind: &str, detail: Option<String>) {
        self.log.push_back(LogEntry { ts_ms: now_ms(), kind: kind.to_string(), detail });
        while self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
    }
}

pub struct AppState(pub Mutex<Inner>);

impl Default for AppState {
    fn default() -> Self {
        // 界面层推送文案之前，托盘先用占位符。
        let placeholder = "…".to_string();
        let texts = TrayTexts {
            open: placeholder.clone(),
            toggle: placeholder.clone(),
            diagnostics: placeholder.clone(),
            quit: placeholder.clone(),
            status_line: "{state}".into(),
            tooltip: "OnePaste {state}".into(),
            synced: placeholder.clone(),
            syncing: placeholder.clone(),
            offline: placeholder.clone(),
            error: placeholder,
        };
        AppState(Mutex::new(Inner {
            recents: VecDeque::new(),
            log: VecDeque::new(),
            tray: TrayState::Synced,
            hotkey: HotkeyState::Unknown,
            texts,
            next_id: 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Content {
        Content::Text(s.to_string())
    }

    #[test]
    fn recents_are_capped_deduplicated_and_newest_first() {
        let AppState(m) = AppState::default();
        let mut s = m.into_inner().unwrap();
        for i in 0..25 {
            s.add_recent(text(&format!("item {i}")));
        }
        assert_eq!(s.recents.len(), MAX_RECENTS);
        assert_eq!(s.recents[0].content, text("item 24"));
        s.add_recent(text("item 10"));
        assert_eq!(s.recents.len(), MAX_RECENTS);
        assert_eq!(s.recents[0].content, text("item 10"));
        assert_eq!(s.recents.iter().filter(|r| r.content == text("item 10")).count(), 1);
    }

    #[test]
    fn tray_states_cycle_through_all_four() {
        let mut s = TrayState::Synced;
        let mut seen = vec![s.key()];
        for _ in 0..3 {
            s = s.next();
            seen.push(s.key());
        }
        assert_eq!(seen, ["synced", "syncing", "offline", "error"]);
        assert_eq!(s.next(), TrayState::Synced);
    }
}
