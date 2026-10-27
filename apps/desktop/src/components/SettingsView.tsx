import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActionResult, AppSettings, AppStatus, HotkeyState, LanguagePreference, LanguageState, LogEntry } from "../api";
import { applyTheme, formatClock, loadTheme } from "../format";
import { LANGUAGES, languageInfo, resolveLanguage } from "../i18n";
import type { ThemeChoice } from "../format";
import { Badge, Button, Card, Chip, Switch } from "./ui";
import { SecurityCard } from "./SecurityCard";

interface Props {
  settings: AppSettings | null;
  onSettings: (s: AppSettings) => void;
  language: LanguageState | null;
  onLanguage: (p: LanguagePreference) => void;
  hotkey: HotkeyState;
  hotkeyText: string;
  status: AppStatus | null;
  /** This computer keeps its history to itself: there is nothing to sync or to recover. */
  localOnly: boolean;
  /** The version and the build id, for the About card. */
  version: string;
  onWiped: () => void;
  log: LogEntry[];
  diag: ActionResult | null;
  onDiagnostics: () => void;
}

export function SettingsView(p: Props) {
  const { t } = useTranslation();
  const [theme, setTheme] = useState<ThemeChoice>(loadTheme);
  const preference = p.language?.preference ?? "system";
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
          <select value={preference} onChange={(e) => p.onLanguage(e.target.value as LanguagePreference)}>
            <option value="system">{t("language.system")}</option>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>
                {l.nativeName}
              </option>
            ))}
            {/* A saved choice that has no translation (yet) stays visible instead of silently changing */}
            {preference !== "system" && !languageInfo(preference) && <option value={preference}>{preference}</option>}
          </select>
        </label>
        <Switch checked={false} disabled onChange={() => undefined} label={t("settingsMore.startup")} hint={t("settingsMore.soon")} />
        <p className="muted small">{t("settingsMore.trayNote")}</p>
        <p className="muted small">
          {preference === "system"
            ? t("language.auto", { name: languageInfo(resolveLanguage(p.language?.effective))?.nativeName })
            : t("language.manual")}
        </p>
      </Card>

      {p.settings && (
        <Card title={t("settings.clipboard")}>
          <Switch
            checked={p.settings.recordClipboard}
            onChange={(v) => p.onSettings({ ...p.settings!, recordClipboard: v })}
            label={t("settings.record")}
          />
          <Switch checked disabled onChange={() => undefined} label={t("settingsMore.skipSecrets")} hint={t("settingsMore.soon")} />
          <Switch checked disabled onChange={() => undefined} label={t("settingsMore.respectMarker")} hint={t("settingsMore.soon")} />
          <div className="setting setting-soon">
            <span>{t("settingsMore.excludedApps")}</span>
            <Badge>{t("settingsMore.soon")}</Badge>
          </div>
          <div className="setting setting-soon">
            <span>{t("settingsMore.sizeLimit")}</span>
            <span className="muted">
              {t("settingsMore.sizeLimitValue")} <Badge>{t("settingsMore.soon")}</Badge>
            </span>
          </div>
          {!p.localOnly && (
            <Switch
              checked={p.settings.autoApplyRemote}
              onChange={(v) => p.onSettings({ ...p.settings!, autoApplyRemote: v })}
              label={t("settings.autoApply")}
            />
          )}
        </Card>
      )}

      <Card title={t("settings.sync")}>
        <div className="setting">
          <span>{t("settingsMore.syncMethod")}</span>
          <span className="muted">{p.localOnly ? t("settingsMore.syncLocal") : t("settingsMore.syncServer")}</span>
        </div>
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
        <p className="muted small">{t("settingsMore.hotkeyHint")}</p>
        {p.hotkey === "busy" && <p className="notice notice-warn">{t("hotkey.busy")}</p>}
      </Card>

      <SecurityCard localOnly={p.localOnly} onWiped={p.onWiped} />

      <Card title={t("settingsMore.advanced")}>
        <div className="setting setting-soon">
          <span>{t("settingsMore.autoLock")}</span>
          <Badge>{t("settingsMore.soon")}</Badge>
        </div>
        <div className="setting setting-soon">
          <span>{t("settingsMore.changePassphrase")}</span>
          <Badge>{t("settingsMore.soon")}</Badge>
        </div>
        <div className="setting">
          <span>{t("settingsMore.telemetry")}</span>
          <span className="muted">{t("settingsMore.telemetryOff")}</span>
        </div>
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
        {p.version && <p className="muted small">{t("about.version", { version: p.version })}</p>}
      </Card>
    </div>
  );
}
