import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActionResult, AppSettings, AppStatus, HotkeyState, LanguagePreference, LanguageState, LogEntry } from "../api";
import { applyTheme, formatClock, loadTheme } from "../format";
import type { ThemeChoice } from "../format";
import { Badge, Button, Card, Chip, Switch } from "./ui";

interface Props {
  settings: AppSettings | null;
  onSettings: (s: AppSettings) => void;
  language: LanguageState | null;
  onLanguage: (p: LanguagePreference) => void;
  hotkey: HotkeyState;
  hotkeyText: string;
  status: AppStatus | null;
  log: LogEntry[];
  diag: ActionResult | null;
  onDiagnostics: () => void;
}

export function SettingsView(p: Props) {
  const { t } = useTranslation();
  const [theme, setTheme] = useState<ThemeChoice>(loadTheme);
  const pickTheme = (c: ThemeChoice) => {
    applyTheme(c);
    setTheme(c);
  };

  return (
    <div className="page">
      <h1>{t("nav.settings")}</h1>

      <Card title={t("settings.general")}>
        <div className="setting">
          <span>{t("settings.appearance")}</span>
          <span className="chips">
            {(["system", "light", "dark"] as ThemeChoice[]).map((c) => (
              <Chip key={c} active={theme === c} onClick={() => pickTheme(c)}>
                {t(`settings.theme.${c}`)}
              </Chip>
            ))}
          </span>
        </div>
        <label className="setting">
          <span>{t("language.label")}</span>
          <select value={p.language?.preference ?? "system"} onChange={(e) => p.onLanguage(e.target.value as LanguagePreference)}>
            <option value="system">{t("language.system")}</option>
            <option value="zh">{t("language.zh")}</option>
            <option value="en">{t("language.en")}</option>
          </select>
        </label>
      </Card>

      {p.settings && (
        <Card title={t("settings.clipboard")}>
          <Switch
            checked={p.settings.recordClipboard}
            onChange={(v) => p.onSettings({ ...p.settings!, recordClipboard: v })}
            label={t("settings.record")}
          />
          <Switch
            checked={p.settings.autoApplyRemote}
            onChange={(v) => p.onSettings({ ...p.settings!, autoApplyRemote: v })}
            label={t("settings.autoApply")}
          />
        </Card>
      )}

      <Card title={t("settings.sync")}>
        <div className="setting">
          <span>{t("settings.syncState")}</span>
          <Badge tone={p.status?.paused ? "danger" : "ok"}>{p.status?.paused ? t("status.stopped") : t("status.active")}</Badge>
        </div>
        <div className="setting">
          <span>{t("settings.queue")}</span>
          <span className="muted">
            {t("settings.queueValue", { pending: p.status?.pending ?? 0, conflict: p.status?.conflict ?? 0, failed: p.status?.failed ?? 0 })}
          </span>
        </div>
      </Card>

      <Card title={t("settings.shortcuts")}>
        <div className="setting">
          <span>{t("settings.showWindow")}</span>
          <kbd>{p.hotkeyText}</kbd>
        </div>
        {p.hotkey === "busy" && <p className="notice notice-warn">{t("hotkey.busy")}</p>}
      </Card>

      <Card title={t("settings.privacy")}>
        <p className="muted">{t("settings.privacyBody")}</p>
        <div className="setting">
          <Button onClick={p.onDiagnostics}>{t("diagnostics.copy")}</Button>
          {p.diag && <span className="muted">{p.diag.ok ? t("diagnostics.copied") : t("diagnostics.failed", { code: p.diag.code })}</span>}
        </div>
        <details>
          <summary>{t("log.title")}</summary>
          {p.log.length === 0 ? (
            <p className="muted">{t("log.empty")}</p>
          ) : (
            <ul className="log">
              {[...p.log].reverse().map((e, i) => (
                <li key={`${e.tsMs}-${i}`}>
                  <span className="muted">{formatClock(e.tsMs)}</span>
                  <span>{t(`log.${e.kind}`, { defaultValue: e.kind })}</span>
                </li>
              ))}
            </ul>
          )}
        </details>
      </Card>

      <Card title={t("settings.about")}>
        <p className="muted">{t("settings.aboutBody")}</p>
      </Card>
    </div>
  );
}
