import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TrayState = "synced" | "syncing" | "offline" | "error";
export type HotkeyState = "registered" | "busy" | "unknown";

export interface RecentItem {
  id: number;
  kind: "text" | "image";
  /** 文本的第一行；图片为空串。 */
  preview: string;
  width: number;
  height: number;
  tsMs: number;
}

export interface LogEntry {
  tsMs: number;
  kind: string;
  detail: string | null;
}

export interface SecureResult {
  ok: boolean;
  code: string;
}

/** 托盘文案：由界面层的 i18next 提供，Rust 端不含任何可见文字。 */
export interface TrayTexts {
  open: string;
  toggle: string;
  diagnostics: string;
  quit: string;
  statusLine: string;
  tooltip: string;
  synced: string;
  syncing: string;
  offline: string;
  error: string;
}

export const api = {
  recents: () => invoke<RecentItem[]>("get_recents"),
  copyBack: (id: number) => invoke<void>("copy_back", { id }),
  log: () => invoke<LogEntry[]>("get_log"),
  hotkey: () => invoke<HotkeyState>("get_hotkey_state"),
  trayState: () => invoke<TrayState>("get_tray_state"),
  setTexts: (texts: TrayTexts) => invoke<void>("set_texts", { texts }),
  secure: (action: "write" | "read" | "delete") => invoke<SecureResult>("secure_action", { action }),
  copyDiagnostics: () => invoke<SecureResult>("copy_diagnostics"),
};

/** Rust 端状态有变化时通知界面重新拉取。 */
export function onChanged(handler: () => void): Promise<() => void> {
  return listen("onepaste://changed", handler);
}
