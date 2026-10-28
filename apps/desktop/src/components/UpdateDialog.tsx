import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { UpdateInfo } from "../api";
import { Button } from "./ui";

/**
 * "A new version is available": the version, a short note, and three buttons. The note is shown as text, never as markup.
 * Escape means "later".
 */
export function UpdateDialog({ info, onUpdate, onSkip, onLater }: { info: UpdateInfo; onUpdate: () => void; onSkip: () => void; onLater: () => void }) {
  const { t } = useTranslation();
  const first = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    first.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onLater();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onLater]);

  return (
    <div className="modal-backdrop">
      <div className="modal card" role="dialog" aria-modal="true" aria-labelledby="update-title">
        <h2 id="update-title">{t("update.title")}</h2>
        <p>{t("update.version", { version: info.version })}</p>
        {info.notes && <pre className="update-notes">{info.notes}</pre>}
        <div className="row modal-actions">
          <button ref={first} type="button" className="btn btn-primary" onClick={onUpdate}>
            {t("update.update")}
          </button>
          <Button onClick={onSkip}>{t("update.skip")}</Button>
          <Button onClick={onLater}>{t("update.later")}</Button>
        </div>
      </div>
    </div>
  );
}
