//! Commands the UI can call. Responses carry nothing sensitive beyond clipboard content; key material is never returned.

use crate::lang::{self, Preference};
use crate::state::{AppState, HotkeyState, LogEntry, TrayState, TrayTexts};
use crate::{diag, tray};
use onepaste_platform::clipboard::{self, Content};
use onepaste_platform::{secure_store, system};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

const TEST_ACCOUNT: &str = "spike-test-key";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    id: u64,
    kind: &'static str,
    preview: String,
    width: u32,
    height: u32,
    ts_ms: u64,
}

#[derive(Serialize)]
pub struct ActionResult {
    ok: bool,
    code: String,
}

impl ActionResult {
    fn from<T>(r: Result<T, onepaste_platform::PlatformError>) -> Self {
        match r {
            Ok(_) => ActionResult { ok: true, code: "ok".into() },
            Err(e) => ActionResult { ok: false, code: e.code() },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageState {
    preference: Preference,
    effective: &'static str,
    system_locale: Option<String>,
}

fn language_state(state: &AppState) -> LanguageState {
    let g = state.0.lock().unwrap();
    LanguageState {
        preference: g.language,
        effective: lang::effective(g.language, g.system_locale.as_deref()),
        system_locale: g.system_locale.clone(),
    }
}

#[tauri::command]
pub fn get_language_state(state: State<AppState>) -> LanguageState {
    language_state(&state)
}

/// Stores and persists the language preference. The UI and the tray switch immediately: the UI
/// reloads its language from the state and pushes fresh tray texts.
#[tauri::command]
pub fn set_language(app: AppHandle, preference: Preference) -> LanguageState {
    let state = app.state::<AppState>();
    let dir = {
        let mut g = state.0.lock().unwrap();
        g.language = preference;
        g.settings_dir.clone()
    };
    if let Some(dir) = dir {
        if lang::save(&dir, preference).is_err() {
            state.0.lock().unwrap().push_log("settings_save_failed", None);
        }
    }
    tray::notify_changed(&app);
    language_state(&state)
}

#[tauri::command]
pub fn get_recents(state: State<AppState>) -> Vec<RecentDto> {
    let g = state.0.lock().unwrap();
    g.recents
        .iter()
        .map(|r| match &r.content {
            Content::Text(t) => RecentDto {
                id: r.id,
                kind: "text",
                preview: t.lines().next().unwrap_or("").to_string(),
                width: 0,
                height: 0,
                ts_ms: r.ts_ms,
            },
            Content::Image(i) => RecentDto {
                id: r.id,
                kind: "image",
                preview: String::new(),
                width: i.width,
                height: i.height,
                ts_ms: r.ts_ms,
            },
        })
        .collect()
}

/// Puts a recent item back on the clipboard. The write carries the origin marker, so no duplicate entry appears in the list.
#[tauri::command]
pub fn copy_back(state: State<AppState>, id: u64) -> Result<(), String> {
    let content = {
        let g = state.0.lock().unwrap();
        g.recents.iter().find(|r| r.id == id).map(|r| r.content.clone())
    };
    let content = content.ok_or_else(|| "not_found".to_string())?;
    clipboard::write(&content).map_err(|e| e.code())
}

#[tauri::command]
pub fn get_log(state: State<AppState>) -> Vec<LogEntry> {
    state.0.lock().unwrap().log.iter().cloned().collect()
}

#[tauri::command]
pub fn get_hotkey_state(state: State<AppState>) -> HotkeyState {
    state.0.lock().unwrap().hotkey
}

#[tauri::command]
pub fn get_tray_state(state: State<AppState>) -> TrayState {
    state.0.lock().unwrap().tray
}

#[tauri::command]
pub fn set_texts(app: AppHandle, texts: TrayTexts) {
    app.state::<AppState>().0.lock().unwrap().texts = texts;
    tray::refresh(&app);
}

/// Secure storage test: returns only success or failure plus an error code, never the key contents.
#[tauri::command]
pub fn secure_action(action: String) -> ActionResult {
    match action.as_str() {
        "write" => {
            let mut secret = [0u8; secure_store::SECRET_LEN];
            OsRng.fill_bytes(&mut secret);
            ActionResult::from(secure_store::write(TEST_ACCOUNT, &secret))
        }
        "read" => ActionResult::from(secure_store::read(TEST_ACCOUNT)),
        "delete" => ActionResult::from(secure_store::delete(TEST_ACCOUNT)),
        _ => ActionResult { ok: false, code: "invalid_action".into() },
    }
}

/// Puts the diagnostics text on the clipboard. It carries the origin marker, so it does not show up as a list entry.
pub fn copy_diagnostics_impl(app: &AppHandle) -> ActionResult {
    let text = {
        let s = app.state::<AppState>();
        let g = s.0.lock().unwrap();
        diag::build(&g, &system::os_version())
    };
    ActionResult::from(clipboard::write(&Content::Text(text)))
}

#[tauri::command]
pub fn copy_diagnostics(app: AppHandle) -> ActionResult {
    copy_diagnostics_impl(&app)
}
