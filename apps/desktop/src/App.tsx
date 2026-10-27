import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged } from "./api";
import type {
  ActionResult,
  AppSettings,
  AppStatus,
  Filter,
  HotkeyState,
  ItemView,
  LanguagePreference,
  LanguageState,
  LogEntry,
  TrayState,
  TrayTexts,
} from "./api";
import { applyTheme, loadTheme } from "./format";
import { DevicesView } from "./components/DevicesView";
import { HistoryView } from "./components/HistoryView";
import { JoinForm } from "./components/JoinForm";
import { SettingsView } from "./components/SettingsView";
import { Sidebar } from "./components/Sidebar";
import type { View } from "./components/Sidebar";

const IS_MAC = navigator.userAgent.includes("Mac");
const HOTKEY_TEXT = IS_MAC ? "⌃⌥V" : "Ctrl+Alt+V";
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
  const [hotkey, setHotkey] = useState<HotkeyState>("unknown");
  const [tray, setTray] = useState<TrayState>("synced");
  const [diag, setDiag] = useState<ActionResult | null>(null);
  const [language, setLanguage] = useState<LanguageState | null>(null);
  // The newest search wins: an older, slower answer must not overwrite it
  const searchSeq = useRef(0);

  useEffect(() => {
    applyTheme(loadTheme());
  }, []);

  const refresh = useCallback(async () => {
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
      try {
        const found = await api.search(query, filter, PAGE_SIZE, 0);
        if (seq === searchSeq.current) setItems(found);
      } catch {
        if (seq === searchSeq.current) setItems([]);
      }
    }
  }, [query, filter]);

  // Switch the UI language as soon as the Rust side reports a different one.
  useEffect(() => {
    if (language && language.effective !== i18n.language) {
      void i18n.changeLanguage(language.effective);
    }
  }, [language, i18n]);

  useEffect(() => {
    const texts: TrayTexts = {
      open: t("tray.open"),
      diagnostics: t("tray.diagnostics"),
      quit: t("tray.quit"),
      statusLine: t("tray.statusLine"),
      tooltip: t("tray.tooltip"),
      synced: t("status.synced"),
      syncing: t("status.syncing"),
      offline: t("status.offline"),
      error: t("status.error"),
    };
    void api.setTexts(texts);
  }, [t]);

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
  const activeView: View = joined ? view : "settings";

  if (status && !joined && view !== "settings") {
    return (
      <div className="app app-solo">
        <JoinForm onJoined={() => void refresh()} />
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar
        view={activeView}
        onView={setView}
        tray={tray}
        status={status}
        recording={settings?.recordClipboard ?? true}
        joined={joined}
      />
      <main className="content">
        {joined && status?.paused && (
          <p className="notice notice-danger" role="alert">
            {t(`paused.${status.paused}`, { defaultValue: t("paused.other", { code: status.paused }) })}
          </p>
        )}
        {joined && status && (status.conflict > 0 || status.failed > 0) && (
          <p className="notice notice-warn">{t("queue.stuck", { conflict: status.conflict, failed: status.failed })}</p>
        )}
        {activeView === "history" && (
          <HistoryView items={items} query={query} filter={filter} onQuery={setQuery} onFilter={setFilter} act={act} />
        )}
        {activeView === "devices" && <DevicesView status={status} />}
        {activeView === "settings" && (
          <SettingsView
            settings={settings}
            onSettings={(s) => void changeSetting(s)}
            language={language}
            onLanguage={(p) => void changeLanguage(p)}
            hotkey={hotkey}
            hotkeyText={HOTKEY_TEXT}
            status={status}
            log={log}
            diag={diag}
            onDiagnostics={() => void runDiagnostics()}
          />
        )}
      </main>
    </div>
  );
}
