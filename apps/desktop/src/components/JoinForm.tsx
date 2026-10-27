import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Button } from "./ui";
import { Icon } from "./Icon";

/** The joining form: connection code, passphrase, device name. The passphrase is used once and never stored. */
export function JoinForm({ onJoined }: { onJoined: () => void }) {
  const { t } = useTranslation();
  const [code, setCode] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.join(code.trim(), passphrase, deviceName.trim() || "OnePaste desktop");
      setPassphrase("");
      onJoined();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="join-wrap">
      <section className="card join-card">
        <div className="join-icon">
          <Icon name="lock" size={26} />
        </div>
        <h1>{t("join.title")}</h1>
        <p className="muted">{t("join.intro")}</p>
        <label className="field">
          <span>{t("join.code")}</span>
          <input value={code} onChange={(e) => setCode(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        <label className="field">
          <span>{t("join.passphrase")}</span>
          <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="off" />
        </label>
        <label className="field">
          <span>{t("join.deviceName")}</span>
          <input value={deviceName} onChange={(e) => setDeviceName(e.target.value)} autoComplete="off" />
        </label>
        {error && (
          <p className="notice notice-danger" role="alert">
            {t(`join.errors.${error}`, { defaultValue: t("join.errors.other", { code: error }) })}
          </p>
        )}
        <Button variant="primary" className="wide" disabled={busy || !code.trim() || !passphrase} onClick={() => void submit()}>
          {busy ? t("join.working") : t("join.submit")}
        </Button>
        <p className="muted small">{t("join.note")}</p>
      </section>
    </div>
  );
}
