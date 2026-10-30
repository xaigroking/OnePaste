import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { HotkeyInfo } from "../api";
import { formatHotkey } from "../hotkeys";

/**
 * A drawing of the tray menu for the screenshot tests. The real menu is drawn by the operating system, so a browser
 * cannot show it: this page lays out the same texts, from the same translation files, in the same order.
 */
export function TrayPreview() {
  const { t } = useTranslation();
  const [hotkey, setHotkey] = useState<HotkeyInfo | null>(null);
  useEffect(() => { void api.hotkey().then(setHotkey).catch(() => setHotkey(null)); }, []);
  const keys = hotkey?.current ? formatHotkey(hotkey.current, navigator.userAgent.includes("Mac")) : null;
  return (
    <div className="tray-preview" role="menu" aria-label={t("app.title")}>
      <div className="tray-status">
        <span className="badge badge-ok">{t("status.synced")}</span>
        <span className="muted small">{t("tray.devicesCount").replace("{count}", "3")}</span>
      </div>
      <hr />
      <div role="menuitem" className="tray-item">{t("tray.open")}</div>
      <div role="menuitem" className="tray-item">
        {t("tray.quickPanel")}
        {keys && <kbd>{keys}</kbd>}
      </div>
      <hr />
      <div role="menuitem" className="tray-item">{t("tray.pause")}</div>
      <div role="menuitem" className="tray-item">{t("tray.syncNow")}</div>
      <hr />
      <div role="menuitem" className="tray-item">{t("tray.devices")}</div>
      <div role="menuitem" className="tray-item">{t("tray.settings")}</div>
      <hr />
      <div role="menuitem" className="tray-item">{t("tray.quit")}</div>
    </div>
  );
}
