import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged, onCloseNotice, onNavigate, onUpdateAvailable } from "./api";
import type {
  ActionResult,
  AppSettings,
  AppStatus,
  Filter,
  HotkeyInfo,
  ItemView,
  UpdateInfo,
  LanguagePreference,
  LanguageState,
  LogEntry,
  TrayState,
  TrayTexts,
} from "./api";
import { applyTheme, loadTheme } from "./format";
import { applyDocumentLanguage, resolveLanguage } from "./i18n";
import { DevicesView } from "./components/DevicesView";
import { HistoryView } from "./components/HistoryView";
import { Onboarding } from "./onboarding";
import { SettingsView } from "./components/SettingsView";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { CloseNotice } from "./components/CloseNotice";
import { UpdateDialog } from "./components/UpdateDialog";
import { Button } from "./components/ui";
import type { View } from "./components/Sidebar";
import { formatHotkey } from "./hotkeys";

const IS_MAC = navigator.userAgent.includes("Mac");
const PAGE_SIZE = 50;

export default function App() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [items, setItems] = useState<ItemView[]>([]);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [view, setView] = useState<View>("history");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [dismissedSizeNotice, setDismissedSizeNotice] = useState(0);
  const [hotkey, setHotkey] = useState<HotkeyInfo | null>(null);
  const hotkeyText = hotkey?.current ? formatHotkey(hotkey.current, IS_MAC) : null;
  const [tray, setTray] = useState<TrayState>("synced");
  const [diag, setDiag] = useState<ActionResult | null>(null);
  const [language, setLanguage] = useState<LanguageState | null>(null);
  const [itemsState, setItemsState] = useState<"loading" | "ready" | "error">("loading");
  const [itemsError, setItemsError] = useState("");
  const [deviceCount, setDeviceCount] = useState<number | null>(null);
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [closing, setClosing] = useState(false);
  // The newest search wins: an older, slower answer must not overwrite it
  const searchSeq = useRef(0);

  useEffect(() => {
    applyTheme(loadTheme());
  }, []);

  const refresh = useCallback(async () => {
    // Opens what is already set up before anything is asked, so the first-run screens do not flash up on a normal start
    await api.firstRunState().catch(() => null);
    const [st, l, h, tr, lang, se] = await Promise.all([
      api.status(),
      api.log(),
      api.hotkey(),
      api.trayState(),
      api.languageState(),
      api.settings(),
    ]);
    setStatus(st);
    setLog(l);
    setHotkey(h);
    setTray(tr);
    setLanguage(lang);
    setSettings(se);
    if (st.joined) {
      const seq = ++searchSeq.current;
      // Links, files and code have no type of their own in the history yet: those chips show nothing
      if (filter === "links" || filter === "files" || filter === "code") {
        setItems([]);
        setItemsState("ready");
        return;
      }
      try {
        const found = await api.search(query, filter, PAGE_SIZE, 0);
        if (seq === searchSeq.current) {
          setItems(found);
          setItemsState("ready");
        }
      } catch (e) {
        if (seq === searchSeq.current) {
          setItems([]);
          setItemsError(String(e));
          setItemsState("error");
        }
      }
    }
  }, [query, filter]);

  // Switch the UI language as soon as the Rust side reports a different one.
  useEffect(() => {
    if (!language) return;
    const next = resolveLanguage(language.effective);
    if (next !== i18n.language) void i18n.changeLanguage(next);
    applyDocumentLanguage(next);
  }, [language, i18n]);

  useEffect(() => {
    void api.version().then(setVersion).catch(() => undefined);
  }, []);

  useEffect(() => {
    const texts: TrayTexts = {
      open: t("tray.open"),
      quickPanel: hotkeyText ? `${t("tray.quickPanel")} (${hotkeyText})` : t("tray.quickPanel"),
      pause: t("tray.pause"),
      resume: t("tray.resume"),
      syncNow: t("tray.syncNow"),
      devices: t("tray.devices"),
      settings: t("tray.settings"),
      devicesCount: t("tray.devicesCount"),
      paused: t("status.paused"),
      quit: t("tray.quit"),
      statusLine: t("tray.statusLine"),
      tooltip: t("tray.tooltip"),
      synced: t("status.synced"),
      syncing: t("status.syncing"),
      offline: t("status.offline"),
      error: t("status.error"),
      local: t("status.local"),
    };
    void api.setTexts(texts);
  }, [t, hotkeyText]);

  // A newer version: one found before the window opened, and one a background check finds later. A failed check says nothing.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void api.availableUpdate().then(setUpdate).catch(() => undefined);
    void onUpdateAvailable(setUpdate).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, []);

  // The window was asked to close: the first time, the user is told the program keeps running in the tray
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onCloseNotice(() => setClosing(true)).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, []);

  // The tray menu asks for a page ("Devices...", "Settings...")
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onNavigate((page) => setView(page)).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, []);

  // The tray's status line shows the number of devices too
  useEffect(() => {
    void api.setDeviceCount(deviceCount).catch(() => undefined);
  }, [deviceCount]);

  // The number next to "Devices" in the sidebar; only a vault on a server has devices
  const syncedVault = status?.mode === "synced";
  useEffect(() => {
    if (!syncedVault) {
      setDeviceCount(null);
      return;
    }
    const load = () => void api.devices().then((l) => setDeviceCount(l.devices.filter((d) => !d.revoked).length)).catch(() => undefined);
    load();
    const timer = window.setInterval(load, 30000);
    return () => window.clearInterval(timer);
  }, [syncedVault]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onChanged(() => void refresh()).then((u) => {
      unlisten = u;
    });
    void refresh();
    // A safety net for changes that do not announce themselves
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => {
      unlisten?.();
      window.clearInterval(timer);
    };
  }, [refresh]);

  const changeLanguage = async (preference: LanguagePreference) => setLanguage(await api.setLanguage(preference));
  const runDiagnostics = async () => setDiag(await api.copyDiagnostics());
  const act = useCallback(
    async (work: Promise<void>) => {
      try {
        await work;
      } finally {
        void refresh();
      }
    },
    [refresh],
  );
  const changeSetting = async (next: AppSettings) => {
    await api.setSettings(next);
    setSettings(next);
  };

  const joined = status?.joined ?? false;
  const rejectedCopy = log.reduce((latest, entry) => entry.kind === "capture_failed" && entry.detail === "item_too_large" ? Math.max(latest, entry.tsMs) : latest, 0);
  const activeView: View = joined ? view : "settings";

  if (status && !joined && view !== "settings") {
    return (
      <div className="app app-solo">
        <Onboarding onDone={() => void refresh()} />
      </div>
    );
  }

  const updateDialog = update && (
    <UpdateDialog
      info={update}
      onUpdate={() => {
        void api.openRelease(update.releaseUrl);
        setUpdate(null);
      }}
      onSkip={() => {
        void api.skipUpdate(update.version);
        setUpdate(null);
      }}
      onLater={() => setUpdate(null)}
    />
  );

  return (
    <div className="app">
      {updateDialog}
      {closing && (
        <CloseNotice
          onHide={(again) => {
            setClosing(false);
            void api.closeToTray(again);
          }}
        />
      )}
      <Sidebar
        view={activeView}
        onView={setView}
        joined={joined}
        deviceCount={deviceCount}
      />
      <main className="content">
        {joined && <TopBar tray={tray} status={status} recording={settings?.recordClipboard ?? true} />}
        {joined && rejectedCopy > dismissedSizeNotice && <div className="notice notice-warn" role="alert">
          <p>{t("settingsMore.sizeLimitNotice")}</p>
          <Button onClick={() => setDismissedSizeNotice(rejectedCopy)}>{t("settingsMore.dismissSizeNotice")}</Button>
        </div>}
        {joined && status?.paused && (
          <p className="notice notice-danger" role="alert">
            {t(`paused.${status.paused}`, { defaultValue: t("paused.other", { code: status.paused }) })}
          </p>
        )}
        {joined && status && (status.conflict > 0 || status.failed > 0) && (
          <p className="notice notice-warn">{t("queue.stuck", { conflict: status.conflict, failed: status.failed })}</p>
        )}
        {activeView === "history" && (
          <HistoryView
            localOnly={status?.mode === "local_only"}
            items={items}
            query={query}
            filter={filter}
            onQuery={setQuery}
            onFilter={setFilter}
            act={act}
            loadState={itemsState}
            loadError={itemsError}
            previewHeight={settings?.textPreviewHeight ?? null}
            onPreviewHeight={(px) => settings && void changeSetting({ ...settings, textPreviewHeight: px })}
          />
        )}
        {activeView === "devices" && <DevicesView status={status} onCount={setDeviceCount} />}
        {activeView === "settings" && (
          <SettingsView
            settings={settings}
            onSettings={changeSetting}
            language={language}
            onLanguage={(p) => void changeLanguage(p)}
            hotkey={hotkey}
            onHotkey={(info) => { setHotkey(info); void refresh(); }}
            status={status}
            version={version}
            localOnly={status?.mode === "local_only"}
            onWiped={() => void refresh()}
            log={log}
            diag={diag}
            onDiagnostics={() => void runDiagnostics()}
          />
        )}
      </main>
    </div>
  );
}
