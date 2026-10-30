import { useTranslation } from "react-i18next";

/** The same complete-code field is used for first setup and reconnecting an existing vault. */
export function ConnectionCodeField({ value, onChange, disabled = false, autoFocus = false }: { value: string; onChange: (value: string) => void; disabled?: boolean; autoFocus?: boolean }) {
  const { t } = useTranslation();
  return (
    <label className="field">
      <span>{t("onboarding.code.code")}</span>
      <input value={value} onChange={(e) => onChange(e.target.value)} disabled={disabled} autoFocus={autoFocus} spellCheck={false} autoComplete="off" />
    </label>
  );
}
