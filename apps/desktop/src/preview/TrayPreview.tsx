import { useTranslation } from "react-i18next";

/**
 * A drawing of the tray menu for the screenshot tests. The real menu is drawn by the operating system, so a browser
 * cannot show it: this page lays out the same texts, from the same translation files, in the same order.
 */
export function TrayPreview() {
  const { t } = useTranslation();
  const keys = navigator.userAgent.includes("Mac") ? "⌃⌥V" : "Ctrl+Alt+V";
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
        <kbd>{keys}</kbd>
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
