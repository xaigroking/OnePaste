import { useTranslation } from "react-i18next";
import type { AppStatus } from "../api";
import { Badge, Card } from "./ui";
import { Icon } from "./Icon";

/** The device list needs a backend call that does not exist yet; this view shows what the app knows today. */
export function DevicesView({ status }: { status: AppStatus | null }) {
  const { t } = useTranslation();
  return (
    <div className="page">
      <h1>{t("nav.devices")}</h1>
      <Card title={t("devices.thisDevice")}>
        <div className="device">
          <span className="row-icon">
            <Icon name="monitor" />
          </span>
          <span className="row-body">
            <span className="row-title">{status?.deviceId ?? "-"}</span>
            <span className="row-meta">{t("devices.idHint")}</span>
          </span>
          <Badge tone="ok">{t("devices.thisBadge")}</Badge>
        </div>
      </Card>
      <p className="muted">{t("devices.soon")}</p>
    </div>
  );
}
