import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AppSettings } from "../api";
import { Button } from "./ui";
export function AutoLock({ settings, onSettings }: { settings: AppSettings; onSettings: (value: AppSettings) => Promise<void> }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<"save" | "lock" | null>(null);
  const [error, setError] = useState(false);
  async function act(work: () => Promise<void>, mode: "save" | "lock") {
    setBusy(mode); setError(false);
    try { await work(); } catch { setError(true); } finally { setBusy(null); }
  }
  return <section aria-label={t("settingsMore.autoLock")}>
    <label className="field"><span>{t("settingsMore.autoLock")}</span>
      <select disabled={busy !== null} value={settings.autoLockMinutes ?? 0} onChange={(event) => { const minutes = Number(event.target.value); void act(() => onSettings({ ...settings, autoLockMinutes: minutes }), "save"); }}>
        {[0, 1, 5, 15, 30].map((minutes) => <option key={minutes} value={minutes}>{t(minutes ? "autoLock.minutes" : "autoLock.off", { count: minutes })}</option>)}
      </select>
    </label>
    <p className="muted small">{t("autoLock.hint")}</p>
    <Button disabled={busy !== null} onClick={() => void act(api.lockSession, "lock")}>{t(busy === "lock" ? "autoLock.working" : "autoLock.now")}</Button>
    {busy === "save" && <p role="status">{t("autoLock.saving")}</p>}
    {error && <p role="alert">{t("autoLock.failed")}</p>}
  </section>;
}
