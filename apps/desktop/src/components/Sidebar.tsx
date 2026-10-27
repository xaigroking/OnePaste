import { useTranslation } from "react-i18next";
import type { AppStatus, TrayState } from "../api";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { Badge } from "./ui";
import type { Tone } from "./ui";

export type View = "history" | "devices" | "settings";

const NAV: { id: View; icon: IconName }[] = [
  { id: "history", icon: "clipboard" },
  { id: "devices", icon: "monitor" },
  { id: "settings", icon: "settings" },
];

const TRAY_TONE: Record<TrayState, Tone> = { synced: "ok", syncing: "neutral", offline: "warn", error: "danger", local: "neutral" };
const TRAY_ICON: Record<TrayState, IconName> = { synced: "check", syncing: "refresh", offline: "wifi-off", error: "alert", local: "monitor" };

interface Props {
  view: View;
  onView: (v: View) => void;
  tray: TrayState;
  status: AppStatus | null;
  recording: boolean;
  joined: boolean;
}

export function Sidebar({ view, onView, tray, status, recording, joined }: Props) {
  const { t } = useTranslation();
  return (
    <nav className="sidebar" aria-label={t("app.title")}>
      <div className="brand">
        <Icon name="clipboard" size={20} />
        <span>{t("app.title")}</span>
      </div>
      <ul className="nav">
        {NAV.filter((n) => joined || n.id === "settings").map((n) => (
          <li key={n.id}>
            <button
              type="button"
              className={`nav-item${view === n.id ? " nav-active" : ""}`}
              aria-current={view === n.id ? "page" : undefined}
              onClick={() => onView(n.id)}
            >
              <Icon name={n.icon} />
              <span>{t(`nav.${n.id}`)}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="sidebar-foot">
        {joined && recording && <Badge tone="ok">{t("status.listening")}</Badge>}
        {joined && !recording && <Badge tone="warn">{t("status.paused")}</Badge>}
        <Badge tone={TRAY_TONE[tray]} icon={<Icon name={TRAY_ICON[tray]} size={13} />}>
          {t(`status.${tray}`)}
        </Badge>
        {status && status.pending > 0 && <span className="muted small">{t("status.pendingCount", { count: status.pending })}</span>}
      </div>
    </nav>
  );
}
