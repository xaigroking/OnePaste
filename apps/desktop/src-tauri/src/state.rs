//! Shell state: event log, tray state, hotkey state, tray texts and the language preference.

use crate::lang::Preference;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// Local-only mode: nothing is synced, on purpose.
    Local,
}

impl TrayState {
    pub fn key(self) -> &'static str {
        match self {
            TrayState::Synced => "synced",
            TrayState::Syncing => "syncing",
            TrayState::Offline => "offline",
            TrayState::Error => "error",
            TrayState::Local => "local",
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

/// Tray texts, supplied by the UI layer through i18next. Rust replaces `{state}` in the templates.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayTexts {
    pub open: String,
    pub diagnostics: String,
    pub quit: String,
    pub status_line: String,
    pub tooltip: String,
    pub synced: String,
    pub syncing: String,
    pub offline: String,
    pub error: String,
    pub local: String,
}

impl TrayTexts {
    pub fn state_name(&self, s: TrayState) -> &str {
        match s {
            TrayState::Synced => &self.synced,
            TrayState::Syncing => &self.syncing,
            TrayState::Offline => &self.offline,
            TrayState::Error => &self.error,
            TrayState::Local => &self.local,
        }
    }
}

pub struct Inner {
    pub log: VecDeque<LogEntry>,
    pub tray: TrayState,
    pub hotkey: HotkeyState,
    pub texts: TrayTexts,
    /// The language preference chosen by the user (`system` by default).
    pub language: Preference,
    /// The raw system locale as reported by the OS, if it could be read.
    pub system_locale: Option<String>,
    /// Where `settings.json` lives; `None` if the location cannot be determined.
    pub settings_dir: Option<PathBuf>,
}

impl Inner {
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
        AppState::new(Preference::System, None, None)
    }
}

impl AppState {
    pub fn new(language: Preference, system_locale: Option<String>, settings_dir: Option<PathBuf>) -> Self {
        // Until the UI layer pushes its texts, the tray uses placeholders.
        let placeholder = "…".to_string();
        let texts = TrayTexts {
            open: placeholder.clone(),
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
            log: VecDeque::new(),
            tray: TrayState::Synced,
            hotkey: HotkeyState::Unknown,
            texts,
            language,
            system_locale,
            settings_dir,
        }))
    }
}

