import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged } from "../api";
import type { AttachmentInfo } from "../api";
import { formatSize } from "../format";
import { Badge, Button } from "./ui";

export function FileDetail({ itemId }: { itemId: string }) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AttachmentInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [acting, setActing] = useState(false);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const sequence = ++generation.current;
    try { const value = await api.attachmentInfo(itemId); if (sequence === generation.current) setInfo(value); }
    catch { if (sequence === generation.current) setError("attachment_unavailable"); }
  }, [itemId]);
  useEffect(() => {
    setInfo(null); setError(null); setSaved(false); void load();
    let cancelled = false; let stop: (() => void) | undefined;
    void onChanged(() => void load()).then((unlisten) => { if (cancelled) unlisten(); else stop = unlisten; });
    return () => { cancelled = true; ++generation.current; stop?.(); };
  }, [load]);
  const act = async (work: () => Promise<unknown>) => {
    setActing(true); setError(null); setSaved(false);
    try { await work(); }
    catch (error) { setError(typeof error === "string" ? error : "attachment_transfer_failed"); }
    finally { setActing(false); void load(); }
  };
  const message = (code: string) => t(`files.errors.${code}`, { defaultValue: t("files.errors.generic") });
  const busy = info && ["uploading", "downloading"].includes(info.transfer.state);
  return <section className="file-detail" aria-label={t("files.details")}>
    {!info && !error && <p role="status">{t("files.loading")}</p>}
    {info && <>
      <h3 className="file-name">{info.fileName}</h3>
      <p className="muted small">{formatSize(info.size)} · {info.mime}</p>
      <div className="row"><Badge>{t(info.cached ? "files.cached" : "files.needed")}</Badge><Badge>{t(`files.sync.${info.syncState}`)}</Badge></div>
      {busy && <p role="status">{t(info.transfer.direction === "upload" ? "files.upload" : "files.downloadProgress", { percent: Math.floor(100 * info.transfer.completed / Math.max(1, info.transfer.total)) })}</p>}
      {info.transfer.state === "cancelled" && <p role="status">{t("files.cancelled")}</p>}
      {info.transfer.error && <p role="alert" className="notice notice-danger">{message(info.transfer.error)}</p>}
      <div className="row file-actions">
        {!info.cached && <Button disabled={!!busy || acting} onClick={() => void act(() => api.prepareAttachment(itemId))}>{t("files.download")}</Button>}
        <Button disabled={!!busy || acting} onClick={() => void act(async () => { if (await api.saveAttachment(itemId)) setSaved(true); })}>{t("files.save")}</Button>
        {busy && <Button onClick={() => void api.cancelAttachment(itemId)}>{t("files.cancel")}</Button>}
        {info.transfer.direction === "upload" && ["failed", "cancelled"].includes(info.transfer.state) && <Button disabled={acting} onClick={() => void act(() => api.retryAttachment(itemId))}>{t("files.retry")}</Button>}
      </div>
      <p className="muted small">{t("files.previewUnavailable")}</p>
    </>}
    {saved && <p role="status">{t("files.saved")}</p>}
    {error && <p role="alert" className="notice notice-danger">{message(error)}</p>}
  </section>;
}

export function FileDialog({ itemId, onClose }: { itemId: string; onClose: () => void }) {
  const { t } = useTranslation();
  const root = useRef<HTMLDivElement>(null);
  const opener = useRef(document.activeElement as HTMLElement | null);
  useEffect(() => { root.current?.querySelector<HTMLButtonElement>("button")?.focus(); return () => { if (opener.current?.isConnected) opener.current.focus(); }; }, []);
  return <div className="modal-backdrop"><div className="modal card file-dialog" role="dialog" aria-modal="true" aria-labelledby="file-title" tabIndex={-1} ref={root} onKeyDown={(event) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onClose(); }
    if (event.key === "Tab") {
      const targets = Array.from(root.current?.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex='0']") ?? []);
      if (event.shiftKey && (document.activeElement === targets[0] || document.activeElement === root.current)) { event.preventDefault(); targets.at(-1)?.focus(); }
      else if (!event.shiftKey && document.activeElement === targets.at(-1)) { event.preventDefault(); targets[0]?.focus(); }
    }
  }}>
    <div className="row"><h2 id="file-title" className="grow">{t("files.details")}</h2><Button onClick={onClose}>{t("image.close")}</Button></div>
    <FileDetail itemId={itemId} />
  </div></div>;
}
