import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "./api";
import { Button } from "./components/ui";

export type Strength = "weak" | "fair" | "strong";

/** A rough hint, not a guarantee: length first, then the variety of characters. */
export function passphraseStrength(p: string): Strength {
  if (p.length < 8) return "weak";
  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((re) => re.test(p)).length;
  if (p.length >= 16 || (p.length >= 12 && classes >= 3)) return "strong";
  return "fair";
}

/** The three ways to use the program. The hosted service is shown but cannot be chosen yet. */
export function ChooseMode({ choice, onChoice, onNext }: { choice: "local" | "server" | null; onChoice: (c: "local" | "server") => void; onNext: () => void }) {
  const { t } = useTranslation();
  return (
    <div>
      <h2>{t("onboarding.mode.title")}</h2>
      <div role="radiogroup" className="modes">
        <label className="mode">
          <input type="radio" name="mode" value="local" checked={choice === "local"} onChange={() => onChoice("local")} />
          <span>
            <strong>{t("onboarding.mode.local")}</strong>
            <span className="muted">{t("onboarding.mode.localHint")}</span>
          </span>
        </label>
        <label className="mode">
          <input type="radio" name="mode" value="server" checked={choice === "server"} onChange={() => onChoice("server")} />
          <span>
            <strong>{t("onboarding.mode.server")}</strong>
            <span className="muted">{t("onboarding.mode.serverHint")}</span>
          </span>
        </label>
        <label className="mode unavailable">
          <input type="radio" name="mode" value="hosted" disabled />
          <span>
            <strong>{t("onboarding.mode.hosted")}</strong>
            <span className="muted">{t("onboarding.mode.hostedHint")}</span>
          </span>
        </label>
      </div>
      <div className="row">
        <Button disabled={choice === null} onClick={onNext}>
          {t("onboarding.next")}
        </Button>
      </div>
    </div>
  );
}

/** The recovery kit, shown once. "Done" stays unavailable until the user says it is saved somewhere offline. */
export function KitScreen({
  kit,
  saved,
  onSavedChange,
  onDone,
  onCopy,
  onSave,
  onPrint,
  busy,
  error,
}: {
  kit: string;
  saved: boolean;
  onSavedChange: (v: boolean) => void;
  onDone: () => void;
  onCopy: () => void;
  onSave: () => void;
  onPrint: () => void;
  busy: boolean;
  error: string | null;
}) {
  const { t } = useTranslation();
  return (
    <div className="kit-screen">
      <h2>{t("onboarding.kit.title")}</h2>
      <p className="notice notice-warn">{t("onboarding.kit.warning")}</p>
      <pre className="kit" data-testid="kit">
        {kit}
      </pre>
      <div className="row">
        <Button onClick={onCopy}>
          {t("onboarding.kit.copy")}
        </Button>
        <Button onClick={onSave}>
          {t("onboarding.kit.save")}
        </Button>
        <Button onClick={onPrint}>
          {t("onboarding.kit.print")}
        </Button>
      </div>
      <label className="row">
        <input type="checkbox" checked={saved} onChange={(e) => onSavedChange(e.target.checked)} />
        <span>{t("onboarding.kit.saved")}</span>
      </label>
      <div className="row">
        <Button disabled={!saved || busy} onClick={onDone}>
          {busy ? t("onboarding.working") : t("onboarding.kit.done")}
        </Button>
      </div>
      {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
    </div>
  );
}

type T = (key: string, options?: Record<string, unknown>) => string;

function errorText(t: T, code: string): string {
  return t(`onboarding.errors.${code}`, { defaultValue: t("onboarding.errors.other", { code }) });
}

type Step = "mode" | "code" | "unlock" | "kitJoin" | "passphrase" | "kit";

/** The first-run flow. Going back is possible everywhere before the vault is registered. */
export function Onboarding({ onDone }: { onDone: () => void }) {
  return (
    <div className="join-wrap">
      <section className="card join-card">
        <OnboardingSteps onDone={onDone} />
      </section>
    </div>
  );
}

function OnboardingSteps({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("mode");
  const [choice, setChoice] = useState<"local" | "server" | null>(null);
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const [kitInput, setKitInput] = useState("");
  const [kit, setKit] = useState("");
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const name = () => deviceName.trim() || "OnePaste desktop";
  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const back = (to: Step) => {
    setError(null);
    setStep(to);
  };
  const leaveKit = async () => {
    // The vault was only prepared in memory: going back drops it, nothing exists anywhere yet
    await api.cancelSetup();
    setKit("");
    setSaved(false);
    setPassphrase("");
    setAgain("");
    back("passphrase");
  };

  if (step === "mode") {
    return (
      <>
        <ChooseMode
          choice={choice}
          onChoice={setChoice}
          onNext={() => (choice === "local" ? void run(async () => { await api.chooseLocalOnly(); onDone(); }) : back("code"))}
        />
        {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
      </>
    );
  }

  if (step === "code") {
    return (
      <div>
        <h2>{t("onboarding.code.title")}</h2>
        <p className="muted">{t("onboarding.code.intro")}</p>
        <p className="muted">{t("onboarding.code.serverHelp")}</p>
        <label className="field">
          <span>{t("onboarding.code.code")}</span>
          <input value={code} onChange={(e) => setCode(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        <label className="field">
          <span>{t("onboarding.deviceName")}</span>
          <input value={deviceName} onChange={(e) => setDeviceName(e.target.value)} autoComplete="off" />
        </label>
        <div className="row">
          <Button onClick={() => back("mode")}>{t("onboarding.back")}</Button>
          <Button
            disabled={busy || !code.trim()}
            onClick={() =>
              void run(async () => {
                const found = await api.probeServer(code.trim());
                setStep(found === "existing" ? "unlock" : "passphrase");
              })
            }
          >
            {busy ? t("onboarding.working") : t("onboarding.next")}
          </Button>
        </div>
        {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
      </div>
    );
  }

  if (step === "unlock") {
    return (
      <div>
        <h2>{t("onboarding.unlock.title")}</h2>
        <p className="muted">{t("onboarding.unlock.intro")}</p>
        <label className="field">
          <span>{t("onboarding.passphrase")}</span>
          <input type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} autoComplete="off" />
        </label>
        <div className="row">
          <Button onClick={() => back("code")}>{t("onboarding.back")}</Button>
          <Button
            disabled={busy || !passphrase}
            onClick={() =>
              void run(async () => {
                await api.join(code.trim(), passphrase, name());
                setPassphrase("");
                onDone();
              })
            }
          >
            {busy ? t("onboarding.working") : t("onboarding.unlock.submit")}
          </Button>
        </div>
        <p>
          <button type="button" className="link" onClick={() => back("kitJoin")}>{t("onboarding.unlock.forgot")}</button>
        </p>
        {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
      </div>
    );
  }

  if (step === "kitJoin") {
    return (
      <div>
        <h2>{t("onboarding.kitJoin.title")}</h2>
        <p className="muted">{t("onboarding.kitJoin.intro")}</p>
        <label className="field">
          <span>{t("onboarding.kitJoin.label")}</span>
          <textarea className="kit-input" rows={4} value={kitInput} onChange={(e) => setKitInput(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        <div className="row">
          <Button onClick={() => back("unlock")}>{t("onboarding.back")}</Button>
          <Button
            disabled={busy || !kitInput.trim()}
            onClick={() =>
              void run(async () => {
                await api.joinWithKit(code.trim(), kitInput.trim(), name());
                setKitInput("");
                onDone();
              })
            }
          >
            {busy ? t("onboarding.working") : t("onboarding.kitJoin.submit")}
          </Button>
        </div>
        {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
      </div>
    );
  }

  if (step === "passphrase") {
    const strength = passphraseStrength(passphrase);
    const mismatch = again.length > 0 && again !== passphrase;
    return (
      <div>
        <h2>{t("onboarding.passphraseStep.title")}</h2>
        <p className="muted">{t("onboarding.passphraseStep.intro")}</p>
        <label className="field">
          <span>{t("onboarding.passphrase")}</span>
          <input type="text" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        {passphrase && <p className={`strength ${strength}`}>{t(`onboarding.passphraseStep.strength.${strength}`)}</p>}
        <label className="field">
          <span>{t("onboarding.passphraseStep.again")}</span>
          <input type="text" value={again} onChange={(e) => setAgain(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        {mismatch && <p className="notice notice-warn">{t("onboarding.passphraseStep.mismatch")}</p>}
        <div className="row">
          <Button
            onClick={() =>
              void run(async () => {
                const p = await api.generatePassphrase();
                setPassphrase(p);
                setAgain(p);
              })
            }
          >
            {t("onboarding.passphraseStep.generate")}
          </Button>
        </div>
        <div className="row">
          <Button onClick={() => back("code")}>{t("onboarding.back")}</Button>
          <Button
            disabled={busy || !passphrase || passphrase !== again}
            onClick={() =>
              void run(async () => {
                setKit(await api.createVault(code.trim(), passphrase, name()));
                setSaved(false);
                setStep("kit");
              })
            }
          >
            {busy ? t("onboarding.working") : t("onboarding.next")}
          </Button>
        </div>
        {error && <p className="notice notice-warn">{errorText(t, error)}</p>}
      </div>
    );
  }

  return (
    <>
      <KitScreen
        kit={kit}
        saved={saved}
        onSavedChange={setSaved}
        busy={busy}
        error={error}
        onCopy={() => void api.copySecret(kit).then(() => setCopied(true))}
        onSave={() => saveKitFile(kit)}
        onPrint={() => window.print()}
        onDone={() =>
          void run(async () => {
            await api.confirmKitSaved();
            // Nothing of the kit stays in the window
            setKit("");
            setPassphrase("");
            setAgain("");
            onDone();
          })
        }
      />
      {copied && <p className="muted">{t("onboarding.kit.copied")}</p>}
      <p>
        <button type="button" className="link" onClick={() => void leaveKit()}>{t("onboarding.back")}</button>
      </p>
    </>
  );
}

/** Offers the kit as a text file download. */
export function saveKitFile(kit: string) {
  const url = URL.createObjectURL(new Blob([`${kit}\n`], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = "onepaste-recovery-kit.txt";
  link.click();
  URL.revokeObjectURL(url);
}
