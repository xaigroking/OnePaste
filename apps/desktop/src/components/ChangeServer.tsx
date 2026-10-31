import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Button } from "./ui";
import { ConnectionCodeField } from "./ConnectionCodeField";

type Step = "intro" | "code" | "review" | "confirm" | "done";
const ERRORS = ["passphrase_pending","invalid_connection_code", "registration_invalid", "rate_limited", "unsupported_version", "wrong_passphrase", "wrong_recovery_kit", "wrong_key", "passphrase_required", "unreachable", "storage", "not_joined", "server_change_failed"] as const;

export function ChangeServerButton({ onChanged }: { onChanged?: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return <>
    <Button onClick={() => setOpen(true)}>{t("changeServer.action")}</Button>
    {open && <ChangeServerDialog onClose={() => setOpen(false)} onChanged={onChanged} />}
  </>;
}

function ChangeServerDialog({ onClose, onChanged }: { onClose: () => void; onChanged?: () => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("intro");
  const [code, setCode] = useState("");
  const [kind, setKind] = useState<"empty" | "existing">("empty");
  const [credential, setCredential] = useState<"passphrase" | "kit">("passphrase");
  const [passphrase, setPassphrase] = useState("");
  const [kit, setKit] = useState("");
  const [probing, setProbing] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);

  useEffect(() => () => { if (opener.current?.isConnected) opener.current.focus(); }, []);
  useEffect(() => {
    if (committing || probing) { dialog.current?.focus(); return; }
    const target = dialog.current?.querySelector<HTMLElement>("[data-first]") ?? dialog.current?.querySelector<HTMLElement>("input:not(:disabled), textarea:not(:disabled), button:not(:disabled)");
    target?.focus();
  }, [step, credential, committing, probing]);

  const report = (failure: unknown) => setError(ERRORS.find((code) => code === String(failure)) ?? "other");
  const go = (next: Step) => { setError(null); setStep(next); };
  const probe = async () => {
    setProbing(true); setError(null);
    try {
      setKind(await api.probeServer(code.trim()));
      setPassphrase(""); setKit("");
      setStep("review");
    } catch (failure) { report(failure); }
    finally { setProbing(false); }
  };
  const change = async () => {
    setCommitting(true); setError(null);
    try {
      await api.changeServer(code.trim(), kind === "existing" && credential === "passphrase" ? passphrase : null, kind === "existing" && credential === "kit" ? kit.trim() : null);
    } catch (failure) { report(failure); setCommitting(false); return; }
    setCommitting(false); setStep("done");
    onChanged?.();
  };
  const keyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault(); event.stopPropagation();
      if (!committing) onClose();
    }
    if (event.key === "Tab") {
      const focusable = Array.from(dialog.current?.querySelectorAll<HTMLElement>("input:not(:disabled), textarea:not(:disabled), button:not(:disabled)") ?? []).filter((element) => !(element instanceof HTMLInputElement && element.type === "radio" && !element.checked));
      if (focusable.length === 0) { event.preventDefault(); dialog.current?.focus(); return; }
      const first = focusable[0], last = focusable[focusable.length - 1];
      if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === dialog.current)) { event.preventDefault(); first?.focus(); }
    }
  };
  const consequences = <ul className="muted">
    <li>{t("changeServer.keepHistory")}</li>
    <li>{t("changeServer.oldDevice")}</li>
    <li>{t("changeServer.otherDevices")}</li>
  </ul>;

  return <div className="modal-backdrop">
    <div className="modal card" ref={dialog} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby="change-server-title" onKeyDown={keyboard}>
      <h2 id="change-server-title">{t(step === "done" ? "changeServer.doneTitle" : step === "confirm" ? "changeServer.confirmTitle" : "changeServer.title")}</h2>
      {step === "intro" && <>
        <p>{t("changeServer.intro")}</p>
        {consequences}
        <div className="row modal-actions">
          <Button data-first variant="primary" onClick={() => go("code")}>{t("changeServer.start")}</Button>
          <Button onClick={onClose}>{t("changeServer.cancel")}</Button>
        </div>
      </>}
      {step === "code" && <>
        <p className="muted">{t("changeServer.codeHint")}</p>
        <ConnectionCodeField value={code} onChange={setCode} disabled={probing} autoFocus />
        {probing && <p role="status">{t("changeServer.checking")}</p>}
        <div className="row modal-actions">
          <Button disabled={probing} onClick={() => go("intro")}>{t("changeServer.back")}</Button>
          <Button variant="primary" disabled={probing || !code.trim()} onClick={() => void probe()}>{t("changeServer.check")}</Button>
          <Button onClick={onClose}>{t("changeServer.cancel")}</Button>
        </div>
      </>}
      {step === "review" && <>
        <p>{t(kind === "empty" ? "changeServer.empty" : "changeServer.existing")}</p>
        <p className="muted">{t(kind === "empty" ? "changeServer.emptyDetails" : "changeServer.existingDetails")}</p>
        {kind === "existing" && <>
          <div className="row" role="radiogroup" aria-label={t("changeServer.credentials")}>
            <label><input type="radio" name="server-credential" checked={credential === "passphrase"} onChange={() => setCredential("passphrase")} /> {t("changeServer.usePassphrase")}</label>
            <label><input type="radio" name="server-credential" checked={credential === "kit"} onChange={() => setCredential("kit")} /> {t("changeServer.useKit")}</label>
          </div>
          <label className="field">
            <span>{t(credential === "passphrase" ? "changeServer.passphrase" : "changeServer.kit")}</span>
            {credential === "passphrase"
              ? <input data-first type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="off" />
              : <textarea data-first value={kit} onChange={(e) => setKit(e.target.value)} autoComplete="off" spellCheck={false} />}
          </label>
        </>}
        <div className="row modal-actions">
          <Button onClick={() => go("code")}>{t("changeServer.back")}</Button>
          <Button data-first={kind === "empty" ? "" : undefined} variant="primary" disabled={kind === "existing" && !(credential === "passphrase" ? passphrase : kit.trim())} onClick={() => go("confirm")}>{t("changeServer.next")}</Button>
          <Button onClick={onClose}>{t("changeServer.cancel")}</Button>
        </div>
      </>}
      {step === "confirm" && <>
        {consequences}
        <p className="muted">{t("changeServer.confirmHint")}</p>
        {committing && <p role="status">{t("changeServer.changing")}</p>}
        <div className="row modal-actions">
          <Button disabled={committing} onClick={() => go("review")}>{t("changeServer.back")}</Button>
          <Button data-first variant="primary" disabled={committing} onClick={() => void change()}>{t("changeServer.confirm")}</Button>
          <Button disabled={committing} onClick={onClose}>{t("changeServer.cancel")}</Button>
        </div>
      </>}
      {step === "done" && <>
        <p>{t("changeServer.doneBody")}</p>
        <p className="muted">{t("changeServer.otherDevices")}</p>
        <div className="row modal-actions"><Button data-first variant="primary" onClick={onClose}>{t("changeServer.done")}</Button></div>
      </>}
      {error && <p className="notice notice-danger" role="alert">{t(`changeServer.errors.${error}`)}</p>}
    </div>
  </div>;
}
