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

const IS_MAC = navigator.userAgent.includes("Mac");
const HOTKEY_TEXT = IS_MAC ? "⌃⌥V" : "Ctrl+Alt+V";
const PAGE_SIZE = 50;
const FILTERS: Filter[] = ["all", "favorites", "text", "images"];

function formatTime(tsMs: number): string {
  return new Date(tsMs).toLocaleTimeString([], { hour12: false });
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

/** The joining form: connection code, passphrase, device name. The passphrase is used once and never stored. */
function JoinForm({ onJoined }: { onJoined: () => void }) {
  const { t } = useTranslation();
  const [code, setCode] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.join(code.trim(), passphrase, deviceName.trim() || "OnePaste desktop");
      setPassphrase("");
      onJoined();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section>
      <h2>{t("join.title")}</h2>
      <p className="muted">{t("join.intro")}</p>
      <label className="field">
        <span>{t("join.code")}</span>
        <input value={code} onChange={(e) => setCode(e.target.value)} spellCheck={false} autoComplete="off" />
      </label>
      <label className="field">
        <span>{t("join.passphrase")}</span>
        <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="off" />
      </label>
      <label className="field">
        <span>{t("join.deviceName")}</span>
        <input value={deviceName} onChange={(e) => setDeviceName(e.target.value)} autoComplete="off" />
      </label>
      <div className="row">
        <button type="button" disabled={busy || !code.trim() || !passphrase} onClick={() => void submit()}>
          {busy ? t("join.working") : t("join.submit")}
        </button>
      </div>
      {error && <p className="banner">{t(`join.errors.${error}`, { defaultValue: t("join.errors.other", { code: error }) })}</p>}
    </section>
  );
}

export default function App() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [items, setItems] = useState<ItemView[]>([]);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [hotkey, setHotkey] = useState<HotkeyState>("unknown");
  const [tray, setTray] = useState<TrayState>("synced");
  const [diag, setDiag] = useState<ActionResult | null>(null);
  const [language, setLanguage] = useState<LanguageState | null>(null);
  // The newest search wins: an older, slower answer must not overwrite it
  const searchSeq = useRef(0);

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
  const act = async (work: Promise<void>) => {
    try {
      await work;
    } finally {
      void refresh();
    }
  };
  const changeSetting = async (next: AppSettings) => {
    await api.setSettings(next);
    setSettings(next);
  };

  const joined = status?.joined ?? false;
  const emptyKey = query.trim() || filter !== "all" ? "results.noMatch" : "results.empty";

  return (
    <main>
      <h1>{t("app.title")}</h1>
      {hotkey === "busy" && <p className="banner">{t("hotkey.busy")}</p>}
      <p className="muted">{t("hotkey.hint", { keys: HOTKEY_TEXT })}</p>

      <label className="row language">
        <span>{t("language.label")}</span>
        <select
          value={language?.preference ?? "system"}
          onChange={(e) => void changeLanguage(e.target.value as LanguagePreference)}
        >
          <option value="system">{t("language.system")}</option>
          <option value="zh">{t("language.zh")}</option>
          <option value="en">{t("language.en")}</option>
        </select>
      </label>

      {status && !joined && <JoinForm onJoined={() => void refresh()} />}

      {joined && status && (
        <>
          {status.paused && (
            <p className="banner">
              {t(`paused.${status.paused}`, { defaultValue: t("paused.other", { code: status.paused }) })}
            </p>
          )}
          {(status.conflict > 0 || status.failed > 0) && (
            <p className="banner">{t("queue.stuck", { conflict: status.conflict, failed: status.failed })}</p>
          )}

          <section>
            <div className="row">
              <input
                className="grow"
                type="search"
                placeholder={t("search.placeholder")}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <select value={filter} onChange={(e) => setFilter(e.target.value as Filter)}>
                {FILTERS.map((f) => (
                  <option key={f} value={f}>
                    {t(`search.filter.${f}`)}
                  </option>
                ))}
              </select>
            </div>
            {items.length === 0 ? (
              <p className="muted">{t(emptyKey)}</p>
            ) : (
              <ul className="list items">
                {items.map((it) => (
                  <li key={it.itemId}>
                    <span className="text">
                      {it.kind === "image"
                        ? t("item.image", { width: it.width, height: it.height, size: formatSize(it.size) })
                        : it.preview}
                      {it.note && <em className="note"> — {it.note}</em>}
                    </span>
                    <span className="actions">
                      <button type="button" onClick={() => void act(api.copy(it.itemId))}>
                        {t("item.copy")}
                      </button>
                      <button
                        type="button"
                        aria-pressed={it.favorite}
                        onClick={() => void act(api.setFlags(it.itemId, { favorite: !it.favorite }))}
                      >
                        {it.favorite ? t("item.unfavorite") : t("item.favorite")}
                      </button>
                      <button
                        type="button"
                        aria-pressed={it.pinned}
                        onClick={() => void act(api.setFlags(it.itemId, { pinned: !it.pinned }))}
                      >
                        {it.pinned ? t("item.unpin") : t("item.pin")}
                      </button>
                      <button type="button" onClick={() => void act(api.remove(it.itemId))}>
                        {t("item.delete")}
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          {settings && (
            <section>
              <h2>{t("settings.title")}</h2>
              <label className="row">
                <input
                  type="checkbox"
                  checked={settings.autoApplyRemote}
                  onChange={(e) => void changeSetting({ ...settings, autoApplyRemote: e.target.checked })}
                />
                <span>{t("settings.autoApply")}</span>
              </label>
              <label className="row">
                <input
                  type="checkbox"
                  checked={settings.recordClipboard}
                  onChange={(e) => void changeSetting({ ...settings, recordClipboard: e.target.checked })}
                />
                <span>{t("settings.record")}</span>
              </label>
            </section>
          )}
        </>
      )}

      <section>
        <h2>{t("log.title")}</h2>
        {log.length === 0 ? (
          <p className="muted">{t("log.empty")}</p>
        ) : (
          <ul className="list log">
            {[...log].reverse().map((e, i) => (
              <li key={`${e.tsMs}-${i}`}>
                <span className="time">{formatTime(e.tsMs)}</span>
                <span className="text">{t(`log.${e.kind}`, { defaultValue: e.kind })}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <footer>
        <span>{t("status.label", { state: t(`status.${tray}`) })}</span>
        <button type="button" onClick={() => void runDiagnostics()}>
          {t("diagnostics.copy")}
        </button>
        {diag && <span>{diag.ok ? t("diagnostics.copied") : t("diagnostics.failed", { code: diag.code })}</span>}
      </footer>
    </main>
  );
}
