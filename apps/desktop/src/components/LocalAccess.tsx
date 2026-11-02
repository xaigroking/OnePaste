import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode, FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged, onLocked } from "../api";
import { clearImageCache } from "../imageCache";
import type { KeyState } from "../api";
import { passphraseStrength } from "../onboarding";
import { Button } from "./ui";

/** Access is resolved before rendering any history or first-run flow. A failed read never becomes a fresh install. */
export function LocalAccessGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [state, setState] = useState<KeyState | null>(null);
  const [failed, setFailed] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const sequence = useRef(0);
  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    try {
      const value = await api.keyState();
      if (request !== sequence.current) return;
      if (!["new", "prepared", "locked", "unlocked", "recovery"].includes(value)) throw Error("invalid access state");
      if (value === "locked" || value === "recovery") clearImageCache();
      setState(value as KeyState); setFailed(false);
    } catch { if (request === sequence.current) setFailed(true); }
  }, []);
  useEffect(() => {
    let live = true;
    let stopLock: (() => void) | undefined;
    void onLocked(() => { sequence.current++; clearImageCache(); setState("locked"); setFailed(false); }).then((off) => { if (live) stopLock = off; else off(); });
    let stop: (() => void) | undefined;
    void refresh();
    void onChanged(() => void refresh()).then((off) => { if (live) stop = off; else off(); });
    return () => { live = false; sequence.current++; stop?.(); stopLock?.(); };
  }, [refresh]);
  useEffect(() => {
    let last = 0;
    const activity = (event: Event) => {
      if (!event.isTrusted || Date.now() - last < 1000) return;
      last = Date.now(); void api.userActivity().catch(() => undefined);
    };
    for (const name of ["keydown", "pointerdown", "pointermove", "wheel"]) window.addEventListener(name, activity, { capture: true, passive: true });
    return () => { for (const name of ["keydown", "pointerdown", "pointermove", "wheel"]) window.removeEventListener(name, activity, true); };
  }, []);
  if (restarting) return <StartFresh onDone={async () => { await refresh(); setRestarting(false); }} onCancel={() => { setRestarting(false); requestAnimationFrame(() => document.querySelector<HTMLButtonElement>("[data-start-fresh]")?.focus()); }} />;
  if (failed) return <div className="join-wrap local-access"><section className="card join-card"><p role="alert">{t("localAccess.readFailed")}</p><Button onClick={() => void refresh()}>{t("localAccess.retry")}</Button><Button data-start-fresh onClick={() => setRestarting(true)}>{t("localAccess.startFresh")}</Button></section></div>;
  if (state === null) return <p role="status">{t("localAccess.loading")}</p>;
  if (state === "prepared" || state === "unlocked") return <>{children}</>;
  return <LocalAccessForm key={state} state={state} onDone={refresh} onRestart={() => setRestarting(true)} />;
}

function LocalAccessForm({ state, onDone, onRestart }: { state: KeyState; onDone: () => Promise<void>; onRestart: () => void }) {
  const { t } = useTranslation();
  const [recovering, setRecovering] = useState(false);
  const [pass, setPass] = useState("");
  const [again, setAgain] = useState("");
  const [credential, setCredential] = useState("");
  const [isKit, setIsKit] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fresh = state === "new";
  const creating = fresh || recovering;
  const recoveryCredential = isKit ? credential.trim() : credential;
  const goodPass = !creating || (Array.from(pass).length >= 8 && pass === again);
  const cancel = () => { setPass(""); setAgain(""); setCredential(""); setError(null); setRecovering(false); };
  const submit = async (event: FormEvent) => {
    event.preventDefault(); if (busy || !pass || !goodPass || (recovering && !recoveryCredential)) return;
    setBusy(true); setError(null);
    try {
      if (fresh) await api.prepareLocalPassphrase(pass);
      else if (recovering) await api.recoverLocal(recoveryCredential, isKit, pass);
      else await api.unlockLocal(pass);
      setPass(""); setAgain(""); setCredential(""); await onDone();
    } catch (e) { setPass(""); setAgain(""); setError(String(e) === "wrong_passphrase" ? "wrong" : String(e) === "wrong_recovery_kit" ? "wrongKit" : "failed"); }
    finally { setBusy(false); }
  };
  return <div className="join-wrap local-access"><section className="card join-card" aria-labelledby="local-access-title">
    <h1 id="local-access-title">{t(fresh ? "localAccess.setupTitle" : recovering || state === "recovery" ? "localAccess.recoveryTitle" : "localAccess.unlockTitle")}</h1>
    <p>{t("localAccess.scope")}</p>
    {state === "locked" && <p role="status">{t("autoLock.paused")}</p>}
    {state === "recovery" && <p className="notice notice-warn">{t("localAccess.legacy")}</p>}
    {(recovering || state === "recovery") && <p className="muted">{t("localAccess.limit")}</p>}
    {(fresh || recovering || state === "locked") && <form onSubmit={(event) => void submit(event)}>
      {recovering && <>
        <label className="field"><span>{t("localAccess.credentialType")}</span><select value={isKit ? "kit" : "vault"} disabled={busy} onChange={(event) => { setIsKit(event.target.value === "kit"); setCredential(""); }}><option value="kit">{t("localAccess.kit")}</option><option value="vault">{t("localAccess.vaultPass")}</option></select></label>
        <label className="field"><span>{t(isKit ? "localAccess.kit" : "localAccess.vaultPass")}</span><input type="password" autoFocus={recovering} value={credential} onChange={(event) => setCredential(event.target.value)} autoComplete="off" disabled={busy} /></label>
      </>}
      <label className="field"><span>{t(creating ? "localAccess.newPass" : "localAccess.pass")}</span><input type="password" autoFocus={!recovering} autoComplete="off" value={pass} disabled={busy} onChange={(event) => setPass(event.target.value)} /></label>
      {creating && <>
        {pass && <p className={`strength ${passphraseStrength(pass)}`}>{t(`onboarding.passphraseStep.strength.${passphraseStrength(pass)}`)}</p>}
        <label className="field"><span>{t("localAccess.confirm")}</span><input type="password" autoComplete="off" value={again} disabled={busy} onChange={(event) => setAgain(event.target.value)} /></label>
        <p className="muted small">{t("localAccess.minimum")}</p>
      </>}
      <div className="row"><Button type="submit" disabled={busy || !pass || !goodPass || (recovering && !recoveryCredential)}>{t(busy ? "localAccess.working" : fresh ? "localAccess.continue" : recovering ? "localAccess.recover" : "localAccess.unlock")}</Button><Button disabled={busy} onClick={cancel}>{t("localAccess.cancel")}</Button></div>
    </form>}
    {!fresh && !recovering && <Button onClick={() => { setPass(""); setAgain(""); setRecovering(true); setError(null); }}>{t("localAccess.recover")}</Button>}
    {!fresh && <Button data-start-fresh disabled={busy} onClick={onRestart}>{t("localAccess.startFresh")}</Button>}
    {error && <p className="notice notice-warn" role="alert">{t(`localAccess.${error}`)}</p>}
  </section></div>;
}

function StartFresh({ onDone, onCancel }: { onDone: () => Promise<void>; onCancel: () => void }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  return <div className="join-wrap local-access"><section className="card join-card" aria-labelledby="start-fresh-title" onKeyDown={(event) => { if (event.key === "Escape" && !busy) onCancel(); }}>
    <h1 id="start-fresh-title">{t("localAccess.startFreshTitle")}</h1>
    <p>{t("localAccess.startFreshBody")}</p>
    <div className="row">
      <Button autoFocus disabled={busy} onClick={onCancel}>{t("localAccess.cancel")}</Button>
      <Button variant="primary" disabled={busy} onClick={() => {
        setBusy(true); setFailed(false);
        void api.restartLocalSetup().then(onDone).catch(() => setFailed(true)).finally(() => setBusy(false));
      }}>{t(busy ? "localAccess.working" : "localAccess.startFresh")}</Button>
    </div>
    {failed && <p role="alert">{t("localAccess.startFreshFailed")}</p>}
  </section></div>;
}
