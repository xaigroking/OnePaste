import { useEffect, useState } from "react";
import { api } from "../api";
import { useTranslation } from "react-i18next";
import type { ActionResult, AppSettings, AppStatus, HotkeyInfo, LanguagePreference, LanguageState, LogEntry } from "../api";
import { applyTheme, formatClock, loadTheme } from "../format";
import { LANGUAGES, languageInfo, resolveLanguage } from "../i18n";
import type { ThemeChoice } from "../format";
import { Badge, Button, Card, Chip, Switch } from "./ui";
import { SecurityCard } from "./SecurityCard";
import { UpdatesCard } from "./UpdatesCard";
import { ChangeServerButton } from "./ChangeServer";
import { HotkeyEditor } from "./HotkeyEditor";

interface Props {
  settings: AppSettings | null;
  onSettings: (s: AppSettings) => Promise<void>;
  language: LanguageState | null;
  onLanguage: (p: LanguagePreference) => void;
  hotkey: HotkeyInfo | null;
  onHotkey: (info: HotkeyInfo) => void;
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
  const [savingSecrets, setSavingSecrets] = useState(false);
  const [secretsSaveFailed, setSecretsSaveFailed] = useState(false);
  const [savingLimit, setSavingLimit] = useState(false);
  const [limitSaveFailed, setLimitSaveFailed] = useState(false);
  const [serverUrl, setServerUrl] = useState<string | null>();
  const [serverRevision, setServerRevision] = useState(0);
  useEffect(() => {
    if (p.status?.mode !== "synced") return;
    let active = true;
    setServerUrl(undefined);
    void api.currentServerUrl().then((url) => { if (active) setServerUrl(url); }).catch(() => { if (active) setServerUrl(null); });
    return () => { active = false; };
  }, [p.status?.mode, p.status?.deviceId, serverRevision]);
  const changeSecrets = async (value: boolean) => {
    if (!p.settings || savingSecrets) return;
    setSavingSecrets(true);
    setSecretsSaveFailed(false);
    try {
      await p.onSettings({ ...p.settings, skipSuspectedSecrets: value });
    } catch {
      setSecretsSaveFailed(true);
    } finally {
      setSavingSecrets(false);
    }
  };
  const preference = p.language?.preference ?? "system";
  const changeLimit = async (value: number) => {
    if (!p.settings || savingLimit) return;
    setSavingLimit(true); setLimitSaveFailed(false);
    try { await p.onSettings({ ...p.settings, maxItemMiB: value }); }
    catch { setLimitSaveFailed(true); }
    finally { setSavingLimit(false); }
  };
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
          <Switch
            checked={p.settings.skipSuspectedSecrets}
            disabled={savingSecrets}
            onChange={(v) => void changeSecrets(v)}
            label={t("settingsMore.skipSecrets")}
            hint={t("settingsMore.skipSecretsHint")}
          />
          {secretsSaveFailed && <p className="notice notice-warn" role="alert">{t("settingsMore.saveFailed")}</p>}
          <details>
            <summary>{t("settingsMore.skipSecretsScope")}</summary>
            <p className="muted small">{t("settingsMore.skipSecretsRules")}</p>
            <p className="muted small">{t("settingsMore.skipSecretsBoundary")}</p>
          </details>
          <Switch checked disabled onChange={() => undefined} label={t("settingsMore.respectMarker")} hint={t("settingsMore.soon")} />
          <div className="setting setting-soon">
            <span>{t("settingsMore.excludedApps")}</span>
            <Badge>{t("settingsMore.soon")}</Badge>
          </div>
          <label className="setting">
            <span>{t("settingsMore.sizeLimit")}</span>
            <select value={p.settings.maxItemMiB} disabled={savingLimit} onChange={(e) => void changeLimit(Number(e.target.value))}>
              {[1, 2, 4, 8].map((size) => <option key={size} value={size}>{t("settingsMore.sizeLimitOption", { size })}</option>)}
            </select>
          </label>
          <p className="muted small">{t("settingsMore.sizeLimitHint")}</p>
          {limitSaveFailed && <p className="notice notice-warn" role="alert">{t("settingsMore.saveFailed")}</p>}
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
        {p.status?.mode === "synced" && <>
          <div className="setting">
            <span>{t("changeServer.serverLabel")}</span>
            <span className="muted small">{serverUrl === undefined ? t("changeServer.reading") : serverUrl ?? t("changeServer.addressUnavailable")}</span>
          </div>
          <div className="setting"><ChangeServerButton onChanged={() => setServerRevision((value) => value + 1)} /></div>
        </>}
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
        <HotkeyEditor value={p.hotkey} onChanged={p.onHotkey} />
      </Card>

      {p.settings && <UpdatesCard settings={p.settings} onSettings={p.onSettings} version={p.version} />}

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
