import { useState } from "react";
import { api } from "../api";
import { useTranslation } from "react-i18next";
import type { AppStatus, TrayState } from "../api";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { Badge, Button } from "./ui";
import type { Tone } from "./ui";

const TRAY_TONE: Record<TrayState, Tone> = { synced: "ok", syncing: "neutral", offline: "warn", error: "danger", local: "neutral" };
const TRAY_ICON: Record<TrayState, IconName> = { synced: "check", syncing: "refresh", offline: "wifi-off", error: "alert", local: "monitor" };

/** The strip above the page: whether OnePaste is listening to the clipboard, and how the sync is doing. */
export function TopBar({ tray, status, recording }: { tray: TrayState; status: AppStatus | null; recording: boolean }) {
  const { t } = useTranslation();
  const [retrying, setRetrying] = useState(false);
  const [retryFailed, setRetryFailed] = useState(false);
  const state = status?.capture?.state ?? "starting";
  const failed = state === "start_failed" || state === "read_error" || state === "stopped";
  const tone: Tone = !recording ? "warn" : state === "listening" ? "ok" : failed ? "danger" : "neutral";
  return (
    <div className="topbar">
      <Badge tone={tone}>{t(!recording ? "status.paused" : state === "listening" ? "status.listening" : `capture.${state}`)}</Badge>
      {recording && failed && <Button disabled={retrying} onClick={() => {
        setRetrying(true); setRetryFailed(false);
        void api.retryClipboardMonitor().catch(() => setRetryFailed(true)).finally(() => setRetrying(false));
      }}>{t("capture.retry")}</Button>}
      {retrying && <span role="status">{t("capture.retrying")}</span>}
      {retryFailed && <span role="alert">{t("capture.retryFailed")}</span>}
      <Badge tone={TRAY_TONE[tray]} icon={<Icon name={TRAY_ICON[tray]} size={13} />}>
        {t(`status.${tray}`)}
      </Badge>
      {status && status.pending > 0 && <span className="muted small">{t("status.pendingCount", { count: status.pending })}</span>}
    </div>
  );
}
