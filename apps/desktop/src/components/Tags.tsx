import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { ItemView, TagOption } from "../api";
import { Button } from "./ui";

export function TagsEditor({ item, act }: { item: ItemView; act: (work: Promise<void>) => Promise<void> }) {
  const { t } = useTranslation();
  const [tags, setTags] = useState(item.tags ?? []);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => setTags(item.tags ?? []), [item.tags]);
  const save = async (values: string[]) => {
    if (busy) return;
    setBusy(true); setError(false);
    try { const result = await api.setTags(item.itemId, values); setTags(result); setDraft(""); await act(Promise.resolve()); }
    catch { setError(true); }
    finally { setBusy(false); }
  };
  return <section className="tags-editor" aria-label={t("tags.title")}>
    <span className="small muted">{t("tags.title")}</span>
    <div className="tag-list">{tags.map((tag, index) => <span className="tag" key={`${index}:${tag}`}>{tag}<button type="button" disabled={busy} aria-label={t("tags.remove", { tag })} onClick={() => void save(tags.filter((value) => value !== tag))}>×</button></span>)}</div>
    <div className="row"><input type="text" aria-label={t("tags.name")} maxLength={256} value={draft} disabled={busy} placeholder={t("tags.name")} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); if (draft.trim()) void save([...tags, draft]); } }} /><Button disabled={busy || !draft.trim()} onClick={() => void save([...tags, draft])}>{t("tags.add")}</Button></div>
    <p className="small muted">{t("tags.hint")}</p>
    {busy && <p role="status">{t("tags.saving")}</p>}
    {error && <p className="notice notice-danger" role="alert">{t("tags.failed")}</p>}
  </section>;
}

export function TagFilter({ value, onChange }: { value: string | null; onChange: (value: string | null) => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState<TagOption[]>([]);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const trigger = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.altKey && !event.ctrlKey && !event.metaKey && event.key.toLowerCase() === "t") { event.preventDefault(); setOpen(true); if (input.current) input.current.focus(); else trigger.current?.focus(); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  useEffect(() => {
    if (!open) return;
    let cancelled = false; setState("loading");
    api.tagOptions(query).then((tags) => { if (!cancelled) { setOptions(tags); setState("ready"); } }).catch(() => { if (!cancelled) setState("error"); });
    return () => { cancelled = true; };
  }, [open, query]);
  useEffect(() => { if (open) input.current?.focus(); }, [open]);
  return <div className="tag-filter" onKeyDown={(event) => {
    if (open) { event.stopPropagation(); if (event.key === "Escape") { event.preventDefault(); close(); } }
  }}>
    <div className="row"><button type="button" className="chip" ref={trigger} aria-label={t("tags.filter")} aria-keyshortcuts="Alt+T" aria-expanded={open} onClick={() => setOpen(!open)}>{value ?? t("tags.all")}</button>{value && <Button onClick={() => onChange(null)}>{t("tags.clear")}</Button>}<span className="small muted">{t("tags.shortcut")}</span></div>
    {open && <div className="tag-menu" role="group" aria-label={t("tags.filter")}>
      <input type="search" ref={input} value={query} maxLength={64} aria-label={t("tags.find")} placeholder={t("tags.find")} onChange={(event) => setQuery(event.target.value)} />
      {state === "loading" && <p role="status">{t("tags.loading")}</p>}
      {state === "error" && <p role="alert">{t("tags.loadFailed")}</p>}
      {state === "ready" && !options.length && <p className="muted">{t("tags.empty")}</p>}
      {state === "ready" && options.map((tag) => <Button key={tag.label} aria-pressed={value?.toLowerCase() === tag.label.toLowerCase()} onClick={() => { onChange(tag.label); close(); }}>{t("tags.option", { tag: tag.label, count: tag.count })}</Button>)}
    </div>}
  </div>;
}
