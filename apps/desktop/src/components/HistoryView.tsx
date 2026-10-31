import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { DeletionStatus, Filter, ItemView } from "../api";
import { formatRelative, formatSize, isLink } from "../format";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { Badge, Button, Chip } from "./ui";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { retainSelection, selectRow } from "../selection";
import type { Selection } from "../selection";
import { ImageDetail } from "./ImageDetail";
import { ImageThumb } from "./ImageThumb";
import { TextDetail } from "./TextDetail";

// Links, files and code have no type of their own in the history yet: their chips are there and show nothing
const FILTERS: Filter[] = ["all", "favorites", "text", "links", "images", "files", "code"];

export function kindIcon(it: ItemView): IconName {
  if (it.kind === "image") return "image";
  // A masked item (a secret the clipboard marked as sensitive) shows as a row of dots
  if (/^[\u2022\u25cf*]+$/.test(it.preview)) return "lock";
  return isLink(it.preview) ? "link" : "text";
}

function itemTitle(it: ItemView, t: (k: string, o?: Record<string, unknown>) => string): string {
  return it.kind === "image"
    ? t("item.image", { width: it.width, height: it.height, size: formatSize(it.size) })
    : it.preview;
}

interface Props {
  localOnly?: boolean;
  items: ItemView[];
  query: string;
  filter: Filter;
  onQuery: (q: string) => void;
  onFilter: (f: Filter) => void;
  act: (work: Promise<void>) => Promise<void>;
  loadState: "loading" | "ready" | "error";
  loadError: string;
  /** The height the user gave the text preview, and what to do when they change it. */
  previewHeight: number | null;
  onPreviewHeight: (px: number) => void;
}

export function HistoryView({ items, query, filter, onQuery, onFilter, act, loadState, loadError, previewHeight, onPreviewHeight, localOnly = false }: Props) {
  const { t, i18n } = useTranslation();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection>({ ids: [], anchor: null });
  const [confirmIds, setConfirmIds] = useState<string[] | null>(null);
  const [deletions, setDeletions] = useState<DeletionStatus>({ pending: [], failedBatches: 0 });
  const [notice, setNotice] = useState<{ key: string; count?: number; error?: boolean } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const rowRefs = useRef(new Map<string, HTMLLIElement>());
  const initialized = useRef(false);
  const pendingSequence = useRef(0);
  const mutatingPending = useRef(false);
  const previousTokens = useRef<string[]>([]);
  const failureCount = useRef(0);
  const opener = useRef<HTMLElement | null>(null);
  const order = useMemo(() => items.map((item) => item.itemId), [items]);
  const selected = useMemo(() => items.find((item) => item.itemId === selectedId) ?? items[0] ?? null, [items, selectedId]);
  const selectedItems = items.filter((item) => selection.ids.includes(item.itemId));
  const pendingIds = new Set(deletions.pending.flatMap((entry) => entry.itemIds));
  const selectedTextCount = selectedItems.filter((item) => item.kind === "text").length;

  useLayoutEffect(() => {
    if (!initialized.current && order.length) { initialized.current = true; setSelection({ ids: [order[0]], anchor: order[0] }); }
    else setSelection((value) => retainSelection(value, order));
  }, [order]);

  const refreshPending = useCallback(async () => {
    if (mutatingPending.current) return;
    const request = ++pendingSequence.current;
    try {
      const status = await api.pendingDeletions();
      if (request !== pendingSequence.current) return;
      const tokens = status.pending.map((entry) => entry.token);
      if (previousTokens.current.some((token) => !tokens.includes(token))) void act(Promise.resolve());
      previousTokens.current = tokens;
      if (status.failedBatches > failureCount.current) setNotice({ key: "deleteFailed", error: true });
      failureCount.current = status.failedBatches;
      setDeletions(status);
    } catch { if (request === pendingSequence.current) setNotice({ key: "statusFailed", error: true }); }
  }, [act]);
  useEffect(() => {
    void refreshPending();
    const timer = window.setInterval(() => void refreshPending(), 1000);
    return () => { pendingSequence.current++; window.clearInterval(timer); };
  }, [refreshPending, items]);

  const choose = useCallback((id: string, toggle = false, range = false) => {
    setSelectedId(id); setSelection((value) => selectRow(value, order, id, toggle, range));
  }, [order]);
  const focusRow = useCallback((id: string) => { rowRefs.current.get(id)?.focus(); rowRefs.current.get(id)?.scrollIntoView({ block: "nearest" }); }, []);
  const clear = () => setSelection({ ids: [], anchor: null });
  const askDelete = useCallback((ids: string[], from?: HTMLElement) => {
    opener.current = from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : searchRef.current);
    setConfirmIds([...ids]);
  }, []);
  const run = async (work: Promise<unknown>, copied = false) => {
    setNotice(null);
    try { const count = await work; if (copied) setNotice({ key: "copied", count: Number(count) }); await act(Promise.resolve()); }
    catch { setNotice({ key: "actionFailed", error: true }); }
  };
  const undo = async (token: string) => {
    mutatingPending.current = true; pendingSequence.current++;
    try { if (!await api.undoDeletion(token)) setNotice({ key: "undoExpired", error: true }); }
    catch { setNotice({ key: "actionFailed", error: true }); }
    finally { mutatingPending.current = false; void refreshPending(); }
  };
  const stage = async (ids: string[]) => {
    mutatingPending.current = true; pendingSequence.current++;
    try {
      const pending = await api.stageDeletion(ids);
      setDeletions((status) => ({ ...status, pending: [...status.pending.filter((entry) => entry.token !== pending.token), pending] }));
      clear(); setConfirmIds(null); setNotice(null);
    } finally { mutatingPending.current = false; void refreshPending(); }
  };

  useLayoutEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (document.querySelector('[aria-modal="true"]')) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") { event.preventDefault(); searchRef.current?.focus(); return; }
      if (event.key === "Escape") { if (selection.ids.length) { event.preventDefault(); setSelection({ ids: [], anchor: null }); } return; }
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest("input, textarea, button, select") || !items.length) return;
      const index = Math.max(0, items.findIndex((item) => item.itemId === selected?.itemId));
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const next = items[Math.max(0, Math.min(items.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))].itemId;
        if ((event.ctrlKey || event.metaKey) && !event.shiftKey) setSelectedId(next);
        else choose(next, event.ctrlKey || event.metaKey, event.shiftKey);
        focusRow(next);
      } else if (event.key === " " && selected) {
        event.preventDefault(); choose(selected.itemId, true, event.shiftKey);
      } else if (event.key === "Enter" && selected) {
        event.preventDefault();
        if (selection.ids.length > 1) void run(api.copyItems(selection.ids), true);
        else void act(api.copy(selected.itemId));
      } else if (event.key === "Delete" && selected) {
        event.preventDefault(); askDelete(selection.ids.length ? selection.ids : [selected.itemId]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items, selected, selection, choose, focusRow, askDelete, act]);

  const emptyKey = query.trim() || filter !== "all" ? "results.noMatch" : "results.empty";
  const multiple = selection.ids.length > 1;
  return <div className="history">
    <div className="history-main">
      <div className="searchbar"><Icon name="search" /><input ref={searchRef} type="search" placeholder={t("search.placeholder")} value={query} onChange={(event) => onQuery(event.target.value)} aria-label={t("search.placeholder")} onKeyDown={(event) => {
        if (event.key === "ArrowDown" && !event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && selected) { event.preventDefault(); focusRow(selected.itemId); }
      }} /></div>
      <div className="chips" role="group" aria-label={t("search.filterLabel")}>
        {FILTERS.map((value) => <Chip key={value} active={filter === value} onClick={() => onFilter(value)}>{value === "favorites" && <Icon name="star" size={13} />}{t(`search.filter.${value}`)}</Chip>)}
      </div>
      <p className="muted small history-selection-hint" id="history-selection-hint">{t(navigator.userAgent.includes("Mac") ? "batch.hintMac" : "batch.hint")}</p>
      {multiple && <div className="batch-toolbar" role="toolbar" aria-label={t("batch.actions")}>
        <strong>{t("batch.selected", { count: selection.ids.length })}</strong>
        <div className="row">
          <Button disabled={!selectedTextCount} onClick={() => void run(api.copyItems(selection.ids), true)}>{t("batch.copy")}</Button>
          <Button onClick={() => void run(api.favoriteItems(selection.ids, true))}>{t("batch.favorite")}</Button>
          <Button onClick={() => void run(api.favoriteItems(selection.ids, false))}>{t("batch.unfavorite")}</Button>
          <Button variant="danger" disabled={selection.ids.some((id) => pendingIds.has(id))} onClick={(event) => askDelete(selection.ids, event.currentTarget)}>{t(localOnly ? "batch.deleteLocal" : "batch.delete")}</Button>
          <Button onClick={clear}>{t("batch.clear")}</Button>
        </div>
        <p className="muted small">{t("batch.copyPolicy")}</p>
      </div>}
      {deletions.pending.map((entry) => <div className="notice notice-warn undo-deletion" key={entry.token} role="status">
        <span>{t("batch.pending", { count: entry.itemIds.length })} {t(entry.committing || entry.remainingMs === 0 ? "batch.committing" : "batch.remaining", { seconds: Math.ceil(entry.remainingMs / 1000) })}</span>
        <Button disabled={entry.committing || entry.remainingMs === 0} onClick={() => void undo(entry.token)}>{t("batch.undo")}</Button>
      </div>)}
      {notice && <p className={`notice ${notice.error ? "notice-warn" : "notice-ok"}`} role={notice.error ? "alert" : "status"}>{t(`batch.${notice.key}`, { count: notice.count })}</p>}
      {loadState === "error" ? <p className="notice notice-danger" role="alert">{t("results.error", { code: loadError })}</p> : loadState === "loading" ? <div className="empty" aria-busy="true"><Icon name="refresh" size={30} /><p>{t("results.loading")}</p></div> : items.length === 0 ? <div className="empty"><Icon name="clipboard" size={34} /><p>{t(emptyKey)}</p></div> :
        <ul className="rows" role="listbox" aria-label={t("nav.history")} aria-multiselectable="true" aria-describedby="history-selection-hint">
          {items.map((item) => {
            const meta = [item.sourceDevice, item.sourceApp, item.createdAtMs ? formatRelative(item.createdAtMs, i18n.language) : undefined].filter(Boolean);
            const checked = selection.ids.includes(item.itemId);
            return <li key={item.itemId} role="option" aria-selected={checked} tabIndex={selected?.itemId === item.itemId ? 0 : -1} ref={(node) => { if (node) rowRefs.current.set(item.itemId, node); else rowRefs.current.delete(item.itemId); }} className={`row${checked ? " row-selected" : ""}`} onMouseDown={(event) => { if (event.ctrlKey || event.metaKey || event.shiftKey) event.preventDefault(); }} onClick={(event) => { choose(item.itemId, event.ctrlKey || event.metaKey, event.shiftKey); event.currentTarget.focus(); }} onDoubleClick={() => void act(api.copy(item.itemId))}>
              <span className="row-check" aria-hidden="true">{checked && <Icon name="check" size={14} />}</span>
              {item.kind === "image" ? <ImageThumb itemId={item.itemId} /> : <span className="row-icon"><Icon name={kindIcon(item)} /></span>}
              <span className="row-body"><span className="row-title">{itemTitle(item, t)}</span>{(meta.length > 0 || item.note) && <span className="row-meta">{meta.join(" · ")}{item.note && <em>{meta.length > 0 ? " · " : ""}{item.note}</em>}</span>}</span>
              <span className="row-flags">
                {pendingIds.has(item.itemId) && <Badge tone="warn">{t("batch.pendingRow")}</Badge>}
                {item.pinned && <Icon name="pin" size={15} filled />}{item.favorite && <span role="img" aria-label={t("item.favorite")}><Icon name="star" size={15} filled /></span>}
                {item.syncState === "pending" && <Badge tone="warn">{t("sync.pending")}</Badge>}{item.syncState === "local" && <Badge>{t("sync.localOnly")}</Badge>}{item.syncState === "synced" && <Badge tone="ok">{t("sync.synced")}</Badge>}
              </span>
              <button type="button" className="row-copy" title={t("item.copy")} aria-label={t("item.copy")} onClick={(event) => { event.stopPropagation(); void act(api.copy(item.itemId)); }}><Icon name="copy" size={16} /></button>
            </li>;
          })}
        </ul>}
    </div>
    <aside className="detail" aria-label={t("detail.title")}>
      <div className="detail-head"><h2>{t("detail.title")}</h2>{selected?.syncState === "synced" && <Badge tone="ok">{t("sync.synced")}</Badge>}{selected?.syncState === "pending" && <Badge tone="warn">{t("sync.pending")}</Badge>}{selected?.syncState === "local" && <Badge>{t("sync.localOnly")}</Badge>}</div>
      {selected ? <>
        {selected.kind === "image" ? <ImageDetail item={selected} act={act} /> : <TextDetail item={selected} height={previewHeight} onHeight={onPreviewHeight} act={act} />}
        {(selected.sourceDevice || selected.sourceApp || selected.createdAtMs) && <p className="muted small">{t("detail.from", { source: [selected.sourceDevice, selected.sourceApp, selected.createdAtMs ? formatRelative(selected.createdAtMs, i18n.language) : undefined].filter(Boolean).join(" \u00b7 ") })}</p>}
        <div className="detail-actions">
          {selected.kind !== "image" && <Button variant="primary" className="grow" onClick={() => void act(api.copy(selected.itemId))}>{t("item.copy")}</Button>}
          <Button aria-pressed={selected.favorite} onClick={() => void act(api.setFlags(selected.itemId, { favorite: !selected.favorite }))}><Icon name="star" size={15} filled={selected.favorite} />{selected.favorite ? t("item.unfavorite") : t("item.favorite")}</Button>
          <Button aria-pressed={selected.pinned} onClick={() => void act(api.setFlags(selected.itemId, { pinned: !selected.pinned }))}><Icon name="pin" size={15} filled={selected.pinned} />{selected.pinned ? t("item.unpin") : t("item.pin")}</Button>
        </div>
        <label className="field"><span>{t("detail.note")}</span><input key={selected.itemId} defaultValue={selected.note} placeholder={t("detail.notePlaceholder")} onBlur={(event) => { if (event.target.value !== selected.note) void act(api.setFlags(selected.itemId, { note: event.target.value })); }} /></label>
        <div className="detail-spacer" />
        <Button variant="ghost" className="wide delete" disabled={pendingIds.has(selected.itemId)} onClick={(event) => askDelete([selected.itemId], event.currentTarget)}><Icon name="trash" size={15} />{t(localOnly ? "batch.singleDeleteLocal" : "detail.delete")}</Button>
      </> : <p className="muted">{t("detail.none")}</p>}
    </aside>
    {confirmIds && <DeleteDialog count={confirmIds.length} localOnly={localOnly} onConfirm={() => stage(confirmIds)} onClose={() => setConfirmIds(null)} returnFocus={opener.current ?? searchRef.current} />}
  </div>;
}

function DeleteDialog({ count, localOnly, onConfirm, onClose, returnFocus }: { count: number; localOnly: boolean; onConfirm: () => Promise<void>; onClose: () => void; returnFocus: HTMLElement | null }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => () => { if (returnFocus?.isConnected) returnFocus.focus(); }, [returnFocus]);
  useEffect(() => { if (busy) root.current?.focus(); else root.current?.querySelector<HTMLButtonElement>("button")?.focus(); }, [busy]);
  const keyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); if (!busy) onClose(); }
    if (event.key !== "Tab") return;
    const targets = Array.from(root.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
    if (!targets.length) { event.preventDefault(); root.current?.focus(); }
    else if (event.shiftKey && (document.activeElement === targets[0] || document.activeElement === root.current)) { event.preventDefault(); targets.at(-1)?.focus(); }
    else if (!event.shiftKey && (document.activeElement === targets.at(-1) || document.activeElement === root.current)) { event.preventDefault(); targets[0]?.focus(); }
  };
  return <div className="modal-backdrop"><div className="modal card" role="dialog" aria-modal="true" aria-labelledby="delete-title" ref={root} tabIndex={-1} onKeyDown={keyboard}>
    <h2 id="delete-title">{t("batch.deleteTitle")}</h2>
    <p>{t("batch.selected", { count })}</p>
    <p className="notice notice-warn">{t(localOnly ? "batch.deleteBodyLocal" : "batch.deleteBody")}</p>
    <p className="muted small">{t("batch.exitPolicy")}</p>
    {error && <p className="notice notice-warn" role="alert">{t("batch.actionFailed")}</p>}
    <div className="row modal-actions">
      <Button disabled={busy} onClick={onClose}>{t("batch.cancel")}</Button>
      <Button variant="danger" disabled={busy} onClick={() => { setBusy(true); setError(false); void onConfirm().catch(() => setError(true)).finally(() => setBusy(false)); }}>{t(localOnly ? "batch.singleDeleteLocal" : "detail.delete")}</Button>
    </div>
  </div></div>;
}
