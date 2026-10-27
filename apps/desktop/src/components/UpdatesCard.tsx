import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AppSettings, UpdateChannel } from "../api";
import { Button, Card, Switch } from "./ui";

const CHANNELS: UpdateChannel[] = ["preview", "beta", "stable"];

/** Updates: look automatically, which releases to offer, and a button to look now. */
export function UpdatesCard({ settings, onSettings, version }: { settings: AppSettings; onSettings: (s: AppSettings) => void; version: string }) {
  const { t } = useTranslation();
  const [state, setState] = useState<"idle" | "busy" | "none" | "found" | "failed">("idle");
  const [error, setError] = useState("");
  // The default channel while the version is 0.x includes pre-releases; the choice "default" is shown as that channel
  const isZero = version.startsWith("0.");
  const channel: UpdateChannel = settings.updateChannel ?? (isZero ? "preview" : "stable");

  const check = async () => {
    setState("busy");
    try {
      setState((await api.checkUpdate(true)) ? "found" : "none");
    } catch (e) {
      setError(String(e));
      setState("failed");
    }
  };

  return (
    <Card title={t("updates.title")}>
      <Switch checked={settings.checkUpdates} onChange={(v) => onSettings({ ...settings, checkUpdates: v })} label={t("updates.auto")} hint={t("updates.autoHint")} />
      <label className="setting">
        <span>{t("updates.channel")}</span>
        <select value={channel} onChange={(e) => onSettings({ ...settings, updateChannel: e.target.value as UpdateChannel })}>
          {CHANNELS.map((c) => (
            <option key={c} value={c}>
              {t(`updates.channels.${c}`)}
            </option>
          ))}
        </select>
      </label>
      <p className="muted small">{t(`updates.channelHint.${channel}`)}</p>
      <div className="setting">
        <Button disabled={state === "busy"} onClick={() => void check()}>
          {state === "busy" ? t("updates.checking") : t("updates.checkNow")}
        </Button>
        <span className="muted" role="status">
          {state === "none" && t("updates.none")}
          {state === "found" && t("updates.found")}
          {state === "failed" && t(`updates.errors.${error}`, { defaultValue: t("updates.errors.other", { code: error }) })}
        </span>
      </div>
    </Card>
  );
}
