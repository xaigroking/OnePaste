import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

/** Shown once when the window is closed: the program keeps running in the tray. Escape means "hide it". */
export function CloseNotice({ onHide }: { onHide: (dontShowAgain: boolean) => void }) {
  const { t } = useTranslation();
  const [again, setAgain] = useState(false);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    button.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onHide(again);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [again, onHide]);

  return (
    <div className="modal-backdrop">
      <div className="modal card" role="dialog" aria-modal="true" aria-labelledby="close-title">
        <h2 id="close-title">{t("closeNotice.title")}</h2>
        <p>{t("closeNotice.body")}</p>
        <label className="row">
          <input type="checkbox" checked={again} onChange={(e) => setAgain(e.target.checked)} />
          <span>{t("closeNotice.dontShow")}</span>
        </label>
        <div className="row modal-actions">
          <button ref={button} type="button" className="btn btn-primary" onClick={() => onHide(again)}>
            {t("closeNotice.hide")}
          </button>
        </div>
      </div>
    </div>
  );
}
