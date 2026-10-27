import { useTranslation } from "react-i18next";
import type { AppStatus, TrayState } from "../api";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { Badge } from "./ui";
import type { Tone } from "./ui";

const TRAY_TONE: Record<TrayState, Tone> = { synced: "ok", syncing: "neutral", offline: "warn", error: "danger", local: "neutral" };
const TRAY_ICON: Record<TrayState, IconName> = { synced: "check", syncing: "refresh", offline: "wifi-off", error: "alert", local: "monitor" };

/** The strip above the page: whether OnePaste is listening to the clipboard, and how the sync is doing. */
export function TopBar({ tray, status, recording }: { tray: TrayState; status: AppStatus | null; recording: boolean }) {
  const { t } = useTranslation();
  return (
    <div className="topbar">
      {recording ? <Badge tone="ok">{t("status.listening")}</Badge> : <Badge tone="warn">{t("status.paused")}</Badge>}
      <Badge tone={TRAY_TONE[tray]} icon={<Icon name={TRAY_ICON[tray]} size={13} />}>
        {t(`status.${tray}`)}
      </Badge>
      {status && status.pending > 0 && <span className="muted small">{t("status.pendingCount", { count: status.pending })}</span>}
    </div>
  );
}
