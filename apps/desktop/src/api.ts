import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TrayState = "synced" | "syncing" | "offline" | "error";
export type HotkeyState = "registered" | "busy" | "unknown";

export interface RecentItem {
  id: number;
  kind: "text" | "image";
  /** First line of the text; empty for images. */
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

export type LanguagePreference = "system" | "zh" | "en";

export interface LanguageState {
  preference: LanguagePreference;
  /** The language the UI actually uses. */
  effective: "zh" | "en";
  /** The raw system locale, if it could be read. */
  systemLocale: string | null;
}

export interface SecureResult {
  ok: boolean;
  code: string;
}

/** Tray texts: supplied by the UI layer through i18next; the Rust side contains no visible text. */
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
  languageState: () => invoke<LanguageState>("get_language_state"),
  setLanguage: (preference: LanguagePreference) => invoke<LanguageState>("set_language", { preference }),
};

/** Notifies the UI to fetch again when the Rust-side state changed. */
export function onChanged(handler: () => void): Promise<() => void> {
  return listen("onepaste://changed", handler);
}
