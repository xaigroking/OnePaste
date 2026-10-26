import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged } from "./api";
import type { HotkeyState, LogEntry, RecentItem, SecureResult, TrayState, TrayTexts } from "./api";

const IS_MAC = navigator.userAgent.includes("Mac");
const HOTKEY_TEXT = IS_MAC ? "⌃⌥V" : "Ctrl+Alt+V";

function formatTime(tsMs: number): string {
  return new Date(tsMs).toLocaleTimeString([], { hour12: false });
}

export default function App() {
  const { t } = useTranslation();
  const [recents, setRecents] = useState<RecentItem[]>([]);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [hotkey, setHotkey] = useState<HotkeyState>("unknown");
  const [tray, setTray] = useState<TrayState>("synced");
  const [secure, setSecure] = useState<SecureResult | null>(null);
  const [diag, setDiag] = useState<SecureResult | null>(null);

  const refresh = useCallback(async () => {
    const [r, l, h, s] = await Promise.all([api.recents(), api.log(), api.hotkey(), api.trayState()]);
    setRecents(r);
    setLog(l);
    setHotkey(h);
    setTray(s);
  }, []);

  useEffect(() => {
    const texts: TrayTexts = {
      open: t("tray.open"),
      toggle: t("tray.toggle"),
      diagnostics: t("tray.diagnostics"),
      quit: t("tray.quit"),
      statusLine: t("tray.statusLine"),
      tooltip: t("tray.tooltip"),
      synced: t("status.synced"),
      syncing: t("status.syncing"),
      offline: t("status.offline"),
      error: t("status.error"),
    };
    void api.setTexts(texts).then(refresh);
  }, [t, refresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onChanged(() => void refresh()).then((u) => {
      unlisten = u;
    });
    void refresh();
    return () => unlisten?.();
  }, [refresh]);

  const runSecure = async (action: "write" | "read" | "delete") => setSecure(await api.secure(action));
  const runDiagnostics = async () => setDiag(await api.copyDiagnostics());

  return (
    <main>
      <h1>{t("app.title")}</h1>
      {hotkey === "busy" && <p className="banner">{t("hotkey.busy")}</p>}
      <p className="muted">{t("hotkey.hint", { keys: HOTKEY_TEXT })}</p>

      <section>
        <h2>{t("recents.title")}</h2>
        {recents.length === 0 ? (
          <p className="muted">{t("recents.empty")}</p>
        ) : (
          <>
            <p className="muted">{t("recents.clickHint")}</p>
            <ul className="list">
              {recents.map((r) => (
                <li key={r.id}>
                  <button type="button" onClick={() => void api.copyBack(r.id)}>
                    <span className="time">{formatTime(r.tsMs)}</span>
                    <span className="text">
                      {r.kind === "image" ? t("recents.image", { width: r.width, height: r.height }) : r.preview}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
      </section>

      <section>
        <h2>{t("secure.title")}</h2>
        <div className="row">
          <button type="button" onClick={() => void runSecure("write")}>
            {t("secure.write")}
          </button>
          <button type="button" onClick={() => void runSecure("read")}>
            {t("secure.read")}
          </button>
          <button type="button" onClick={() => void runSecure("delete")}>
            {t("secure.delete")}
          </button>
        </div>
        {secure && <p>{secure.ok ? t("secure.ok") : t("secure.fail", { code: secure.code })}</p>}
      </section>

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
