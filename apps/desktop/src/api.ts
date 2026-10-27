import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TrayState = "synced" | "syncing" | "offline" | "error" | "local";
export type Mode = "unset" | "local_only" | "synced";
export type HotkeyState = "registered" | "busy" | "unknown";

export type Filter = "all" | "favorites" | "text" | "images" | "links" | "files" | "code";
export type Page = "history" | "devices" | "settings";

/** What the application reports about the vault and the sync. */
export interface AppStatus {
  joined: boolean;
  mode: Mode;
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

export interface DeviceView {
  deviceId: string;
  /** The name the user gave the device; null if it cannot be read. */
  name: string | null;
  /** Seconds since the epoch. */
  createdAt: number;
  lastSeenAt: number | null;
  revoked: boolean;
  thisDevice: boolean;
}

export interface DeviceList {
  devices: DeviceView[];
  serverUrl: string;
}

export type UpdateChannel = "stable" | "preview" | "beta";

export interface UpdateInfo {
  version: string;
  /** Plain text: it is shown as text and never as markup. */
  notes: string;
  releaseUrl: string;
}

export interface AppSettings {
  autoApplyRemote: boolean;
  recordClipboard: boolean;
  checkUpdates: boolean;
  /** null means the default for this version. */
  updateChannel: UpdateChannel | null;
  skippedUpdate: string | null;
}

export interface LogEntry {
  tsMs: number;
  kind: string;
  detail: string | null;
}

/** `"system"` to follow the system, or a language tag such as `"zh"` or `"pt-br"`. */
export type LanguagePreference = string;

export interface LanguageState {
  preference: LanguagePreference;
  /** The language tag the UI should try; the UI maps it to the closest translation it has. */
  effective: string;
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
  quit: string;
  statusLine: string;
  tooltip: string;
  synced: string;
  syncing: string;
  offline: string;
  error: string;
  local: string;
  paused: string;
  quickPanel: string;
  pause: string;
  resume: string;
  syncNow: string;
  devices: string;
  settings: string;
  devicesCount: string;
}

export const api = {
  join: (code: string, passphrase: string, deviceName: string) => invoke<void>("app_join", { code, passphrase, deviceName }),
  firstRunState: () => invoke<Mode>("app_first_run_state"),
  chooseLocalOnly: () => invoke<void>("app_choose_local_only"),
  probeServer: (code: string) => invoke<"existing" | "empty">("app_probe_server", { code }),
  generatePassphrase: () => invoke<string>("app_generate_passphrase"),
  createVault: (code: string, passphrase: string, deviceName: string) => invoke<string>("app_create_vault", { code, passphrase, deviceName }),
  confirmKitSaved: () => invoke<void>("app_confirm_kit_saved"),
  cancelSetup: () => invoke<void>("app_cancel_setup"),
  joinWithKit: (code: string, kit: string, deviceName: string) => invoke<void>("app_join_with_kit", { code, kit, deviceName }),
  showKit: () => invoke<string>("app_show_kit"),
  wipeLocalData: () => invoke<void>("app_wipe_local_data"),
  /** Puts text on the clipboard so that it is neither recorded nor offered to clipboard history tools. */
  copySecret: (text: string) => invoke<void>("app_copy_secret", { text }),
  devices: () => invoke<DeviceList>("app_devices"),
  revokeDevice: (deviceId: string) => invoke<void>("app_revoke_device", { deviceId }),
  newConnectionCode: () => invoke<string>("app_new_connection_code"),
  hideQuickPanel: () => invoke<void>("hide_quick_panel"),
  setDeviceCount: (count: number | null) => invoke<void>("set_device_count", { count }),
  /** The version and the build id, for example `0.0.1 (v0.0.1-main.7)`. */
  version: () => invoke<string>("app_version"),
  checkUpdate: (manual: boolean) => invoke<UpdateInfo | null>("app_check_update", { manual }),
  availableUpdate: () => invoke<UpdateInfo | null>("app_available_update"),
  skipUpdate: (version: string) => invoke<void>("app_skip_update", { version }),
  openRelease: (url: string) => invoke<void>("app_open_release", { url }),
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

/** The shell asks the window to show one of its pages (from the tray menu). */
export function onNavigate(handler: (page: Page) => void): Promise<() => void> {
  return listen<Page>("onepaste://navigate", (e) => handler(e.payload));
}

/** The quick panel was brought up again (by the hotkey or the tray menu). */
export function onQuickOpened(handler: () => void): Promise<() => void> {
  return listen("onepaste://quick-opened", handler);
}

/** A background check found a newer version. */
export function onUpdateAvailable(handler: (info: UpdateInfo) => void): Promise<() => void> {
  return listen<UpdateInfo>("onepaste://update-available", (e) => handler(e.payload));
}
