//! 界面可以调用的命令。返回值里没有剪贴板内容之外的敏感信息；密钥内容永远不返回。

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

/// 把一条记录写回剪贴板；写入时带防回环标记，所以不会在列表里多出重复记录。
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

/// 安全存储测试：只返回成功或失败和错误码，不返回密钥内容。
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

/// 把诊断信息放进剪贴板。带防回环标记，所以不会作为一条记录出现在列表里。
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
