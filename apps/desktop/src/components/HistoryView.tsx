import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { Filter, ItemView } from "../api";
import { formatRelative, formatSize, isLink } from "../format";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { Badge, Button, Chip } from "./ui";

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
  items: ItemView[];
  query: string;
  filter: Filter;
  onQuery: (q: string) => void;
  onFilter: (f: Filter) => void;
  act: (work: Promise<void>) => Promise<void>;
  loadState: "loading" | "ready" | "error";
  loadError: string;
}

export function HistoryView({ items, query, filter, onQuery, onFilter, act, loadState, loadError }: Props) {
  const { t, i18n } = useTranslation();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  // Keep a valid selection as the list changes.
  const selected = useMemo(
    () => items.find((it) => it.itemId === selectedId) ?? items[0] ?? null,
    [items, selectedId],
  );

  useEffect(() => {
    setConfirmDelete(null);
  }, [selected?.itemId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (inField || items.length === 0) return;
      const idx = Math.max(0, items.findIndex((it) => it.itemId === selected?.itemId));
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedId(items[Math.min(items.length - 1, idx + 1)]?.itemId ?? null);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedId(items[Math.max(0, idx - 1)]?.itemId ?? null);
      } else if (e.key === "Enter" && selected) {
        void act(api.copy(selected.itemId));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items, selected, act]);

  const emptyKey = query.trim() || filter !== "all" ? "results.noMatch" : "results.empty";

  return (
    <div className="history">
      <div className="history-main">
        <div className="searchbar">
          <Icon name="search" />
          <input
            ref={searchRef}
            type="search"
            placeholder={t("search.placeholder")}
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            aria-label={t("search.placeholder")}
          />
        </div>
        <div className="chips" role="group" aria-label={t("search.filterLabel")}>
          {FILTERS.map((f) => (
            <Chip key={f} active={filter === f} onClick={() => onFilter(f)}>
              {f === "favorites" && <Icon name="star" size={13} />}
              {t(`search.filter.${f}`)}
            </Chip>
          ))}
        </div>

        {loadState === "error" ? (
          <p className="notice notice-danger" role="alert">
            {t("results.error", { code: loadError })}
          </p>
        ) : loadState === "loading" ? (
          <div className="empty" aria-busy="true">
            <Icon name="refresh" size={30} />
            <p>{t("results.loading")}</p>
          </div>
        ) : items.length === 0 ? (
          <div className="empty">
            <Icon name="clipboard" size={34} />
            <p>{t(emptyKey)}</p>
          </div>
        ) : (
          <ul className="rows" role="listbox" aria-label={t("nav.history")}>
            {items.map((it) => {
              const meta = [
                it.sourceDevice,
                it.sourceApp,
                it.createdAtMs ? formatRelative(it.createdAtMs, i18n.language) : undefined,
              ].filter(Boolean);
              return (
                <li
                  key={it.itemId}
                  role="option"
                  aria-selected={selected?.itemId === it.itemId}
                  className={`row${selected?.itemId === it.itemId ? " row-selected" : ""}`}
                  onClick={() => setSelectedId(it.itemId)}
                  onDoubleClick={() => void act(api.copy(it.itemId))}
                >
                  <span className="row-icon">
                    <Icon name={kindIcon(it)} />
                  </span>
                  <span className="row-body">
                    <span className="row-title">{itemTitle(it, t)}</span>
                    {(meta.length > 0 || it.note) && (
                      <span className="row-meta">
                        {meta.join(" · ")}
                        {it.note && <em>{meta.length > 0 ? " · " : ""}{it.note}</em>}
                      </span>
                    )}
                  </span>
                  <span className="row-flags">
                    {it.pinned && <Icon name="pin" size={15} filled />}
                    {it.favorite && <Icon name="star" size={15} filled />}
                    {it.syncState === "pending" && <Badge tone="warn">{t("sync.pending")}</Badge>}
                    {it.syncState === "local" && <Badge>{t("sync.localOnly")}</Badge>}
                    {it.syncState === "synced" && <Badge tone="ok">{t("sync.synced")}</Badge>}
                  </span>
                  <button
                    type="button"
                    className="row-copy"
                    title={t("item.copy")}
                    aria-label={t("item.copy")}
                    onClick={(e) => {
                      e.stopPropagation();
                      void act(api.copy(it.itemId));
                    }}
                  >
                    <Icon name="copy" size={16} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      <aside className="detail" aria-label={t("detail.title")}>
        <div className="detail-head">
          <h2>{t("detail.title")}</h2>
          {selected?.syncState === "synced" && <Badge tone="ok">{t("sync.synced")}</Badge>}
          {selected?.syncState === "pending" && <Badge tone="warn">{t("sync.pending")}</Badge>}
          {selected?.syncState === "local" && <Badge>{t("sync.localOnly")}</Badge>}
        </div>
        {selected ? (
          <>
            <div className="preview">
              {selected.kind === "image" ? (
                <div className="preview-image">
                  <Icon name="image" size={30} />
                  <span>{t("item.image", { width: selected.width, height: selected.height, size: formatSize(selected.size) })}</span>
                </div>
              ) : (
                <p>{selected.preview}</p>
              )}
            </div>
            {(selected.sourceDevice || selected.sourceApp || selected.createdAtMs) && (
              <p className="muted small">
                {t("detail.from", {
                  source: [selected.sourceDevice, selected.sourceApp, selected.createdAtMs ? formatRelative(selected.createdAtMs, i18n.language) : undefined]
                    .filter(Boolean)
                    .join(" \u00b7 "),
                })}
              </p>
            )}
            <div className="detail-actions">
              <Button variant="primary" className="grow" onClick={() => void act(api.copy(selected.itemId))}>
                {t("item.copy")}
              </Button>
              <Button
                aria-pressed={selected.favorite}
                onClick={() => void act(api.setFlags(selected.itemId, { favorite: !selected.favorite }))}
              >
                <Icon name="star" size={15} filled={selected.favorite} />
                {selected.favorite ? t("item.unfavorite") : t("item.favorite")}
              </Button>
              <Button
                aria-pressed={selected.pinned}
                onClick={() => void act(api.setFlags(selected.itemId, { pinned: !selected.pinned }))}
              >
                <Icon name="pin" size={15} filled={selected.pinned} />
                {selected.pinned ? t("item.unpin") : t("item.pin")}
              </Button>
            </div>
            <label className="field">
              <span>{t("detail.note")}</span>
              <input
                key={selected.itemId}
                defaultValue={selected.note}
                placeholder={t("detail.notePlaceholder")}
                onBlur={(e) => {
                  if (e.target.value !== selected.note) void act(api.setFlags(selected.itemId, { note: e.target.value }));
                }}
              />
            </label>
            <div className="detail-spacer" />
            {confirmDelete === selected.itemId ? (
              <Button variant="danger" className="wide" onClick={() => void act(api.remove(selected.itemId))}>
                {t("detail.deleteConfirm")}
              </Button>
            ) : (
              <Button variant="ghost" className="wide delete" onClick={() => setConfirmDelete(selected.itemId)}>
                <Icon name="trash" size={15} />
                {t("detail.delete")}
              </Button>
            )}
          </>
        ) : (
          <p className="muted">{t("detail.none")}</p>
        )}
      </aside>
    </div>
  );
}
