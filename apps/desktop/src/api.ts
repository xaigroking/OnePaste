import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TrayState = "synced" | "syncing" | "offline" | "error";
export type HotkeyState = "registered" | "busy" | "unknown";

export type Filter = "all" | "favorites" | "text" | "images";

/** What the application reports about the vault and the sync. */
export interface AppStatus {
  joined: boolean;
  tray: TrayState;
  /** "revoked" or "server_rollback" when syncing is stopped. */
  paused: string | null;
  pending: number;
  conflict: number;
  failed: number;
  /** Number of items in the history. */
  items: number;
  deviceId: string;
}

export interface ItemView {
  itemId: string;
  kind: "text" | "image";
  /** First line of a text item. */
  preview: string;
  favorite: boolean;
  pinned: boolean;
  note: string;
  width: number;
  height: number;
  /** Size in bytes of an image. */
  size: number;
  /** Optional: shown when the backend reports where and when an item was copied. */
  sourceDevice?: string;
  sourceApp?: string;
  createdAtMs?: number;
  /** Optional per-item sync state. */
  syncState?: "synced" | "pending" | "local";
}

export interface AppSettings {
  autoApplyRemote: boolean;
  recordClipboard: boolean;
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

export interface ActionResult {
  ok: boolean;
  code: string;
}

/** Tray texts: supplied by the UI layer through i18next; the Rust side contains no visible text. */
export interface TrayTexts {
  open: string;
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
  join: (code: string, passphrase: string, deviceName: string) => invoke<void>("app_join", { code, passphrase, deviceName }),
  status: () => invoke<AppStatus>("app_status"),
  search: (query: string, filter: Filter, limit: number, offset: number) =>
    invoke<ItemView[]>("app_search", { query, filter, limit, offset }),
  copy: (itemId: string) => invoke<void>("app_copy", { itemId }),
  setFlags: (itemId: string, flags: { favorite?: boolean; pinned?: boolean; note?: string }) =>
    invoke<void>("app_set_flags", { itemId, favorite: flags.favorite ?? null, pinned: flags.pinned ?? null, note: flags.note ?? null }),
  remove: (itemId: string) => invoke<void>("app_delete", { itemId }),
  settings: () => invoke<AppSettings>("app_get_settings"),
  setSettings: (settings: AppSettings) => invoke<void>("app_set_settings", { settings }),
  log: () => invoke<LogEntry[]>("get_log"),
  hotkey: () => invoke<HotkeyState>("get_hotkey_state"),
  trayState: () => invoke<TrayState>("get_tray_state"),
  setTexts: (texts: TrayTexts) => invoke<void>("set_texts", { texts }),
  copyDiagnostics: () => invoke<ActionResult>("copy_diagnostics"),
  languageState: () => invoke<LanguageState>("get_language_state"),
  setLanguage: (preference: LanguagePreference) => invoke<LanguageState>("set_language", { preference }),
};

/** Notifies the UI to fetch again when the Rust-side state changed. */
export function onChanged(handler: () => void): Promise<() => void> {
  return listen("onepaste://changed", handler);
}
