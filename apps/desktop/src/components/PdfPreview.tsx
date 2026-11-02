import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { AnnotationMode, getDocument, GlobalWorkerOptions } from "pdfjs-dist/legacy/build/pdf.mjs";
import type { PDFDocumentProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { Button } from "./ui";
GlobalWorkerOptions.workerSrc = workerUrl;

export default function PdfPreview({ data }: { data: Uint8Array }) {
  const { t } = useTranslation();
  const canvas = useRef<HTMLCanvasElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy | null>(null);
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [rendering, setRendering] = useState(true);
  useEffect(() => {
    let active = true;
    // Only verified memory bytes enter the parser. No URL, annotation layer, XFA or script execution.
    const task = getDocument({ data, enableXfa: false, useSystemFonts: false, useWorkerFetch: false, isOffscreenCanvasSupported: false,
      maxImageSize: 16_000_000, canvasMaxAreaInBytes: 64_000_000, stopAtErrors: true, disableFontFace: true });
    task.onPassword = () => { if (active) setError("preview_encrypted"); void task.destroy(); };
    void task.promise.then((value) => {
      if (!active) return;
      if (value.numPages > 500) { setError("preview_limit"); void task.destroy(); return; }
      setDocument(value);
    }).catch(() => { if (active) setError((value) => value ?? "preview_corrupt"); });
    return () => { active = false; void task.destroy(); };
  }, [data]);
  useEffect(() => {
    if (!document || !canvas.current) return;
    let active = true;
    let renderingTask: ReturnType<Awaited<ReturnType<PDFDocumentProxy["getPage"]>>["render"]> | undefined;
    setRendering(true); setLabel("");
    void document.getPage(page).then(async (value) => {
      if (!active || !canvas.current) return;
      const viewport = value.getViewport({ scale: zoom });
      if (viewport.width * viewport.height > 16_000_000) throw "preview_limit";
      const target = canvas.current;
      target.width = Math.ceil(viewport.width); target.height = Math.ceil(viewport.height);
      renderingTask = value.render({ canvas: target, viewport, annotationMode: AnnotationMode.DISABLE });
      await renderingTask.promise;
      const text = await value.getTextContent();
      if (active) { setLabel(text.items.map((item) => "str" in item ? item.str : "").join(" ")); setRendering(false); }
    }).catch((cause) => { if (active) { setError(cause === "preview_limit" ? cause : "preview_corrupt"); setRendering(false); } });
    return () => { active = false; renderingTask?.cancel(); };
  }, [document, page, zoom]);
  useEffect(() => () => { if (canvas.current) { canvas.current.width = 0; canvas.current.height = 0; } }, []);
  if (error) return <p role="alert" className="notice notice-danger">{t(`files.errors.${error}`)}</p>;
  return <div className="pdf-preview">
    {document && <div className="row pdf-controls">
      <Button disabled={page <= 1 || rendering} onClick={() => setPage((value) => value - 1)}>{t("files.preview.previous")}</Button>
      <span role="status">{t("files.preview.page", { page, total: document.numPages })}</span>
      <Button disabled={page >= document.numPages || rendering} onClick={() => setPage((value) => value + 1)}>{t("files.preview.next")}</Button>
      <label>{t("files.preview.zoom")} <select value={zoom} onChange={(event) => setZoom(Number(event.target.value))}>{[0.5,0.75,1,1.25,1.5,2].map((value) => <option key={value} value={value}>{value * 100}%</option>)}</select></label>
    </div>}
    {rendering && <p role="status">{t("files.preview.loading")}</p>}
    <div className="pdf-canvas"><canvas ref={canvas} role="img" aria-label={label || t("files.preview.page", { page, total: document?.numPages ?? "…" })} /></div>
  </div>;
}
