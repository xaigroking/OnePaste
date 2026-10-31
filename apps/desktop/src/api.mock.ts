// A stand-in for ./api used only by the screenshot and keyboard tests (vite build --mode mock). It serves fixed data, so
// every screen looks the same on every run. The scene comes from the address: ?lang=zh|en&state=base|empty|loading|error
// &screen=main|dev|set|qp|tray|state|ob1|ob2.
import type {
  ActionResult,
  AppSettings,
  AppStatus,
  DeviceList,
  Filter,
  HotkeyInfo,
  ImageData,
  ItemView,
  UpdateInfo,
  LanguagePreference,
  LanguageState,
  LogEntry,
  Mode,
  Page,
  TrayState,
  TrayTexts,
} from "./api";

const q = new URLSearchParams(window.location.search);
const SCREEN = q.get("screen") ?? "main";
const STATE = q.get("state") ?? "base";
const LANG = q.get("lang") === "zh" ? "zh" : "en";
const NOW = Date.UTC(2026, 9, 26, 10, 30, 0);
const MIN = 60_000;
(window as unknown as { __now: number }).__now = NOW;

const called = (name: string) => {
  const w = window as unknown as { __called?: string[] };
  w.__called = [...(w.__called ?? []), name];
};

const never = <T,>() => new Promise<T>(() => undefined);
const deviceAction = <T,>(value: T): Promise<T> => {
  if (q.get("deviceAction") !== "pending") return Promise.resolve(value);
  return new Promise<T>((resolve, reject) => {
    (window as unknown as { __finishDeviceAction?: (fail?: boolean) => void }).__finishDeviceAction = (fail) => fail ? reject("unreachable") : resolve(value);
  });
};

const ITEMS: ItemView[] = [
  { itemId: "i1", kind: "text", preview: "Review moved to 3 pm Friday, room B2 \u5468\u4e94\u8bc4\u5ba1\u4f1a\u6539\u5230\u4e0b\u5348\u4e09\u70b9", favorite: false, pinned: true, note: "", width: 0, height: 0, size: 0, sourceDevice: "Office Mac", sourceApp: "Chat", createdAtMs: NOW - 1 * MIN, syncState: "synced" },
  { itemId: "i2", kind: "text", preview: "https://example.org/help/clipboard-api", favorite: true, pinned: false, note: "read later", width: 0, height: 0, size: 0, sourceDevice: "My laptop", sourceApp: "Browser", createdAtMs: NOW - 2 * MIN, syncState: "synced" },
  { itemId: "i3", kind: "text", preview: "docker run -d -p 8787:8787 -v onepaste-data:/data onepaste/server", favorite: false, pinned: false, note: "", width: 0, height: 0, size: 0, sourceDevice: "My laptop", sourceApp: "Terminal", createdAtMs: NOW - 8 * MIN, syncState: "synced" },
  { itemId: "i4", kind: "text", preview: "••••••••••••", favorite: false, pinned: false, note: "", width: 0, height: 0, size: 0, sourceDevice: "My laptop", sourceApp: "Password manager", createdAtMs: NOW - 15 * MIN, syncState: "local" },
  { itemId: "i5", kind: "image", preview: "", favorite: false, pinned: false, note: "", width: 1280, height: 720, size: 86016, sourceDevice: "Office Mac", sourceApp: "Screenshot", createdAtMs: NOW - 60 * MIN, syncState: "synced" },
  { itemId: "i6", kind: "text", preview: "Quarterly report, final draft", favorite: false, pinned: false, note: "", width: 0, height: 0, size: 0, sourceDevice: "Office Mac", sourceApp: "Reader", createdAtMs: NOW - 90 * MIN, syncState: "pending" },
];

let activeHotkey: string | null = q.get("hotkey") === "unavailable" ? null : q.get("hotkey") === "custom" && STATE !== "error" ? "Control+Shift+KeyK" : "Control+Alt+KeyV";
let hotkeyWarning: HotkeyInfo["warning"] = !activeHotkey || SCREEN === "set" && STATE === "error" ? "fallback" : null;
const hotkeyInfo = (): HotkeyInfo => ({ state: hotkeyWarning ? "busy" : "registered", current: activeHotkey, warning: hotkeyWarning });
const settings: AppSettings = { autoApplyRemote: true, recordClipboard: true, maxItemMiB: 8, quickHotkey: q.get("hotkey") === "custom" || hotkeyWarning ? "Control+Shift+KeyK" : null, skipSuspectedSecrets: true, checkUpdates: true, updateChannel: null, skippedUpdate: null, trayNoticeDismissed: false, textPreviewHeight: null };
let serverUrl = "https://192.168.1.20:8443";

const status = (): AppStatus => {
  const joined = SCREEN !== "ob1" && SCREEN !== "ob2";
  const mode: Mode = joined ? q.get("mode") === "local" ? "local_only" : "synced" : "unset";
  const paused = SCREEN === "state" ? "revoked" : null;
  return { joined, mode, tray: SCREEN === "state" ? "error" : "synced", paused, pending: SCREEN === "state" ? 3 : 0, conflict: 0, failed: SCREEN === "state" ? 1 : 0, items: ITEMS.length, deviceId: "d1" };
};

const devices = (): DeviceList => ({
  serverUrl,
  devices: [
    { deviceId: "d1", name: "My laptop", createdAt: NOW / 1000 - 86400 * 30, lastSeenAt: NOW / 1000, revoked: false, thisDevice: true },
    { deviceId: "d2", name: "Office Mac", createdAt: NOW / 1000 - 86400 * 20, lastSeenAt: NOW / 1000 - 120, revoked: false, thisDevice: false },
    { deviceId: "d3", name: "Old phone", createdAt: NOW / 1000 - 86400 * 90, lastSeenAt: NOW / 1000 - 86400 * 12, revoked: true, thisDevice: false },
  ],
});

/** A fixed picture made in the page itself: a diagonal gradient with a frame. */
function makePicture(): ImageData {
  const canvas = document.createElement("canvas");
  canvas.width = 1280;
  canvas.height = 720;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    const g = ctx.createLinearGradient(0, 0, 1280, 720);
    g.addColorStop(0, "#0f766e");
    g.addColorStop(1, "#f59e0b");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, 1280, 720);
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 16;
    ctx.strokeRect(40, 40, 1200, 640);
    ctx.fillStyle = "#ffffff";
    ctx.font = "bold 96px sans-serif";
    ctx.fillText("1280 x 720", 380, 390);
  }
  const data = canvas.toDataURL("image/png").split(",")[1] ?? "";
  return { data, width: 1280, height: 720, size: 86016, format: "png" };
}

const LONG_TEXT = Array.from({ length: 34 }, (_, i) => `Line ${i + 1}: the quick brown fox jumps over the lazy dog`).join("\n");

const UPDATE: UpdateInfo = {
  version: "0.0.1-main.12",
  notes: "Add the update check.\nFix the devices table.\n<b>Not markup</b>",
  releaseUrl: "https://github.com/xaigroking/OnePaste/releases/tag/v0.0.1-main.12",
};

let startupEnabled = q.get("autostart") === "on";
window.addEventListener("onepaste-test-remove-autostart", () => { startupEnabled = false; });
const startupInfo = () => ({ enabled: startupEnabled, canEnable: q.get("autostart") !== "unstable", reason: q.get("autostart") === "unstable" ? "startup_unstable_path" : null });

export const api = {
  closeToTray: async (_dontShowAgain: boolean) => {
    called("closeToTray");
  },
  itemImage: async (_itemId: string): Promise<ImageData> => makePicture(),
  itemText: async (itemId: string): Promise<string> => {
    if (SCREEN === "longtext" || SCREEN === "textview") return LONG_TEXT;
    return ITEMS.find((i) => i.itemId === itemId)?.preview ?? "";
  },
  saveImage: async (_itemId: string, _fileName: string): Promise<boolean> => {
    called("saveImage");
    return true;
  },
  checkUpdate: async (_manual: boolean): Promise<UpdateInfo | null> => {
    called("checkUpdate");
    if (STATE === "error") return Promise.reject("update_unreachable");
    return STATE === "empty" ? null : UPDATE;
  },
  availableUpdate: async (): Promise<UpdateInfo | null> => (SCREEN === "update" ? UPDATE : null),
  skipUpdate: async (_version: string) => {
    called("skipUpdate");
  },
  openServerHelp: async () => {
    called("openServerHelp");
    if (q.get("help") === "error") throw "open_failed";
  },
  openRelease: async (_url: string) => {
    called("openRelease");
  },
  languageState: async (): Promise<LanguageState> => ({ preference: "system", effective: LANG, systemLocale: LANG }),
  setLanguage: async (preference: LanguagePreference): Promise<LanguageState> => ({ preference, effective: LANG, systemLocale: LANG }),
  firstRunState: async (): Promise<Mode> => status().mode,
  status: async () => status(),
  search: (query: string, filter: Filter, _limit: number, _offset: number): Promise<ItemView[]> => {
    if (STATE === "loading") return never();
    if (STATE === "error") return Promise.reject("storage");
    if (STATE === "empty") return Promise.resolve([]);
    const text = query.trim().toLowerCase();
    return Promise.resolve(
      ITEMS.filter((i) => (filter === "favorites" ? i.favorite : filter === "text" ? i.kind === "text" : filter === "images" ? i.kind === "image" : true)).filter(
        (i) => !text || i.preview.toLowerCase().includes(text),
      ),
    );
  },
  copy: async (itemId: string) => {
    (window as unknown as { __copied: string[] }).__copied = [...((window as unknown as { __copied?: string[] }).__copied ?? []), itemId];
  },
  pastePermission: async () => q.get("pastePermission") !== "no",
  openAccessibilitySettings: async () => { called("openAccessibilitySettings"); },
  quickPaste: async (itemId: string) => {
    const w = window as unknown as { __copied?: string[]; __hidden?: boolean; __pasted?: string[] };
    w.__copied = [...(w.__copied ?? []), itemId]; w.__hidden = true;
    if (q.get("pastePermission") === "no") return "copied_no_permission";
    if (q.get("pasteTarget") === "missing") return "copied_manual";
    w.__pasted = [...(w.__pasted ?? []), itemId]; return "paste_requested";
  },
  hideQuickPanel: async () => {
    (window as unknown as { __hidden: boolean }).__hidden = true;
  },
  setFlags: async () => undefined,
  remove: async () => undefined,
  settings: () => (SCREEN === "set" && STATE === "loading" ? never<AppSettings>() : Promise.resolve(settings)),
  autostart: async () => startupInfo(),
  setAutostart: async (enabled: boolean) => {
    if (q.get("autostartSave") === "error") throw "startup_write";
    startupEnabled = enabled; return startupInfo();
  },
  setSettings: async (next: AppSettings) => {
    const w = window as unknown as { __settingsCalls?: AppSettings[] };
    w.__settingsCalls = [...(w.__settingsCalls ?? []), { ...next }];
    if (q.get("settingsSave") === "error") throw "storage";
    Object.assign(settings, next);
  },
  log: async (): Promise<LogEntry[]> => [
    { tsMs: NOW - 5 * MIN, kind: "started", detail: null },
    { tsMs: NOW - 3 * MIN, kind: "captured", detail: "text, 42 bytes" },
    ...(q.has("captureLimit") ? [{ tsMs: NOW, kind: "capture_failed", detail: "item_too_large" }] : []),
  ],
  hotkey: async (): Promise<HotkeyInfo> => hotkeyInfo(),
  setHotkey: async (shortcut: string | null): Promise<HotkeyInfo> => {
    const w = window as unknown as { __hotkeyCalls?: (string | null)[] };
    w.__hotkeyCalls = [...(w.__hotkeyCalls ?? []), shortcut];
    if (q.get("hotkeySave") === "busy") throw "hotkey_busy";
    if (q.get("hotkeySave") === "storage") throw "hotkey_storage";
    activeHotkey = shortcut ?? "Control+Alt+KeyV";
    hotkeyWarning = null;
    settings.quickHotkey = shortcut;
    return hotkeyInfo();
  },
  trayState: async (): Promise<TrayState> => status().tray,
  setTexts: async (texts: TrayTexts) => { (window as unknown as { __trayTexts?: TrayTexts }).__trayTexts = { ...texts }; },
  setDeviceCount: async () => undefined,
  copyDiagnostics: async (): Promise<ActionResult> => ({ ok: true, code: "ok" }),
  devices: async (): Promise<DeviceList> => {
    if (STATE === "loading") return never();
    if (STATE === "error") return Promise.reject("unreachable");
    if (STATE === "empty") return { serverUrl: "https://192.168.1.20:8443", devices: devices().devices.slice(0, 1) };
    return devices();
  },
  revokeDevice: async () => { called("revokeDevice"); return deviceAction(undefined); },
  newConnectionCode: async () => { called("newConnectionCode"); return deviceAction("onepaste1:eyJ1IjoiaHR0cHM6Ly8xOTIuMTY4LjEuMjA6ODQ0MyIsImZwIjoiLi4uIiwidCI6Ii4uLiJ9"); },
  version: async () => "0.0.1 (v0.0.1-main.1)",
  probeServer: async () => {
    called("probeServer");
    if (q.get("serverChange") === "loading") return never<"existing" | "empty">();
    if (q.get("serverChange") === "error") return Promise.reject("unreachable");
    if (q.get("serverChangeVault") === "existing") return "existing" as const;
    if (STATE === "loading") return never<"existing" | "empty">();
    if (STATE === "error") return Promise.reject("invalid_connection_code");
    return q.get("vault") === "existing" ? "existing" as const : "empty" as const;
  },
  currentServerUrl: async () => status().mode === "synced" ? serverUrl : null,
  changeServer: async (code: string, passphrase: string | null, recoveryKit: string | null) => {
    called("changeServer");
    const w = window as unknown as { __serverChanges?: { code: string; passphrase: string | null; recoveryKit: string | null }[] };
    w.__serverChanges = [...(w.__serverChanges ?? []), { code, passphrase, recoveryKit }];
    if (q.get("serverChangeCommit") === "error") throw "wrong_passphrase";
    if (q.get("serverChangeCommit") === "unexpected") throw "onepaste1:do-not-show-error";
    serverUrl = "https://new-server.example:8443";
  },
  generatePassphrase: async () => "7K3M-QX9D-4HFA-W2JB-N8RT",
  createVault: async () => { called("createVault"); return "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789-ABCD-EFGH-JKMN-PQRS-TVWX-YZ01"; },
  confirmKitSaved: async () => { called("confirmKitSaved"); },
  cancelSetup: async () => undefined,
  chooseLocalOnly: async () => {
    called("chooseLocalOnly");
  },
  join: async () => { called("join"); },
  joinWithKit: async () => undefined,
  showKit: async () => "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789-ABCD-EFGH-JKMN-PQRS-TVWX-YZ01",
  wipeLocalData: async () => undefined,
  copySecret: async () => { called("copySecret"); },
};

export async function onChanged(_handler: () => void): Promise<() => void> {
  return () => undefined;
}

/** The scene's page is opened once, as the tray menu would. */
export async function onNavigate(handler: (page: Page) => void): Promise<() => void> {
  const page: Page | null = SCREEN === "dev" ? "devices" : SCREEN === "set" || SCREEN === "upd" ? "settings" : null;
  if (page) window.setTimeout(() => handler(page), 0);
  return () => undefined;
}

export async function onQuickOpened(_handler: () => void): Promise<() => void> {
  return () => undefined;
}

export async function onUpdateAvailable(_handler: (info: UpdateInfo) => void): Promise<() => void> {
  return () => undefined;
}

export async function onCloseNotice(handler: () => void): Promise<() => void> {
  if (SCREEN === "closenote") window.setTimeout(handler, 0);
  return () => undefined;
}
