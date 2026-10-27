import { useTranslation } from "react-i18next";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";

export type View = "history" | "devices" | "settings";

const NAV: { id: View; icon: IconName }[] = [
  { id: "history", icon: "clipboard" },
  { id: "devices", icon: "monitor" },
  { id: "settings", icon: "settings" },
];


interface Props {
  view: View;
  onView: (v: View) => void;
  joined: boolean;
  /** Devices in the vault; null when not known or not applicable. */
  deviceCount: number | null;
}

export function Sidebar({ view, onView, joined, deviceCount }: Props) {
  const { t } = useTranslation();
  return (
    <nav className="sidebar" aria-label={t("app.title")}>
      <div className="brand">
        <Icon name="clipboard" size={20} />
        <span>{t("app.title")}</span>
      </div>
      <ul className="nav">
        {NAV.filter((n) => joined || n.id === "settings").map((n) => (
          <li key={n.id} className={n.id === "settings" ? "nav-bottom" : undefined}>
            <button
              type="button"
              className={`nav-item${view === n.id ? " nav-active" : ""}`}
              aria-current={view === n.id ? "page" : undefined}
              onClick={() => onView(n.id)}
            >
              <Icon name={n.icon} />
              <span>{t(`nav.${n.id}`)}</span>
              {n.id === "devices" && deviceCount !== null && <span className="count">{deviceCount}</span>}
            </button>
          </li>
        ))}
      </ul>
    </nav>
  );
}
