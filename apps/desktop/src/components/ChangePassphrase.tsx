import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { passphraseStrength } from "../onboarding";
import { Button } from "./ui";

const ERRORS = ["wrong_passphrase", "passphrase_required", "passphrase_pending", "passphrase_stale", "unreachable", "revoked", "rate_limited", "storage", "wrong_key", "not_joined"];

export function ChangePassphraseButton() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<boolean | null>(null);
  const [readError, setReadError] = useState(false);
  useEffect(() => { let live = true; void api.passphrasePending().then((value) => { if (live) setPending(value); }).catch(() => { if (live) setReadError(true); }); return () => { live = false; }; }, []);
  return <div>
    {pending && <p className="notice notice-warn" role="status">{t("changePassphrase.pending")}</p>}
    {readError && <p className="notice notice-warn" role="alert">{t("changePassphrase.errors.other")}</p>}
    <div className="setting"><span>{t("settingsMore.changePassphrase")}</span><Button disabled={pending === null} onClick={() => setOpen(true)}>{t("changePassphrase.action")}</Button></div>
    {open && <Dialog pendingInitially={pending === true} onPending={setPending} onClose={() => setOpen(false)} />}
  </div>;
}

function Dialog({ pendingInitially, onPending, onClose }: { pendingInitially: boolean; onPending: (value: boolean) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [old, setOld] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [revealed, setRevealed] = useState(false);
  const [pending, setPending] = useState(pendingInitially);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  useEffect(() => () => { if (opener.current?.isConnected) opener.current.focus(); }, []);
  useEffect(() => { if (busy) { dialog.current?.focus(); return; } dialog.current?.querySelector<HTMLElement>("input, button:not(:disabled)")?.focus(); }, [pending, done, busy]);
  const submit = async () => {
    setBusy(true); setError(null);
    try {
      if (pending) await api.resumePassphraseChange(); else await api.changePassphrase(old, next);
      setOld(""); setNext(""); setAgain(""); setPending(false); onPending(false); setDone(true);
    } catch (failure) {
      setError(ERRORS.includes(String(failure)) ? String(failure) : "other");
      try { const value = await api.passphrasePending(); setPending(value); onPending(value); } catch { setError("other"); }
    } finally { setBusy(false); }
  };
  const keyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); if (!busy) onClose(); }
    if (event.key !== "Tab") return;
    const targets = Array.from(dialog.current?.querySelectorAll<HTMLElement>("input:not(:disabled), button:not(:disabled)") ?? []);
    const first = targets[0], last = targets.at(-1);
    if (!targets.length) { event.preventDefault(); dialog.current?.focus(); }
    else if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && (document.activeElement === last || document.activeElement === dialog.current)) { event.preventDefault(); first?.focus(); }
  };
  return <div className="modal-backdrop"><div className="modal card" role="dialog" aria-modal="true" aria-labelledby="passphrase-title" ref={dialog} tabIndex={-1} onKeyDown={keyboard}>
    <h2 id="passphrase-title">{t("changePassphrase.title")}</h2>
    {done ? <>
      <p role="status">{t("changePassphrase.done")}</p>
      <Button onClick={onClose}>{t("changePassphrase.close")}</Button>
    </> : <>
      <p className="muted">{t("changePassphrase.devices")}</p>
      <p className="muted">{t("changePassphrase.recovery")}</p>
      {pending ? <p className="notice notice-warn">{t("changePassphrase.pending")}</p> : <>
        <label className="field"><span>{t("changePassphrase.old")}</span><input type="password" autoComplete="current-password" disabled={busy} value={old} onChange={(e) => setOld(e.target.value)} /></label>
        <label className="field"><span>{t("changePassphrase.new")}</span><input type={revealed ? "text" : "password"} autoComplete="new-password" disabled={busy} value={next} onChange={(e) => setNext(e.target.value)} /></label>
        {next && <p className={`strength ${passphraseStrength(next)}`}>{t(`onboarding.passphraseStep.strength.${passphraseStrength(next)}`)}</p>}
        <label className="field"><span>{t("changePassphrase.again")}</span><input type={revealed ? "text" : "password"} autoComplete="new-password" disabled={busy} value={again} onChange={(e) => setAgain(e.target.value)} /></label>
        {again && again !== next && <p className="notice notice-warn">{t("onboarding.passphraseStep.mismatch")}</p>}
        <div className="row">
          <Button disabled={busy} aria-pressed={revealed} onClick={() => setRevealed(!revealed)}>{t(revealed ? "changePassphrase.hide" : "changePassphrase.show")}</Button>
          <Button disabled={busy} onClick={() => {
            setBusy(true); setError(null);
            void api.generatePassphrase().then((value) => { setNext(value); setAgain(value); setRevealed(true); }).catch(() => setError("other")).finally(() => setBusy(false));
          }}>{t("onboarding.passphraseStep.generate")}</Button>
          <Button disabled={busy || !next} onClick={() => { void api.copySecret(next).catch(() => setError("other")); }}>{t("changePassphrase.copy")}</Button>
        </div>
      </>}
      {error && <p className="notice notice-warn" role="alert">{t(`changePassphrase.errors.${error}`)}</p>}
      {busy && <p role="status">{t("changePassphrase.working")}</p>}
      <div className="row modal-actions">
        <Button variant="primary" disabled={busy || (!pending && (!old || !next || next !== again))} onClick={() => void submit()}>{t(pending ? "changePassphrase.retry" : "changePassphrase.save")}</Button>
        <Button disabled={busy} onClick={onClose}>{t("changePassphrase.cancel")}</Button>
      </div>
    </>}
  </div></div>;
}
