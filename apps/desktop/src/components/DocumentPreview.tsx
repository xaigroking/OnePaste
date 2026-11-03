import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { Button } from "./ui";
const PdfPreview = lazy(() => import("./PdfPreview"));

export function DocumentPreview({ itemId, fileName }: { itemId: string; fileName: string }) {
  const { t } = useTranslation();
  const format = fileName.split(".").at(-1)?.toLowerCase() ?? "";
  const supported = ["pdf", "doc", "docx", "txt"].includes(format);
  const [opened, setOpened] = useState(false);
  const [loading, setLoading] = useState(false);
  const [text, setText] = useState<string | null>(null);
  const [pdf, setPdf] = useState<Uint8Array | null>(null);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const bytes = useRef<Uint8Array | null>(null);
  const clear = () => { if (bytes.current?.byteLength) bytes.current.fill(0); bytes.current = null; };
  useEffect(() => () => { ++generation.current; clear(); }, [itemId]);
  const load = async () => {
    const current = ++generation.current;
    setOpened(true); setLoading(true); setError(null);
    try {
      if (format === "pdf") {
        const data = await api.attachmentPdf(itemId);
        if (current !== generation.current) { if (data.byteLength) data.fill(0); return; }
        bytes.current = data; setPdf(data);
      } else {
        const result = await api.documentPreview(itemId);
        if (current === generation.current) setText(result.text);
      }
    } catch (cause) { if (current === generation.current) setError(typeof cause === "string" ? cause : "preview_corrupt"); }
    finally { if (current === generation.current) setLoading(false); }
  };
  if (!supported) return <p className="muted small">{t("files.previewUnavailable")}</p>;
  return <section className="document-preview" aria-label={t("files.preview.title")}>
    <div className="row"><Button disabled={loading} onClick={() => void load()}>{t(error ? "files.preview.retry" : "files.preview.open")}</Button>
      {opened && <Button onClick={() => { ++generation.current; setOpened(false); setText(null); setPdf(null); clear(); if (loading) void api.cancelAttachment(itemId); setLoading(false); }}>{t("files.preview.close")}</Button>}
    </div>
    {opened && <>
      <p className="muted small">{t(format === "pdf" ? "files.preview.pdfLimit" : format === "txt" ? "files.preview.textLimit" : "files.preview.wordLimit")}</p>
      {loading && <p role="status">{t("files.preview.loading")}</p>}
      {error && <p role="alert" className="notice notice-danger">{t(`files.errors.${error}`, { defaultValue: t("files.errors.preview_corrupt") })}</p>}
      {text !== null && <pre className="document-text" tabIndex={0}>{text || t("files.preview.empty")}</pre>}
      {pdf && <Suspense fallback={<p role="status">{t("files.preview.loading")}</p>}><PdfPreview data={pdf} /></Suspense>}
    </>}
  </section>;
}
