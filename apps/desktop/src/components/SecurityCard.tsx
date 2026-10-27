import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Button, Card } from "./ui";

/** Security and privacy: show the recovery kit again, and clear what this computer holds. Both ask first. */
export function SecurityCard({ localOnly, onWiped }: { localOnly: boolean; onWiped: () => void }) {
  const { t } = useTranslation();
  const [asking, setAsking] = useState(false);
  const [kit, setKit] = useState<string | null>(null);
  const [wiping, setWiping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reveal = async () => {
    try {
      setKit(await api.showKit());
    } catch (e) {
      setError(String(e));
    }
  };
  const wipe = async () => {
    try {
      await api.wipeLocalData();
      setWiping(false);
      onWiped();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <Card title={t("security.title")}>
      {!localOnly &&
        (kit === null ? (
          !asking ? (
            <div className="setting">
              <span>{t("security.showKit")}</span>
              <Button onClick={() => setAsking(true)}>{t("security.showKit")}</Button>
            </div>
          ) : (
            <>
              <p className="notice notice-warn">{t("security.showKitWarning")}</p>
              <div className="setting">
                <Button onClick={() => void reveal()}>{t("security.showKitConfirm")}</Button>
                <Button onClick={() => setAsking(false)}>{t("security.cancel")}</Button>
              </div>
            </>
          )
        ) : (
          <>
            <pre className="kit">{kit}</pre>
            <div className="setting">
              <Button onClick={() => void api.copySecret(kit)}>{t("onboarding.kit.copy")}</Button>
              <Button
                onClick={() => {
                  setKit(null);
                  setAsking(false);
                }}
              >
                {t("security.hideKit")}
              </Button>
            </div>
          </>
        ))}
      {!wiping ? (
        <div className="setting">
          <span>{t("security.wipe")}</span>
          <Button variant="danger" onClick={() => setWiping(true)}>
            {t("security.wipe")}
          </Button>
        </div>
      ) : (
        <>
          <p className="notice notice-danger">{t(localOnly ? "security.wipeWarningLocal" : "security.wipeWarning")}</p>
          <div className="setting">
            <Button variant="danger" onClick={() => void wipe()}>
              {t("security.wipeConfirm")}
            </Button>
            <Button onClick={() => setWiping(false)}>{t("security.cancel")}</Button>
          </div>
        </>
      )}
      {error && <p className="notice notice-danger">{t("security.failed", { code: error })}</p>}
    </Card>
  );
}
