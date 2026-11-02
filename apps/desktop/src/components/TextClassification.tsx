import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { ItemView } from "../api";
import { Button } from "./ui";

const LANGUAGES = ["plain", "yaml", "toml", "json", "rust", "javascript", "typescript", "python", "shell", "sql", "html", "css", "other"];

export function TextClassification({ item, act }: { item: ItemView; act: (work: Promise<void>) => Promise<void> }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState(item.kind);
  const [language, setLanguage] = useState(item.language ?? "other");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const changed = kind !== item.kind || kind === "code" && language !== item.language;
  return <div className="classification-fields"><Button aria-expanded={open} onClick={() => setOpen(!open)}>{t("classification.adjust")}</Button>{open && <>
    <label className="field"><span>{t("classification.type")}</span><select aria-label={t("classification.type")} value={kind} disabled={busy} onChange={(event) => setKind(event.target.value as ItemView["kind"])}>
      <option value="text">{t("search.filter.text")}</option><option value="link">{t("search.filter.links")}</option><option value="code">{t("search.filter.code")}</option>
    </select></label>
    {kind === "code" && <label className="field"><span>{t("classification.language")}</span><select aria-label={t("classification.language")} value={language} disabled={busy} onChange={(event) => setLanguage(event.target.value)}>
      {LANGUAGES.map((value) => <option key={value} value={value}>{value === "plain" || value === "other" ? t(`classification.${value}`) : value.toUpperCase()}</option>)}
    </select></label>}
    {changed && <Button disabled={busy} onClick={() => {
      setBusy(true); setError(false);
      void api.setTextClassification(item.itemId, kind, kind === "code" ? language : null)
        .then(() => act(Promise.resolve())).catch(() => setError(true)).finally(() => setBusy(false));
    }}>{t(busy ? "classification.saving" : "classification.save")}</Button>}
    {error && <p role="alert" className="notice notice-danger">{t("classification.failed")}</p>}
  </>}</div>;
}
