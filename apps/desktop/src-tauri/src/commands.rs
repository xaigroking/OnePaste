//! Commands the UI can call. Responses carry nothing sensitive beyond clipboard content; key material is never returned.

use crate::lang::{self, Preference};
use crate::state::{AppState, HotkeyState, LogEntry, TrayState, TrayTexts};
use crate::{diag, tray};
use onepaste_app::{parse_filter, App, ItemView, Settings, Status};
use onepaste_platform::clipboard::{self, Content};
use onepaste_platform::system;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

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

/// The shell's own events (start, hotkey, watchers) together with the application's (join, capture, sync).
#[tauri::command]
pub fn get_log(state: State<AppState>, core: State<App>) -> Vec<LogEntry> {
    let mut all: Vec<LogEntry> = state.0.lock().unwrap().log.iter().cloned().collect();
    all.extend(core.event_log().into_iter().map(|e| LogEntry { ts_ms: e.ts_ms, kind: e.kind, detail: e.detail }));
    all.sort_by_key(|e| e.ts_ms);
    all
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

/// Joins an existing vault. The passphrase is used once; the master key goes into the system secure store.
#[tauri::command]
pub async fn app_join(core: State<'_, App>, code: String, passphrase: String, device_name: String) -> Result<(), String> {
    core.join(&code, &passphrase, &device_name).await.map_err(|e| e.code())
}

#[tauri::command]
pub async fn app_status(core: State<'_, App>) -> Result<Status, String> {
    Ok(core.status().await)
}

#[tauri::command]
pub async fn app_search(core: State<'_, App>, query: String, filter: String, limit: usize, offset: usize) -> Result<Vec<ItemView>, String> {
    core.search(&query, parse_filter(&filter), limit, offset).await.map_err(|e| e.code())
}

/// Puts an item back on the clipboard. The write carries the origin marker, so no new item appears.
#[tauri::command]
pub async fn app_copy(core: State<'_, App>, item_id: String) -> Result<(), String> {
    core.copy(&item_id).await.map_err(|e| e.code())
}

#[tauri::command]
pub async fn app_set_flags(core: State<'_, App>, item_id: String, favorite: Option<bool>, pinned: Option<bool>, note: Option<String>) -> Result<(), String> {
    core.set_flags(&item_id, favorite, pinned, note).await.map_err(|e| e.code())
}

#[tauri::command]
pub async fn app_delete(core: State<'_, App>, item_id: String) -> Result<(), String> {
    core.delete(&item_id).await.map_err(|e| e.code())
}

#[tauri::command]
pub fn app_get_settings(core: State<App>) -> Settings {
    core.settings()
}

#[tauri::command]
pub fn app_set_settings(core: State<App>, settings: Settings) -> Result<(), String> {
    core.set_settings(settings).map_err(|e| e.code())
}

/// Puts the diagnostics text on the clipboard. It carries the origin marker, so it does not show up as a list entry.
/// The text holds versions, states, counts and event kinds; never clipboard content or keys.
pub async fn copy_diagnostics_impl(app: &AppHandle) -> ActionResult {
    let core = app.state::<App>().inner().clone();
    let from_app = core.diagnostics().await;
    let text = {
        let s = app.state::<AppState>();
        let g = s.0.lock().unwrap();
        format!("{}{}", diag::build(&g, &system::os_version()), from_app)
    };
    ActionResult::from(clipboard::write(&Content::Text(text)))
}

#[tauri::command]
pub async fn copy_diagnostics(app: AppHandle) -> ActionResult {
    copy_diagnostics_impl(&app).await
}
