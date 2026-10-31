import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, onChanged, onQuickOpened } from "../api";
import type { Filter, ItemView, HotkeyInfo } from "../api";
import { formatRelative, formatSize } from "../format";
import { kindIcon } from "./HistoryView";
import { Icon } from "./Icon";
import { Badge, Button, Chip } from "./ui";
import { formatHotkey } from "../hotkeys";

/** The types the panel offers. Links, files and code have no type of their own in the history yet, so they show nothing. */
export type PanelFilter = Filter | "links" | "files" | "code";
export const PANEL_FILTERS: PanelFilter[] = ["all", "text", "links", "images", "files", "code", "favorites"];
const BACKED: PanelFilter[] = ["all", "text", "images", "favorites"];

/** The quick panel: search, type chips, a numbered list, and a bar that lists the keys. */
export function QuickPanel() {
  const { t, i18n } = useTranslation();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<PanelFilter>("all");
  const [items, setItems] = useState<ItemView[]>([]);
  const [selected, setSelected] = useState(0);
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState("");
  const [pasteAllowed, setPasteAllowed] = useState<boolean | null>(null);
  const [permissionDismissed, dismissPermission] = useState(false);
  const [actionNotice, setActionNotice] = useState<string | null>(null);
  const acting = useRef(false);
  const noticeRef = useRef<HTMLDivElement>(null);
  const isMac = navigator.userAgent.includes("Mac");
  const refreshPermission = useCallback(() => { void api.pastePermission().then(setPasteAllowed).catch(() => setPasteAllowed(false)); }, []);
  const [hotkey, setHotkey] = useState<HotkeyInfo | null>(null);
  const refreshHotkey = useCallback(() => { void api.hotkey().then(setHotkey).catch(() => setHotkey(null)); }, []);
  const inputRef = useRef<HTMLInputElement>(null);
  const seq = useRef(0);

  const load = useCallback(async () => {
    const mine = ++seq.current;
    if (!BACKED.includes(filter)) {
      setItems([]);
      setState("ready");
      return;
    }
    setState("loading");
    try {
      const found = await api.search(query, filter as Filter, 50, 0);
      if (mine !== seq.current) return;
      setItems(found);
      setSelected(0);
      setState("ready");
    } catch (e) {
      if (mine !== seq.current) return;
      setError(String(e));
      setState("error");
    }
  }, [query, filter]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    refreshHotkey(); refreshPermission();
    void onChanged(refreshHotkey).then((stop) => { if (live) unlisten = stop; else stop(); });
    return () => { live = false; unlisten?.(); };
  }, [refreshHotkey, refreshPermission]);

  // Every time the panel is brought up it starts clean, with the search box focused
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onQuickOpened(() => {
      refreshHotkey(); refreshPermission();
      setQuery("");
      setFilter("all");
      inputRef.current?.focus();
      void load();
    }).then((u) => {
      unlisten = u;
    });
    inputRef.current?.focus();
    return () => unlisten?.();
  }, [load, refreshHotkey, refreshPermission]);

  const choose = useCallback(
    async (it: ItemView | undefined, paste = false) => {
      if (!it || acting.current) return;
      acting.current = true;
      try {
        if (paste) {
          const result = await api.quickPaste(it.itemId);
          if (result === "copied_no_permission") setPasteAllowed(false);
          if (result === "copied_manual") setActionNotice("manualPaste");
        } else { await api.copy(it.itemId); await api.hideQuickPanel(); }
      } catch { setActionNotice("copyFailed"); }
      finally { acting.current = false; }
    },
    [],
  );

  useLayoutEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F6" && noticeRef.current) {
        e.preventDefault();
        if (noticeRef.current.contains(document.activeElement)) inputRef.current?.focus();
        else noticeRef.current.querySelector<HTMLButtonElement>("button")?.focus();
        return;
      }
      if (e.key === "Enter" && (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) && !e.altKey && !e.shiftKey) { e.preventDefault(); if (!e.repeat) void choose(items[selected], true); return; }
      if (noticeRef.current?.contains(document.activeElement) && e.key !== "Escape") return;
      if (e.key === "Escape") {
        e.preventDefault();
        void api.hideQuickPanel();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelected((s) => Math.min(items.length - 1, s + 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((s) => Math.max(0, s - 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        void choose(items[selected]);
      } else if (e.key === "Tab") {
        e.preventDefault();
        const at = PANEL_FILTERS.indexOf(filter);
        const step = e.shiftKey ? PANEL_FILTERS.length - 1 : 1;
        setFilter(PANEL_FILTERS[(at + step) % PANEL_FILTERS.length]);
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        inputRef.current?.focus();
      } else if (/^[1-9]$/.test(e.key) && query === "" && !e.ctrlKey && !e.metaKey && !e.altKey) {
        // With an empty search box a digit picks that row; otherwise it is part of what is being typed
        e.preventDefault();
        void choose(items[Number(e.key) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [items, selected, filter, query, choose, isMac]);

  useEffect(() => {
    document.querySelector(".qp-row-selected")?.scrollIntoView({ block: "nearest" });
  }, [selected, items]);

  return (
    <div className="qp" role="dialog" aria-label={t("qp.title")}>
      <div className="qp-search">
        <Icon name="search" />
        <input
          ref={inputRef}
          type="search"
          autoFocus
          value={query}
          placeholder={t("qp.placeholder")}
          aria-label={t("qp.placeholder")}
          onChange={(e) => setQuery(e.target.value)}
        />
        <kbd>{navigator.userAgent.includes("Mac") ? "⌘ F" : "Ctrl F"}</kbd>
      </div>
      <div className="chips qp-chips" role="group" aria-label={t("search.filterLabel")}>
        {PANEL_FILTERS.map((f) => (
          <Chip key={f} active={filter === f} onClick={() => setFilter(f)}>
            {f === "favorites" && <Icon name="star" size={13} />}
            {t(`search.filter.${f}`)}
          </Chip>
        ))}
      </div>
      {state === "error" ? (
        <p className="notice notice-danger qp-message" role="alert">
          {t("qp.error", { code: error })}
        </p>
      ) : state === "loading" ? (
        <p className="muted qp-message" aria-busy="true">
          {t("qp.loading")}
        </p>
      ) : items.length === 0 ? (
        <div className="empty qp-message">
          <Icon name="clipboard" size={30} />
          <p>{query.trim() || filter !== "all" ? t("results.noMatch") : t("results.empty")}</p>
        </div>
      ) : (
        <ul className="qp-rows" role="listbox" aria-label={t("nav.history")}>
          {items.map((it, i) => {
            const meta = [it.sourceDevice, it.sourceApp, it.createdAtMs ? formatRelative(it.createdAtMs, i18n.language) : undefined].filter(Boolean);
            return (
              <li
                key={it.itemId}
                role="option"
                aria-selected={i === selected}
                className={`qp-row${i === selected ? " qp-row-selected" : ""}`}
                onMouseMove={() => setSelected(i)}
                onClick={() => void choose(it)}
              >
                <span className="qp-num">{i < 9 ? i + 1 : ""}</span>
                <span className="row-icon">
                  <Icon name={kindIcon(it)} />
                </span>
                <span className="row-body">
                  <span className="row-title">
                    {it.kind === "image" ? t("item.image", { width: it.width, height: it.height, size: formatSize(it.size) }) : it.preview}
                  </span>
                  {meta.length > 0 && <span className="row-meta">{meta.join(" · ")}</span>}
                </span>
                {it.syncState === "pending" && <Badge tone="warn">{t("sync.pending")}</Badge>}
                {it.syncState === "local" && <Badge>{t("sync.localOnly")}</Badge>}
                {it.syncState === "synced" && <Badge tone="ok">{t("sync.synced")}</Badge>}
              </li>
            );
          })}
        </ul>
      )}
      {((isMac && pasteAllowed === false && !permissionDismissed) || actionNotice) && <div ref={noticeRef} className="notice notice-warn" role="note">
        {isMac && pasteAllowed === false && !permissionDismissed && <>
          <p>{t("qp.pastePermission")}</p>
          <Button onClick={() => { void api.openAccessibilitySettings().catch(() => setActionNotice("settingsFailed")); }}>{t("qp.openAccessibility")}</Button>
          <Button onClick={() => { dismissPermission(true); inputRef.current?.focus(); }}>{t("qp.dismiss")}</Button>
        </>}
        {actionNotice && <><p>{t(`qp.${actionNotice}`)}</p><Button onClick={() => { setActionNotice(null); inputRef.current?.focus(); }}>{t("qp.dismiss")}</Button></>}
        <span className="muted small"><kbd>F6</kbd> {t("qp.keyNotice")}</span>
      </div>}
      <div className="qp-keys" aria-label={t("qp.keys")}>
        <span><kbd>↑</kbd><kbd>↓</kbd> {t("qp.keySelect")}</span>
        <span><kbd>{isMac ? "\u2318" : "Ctrl"}+Enter</kbd> {t("qp.keyPaste")}</span>
        <span><kbd>Enter</kbd> {t("qp.keyCopy")}</span>
        <span><kbd>1</kbd>–<kbd>9</kbd> {t("qp.keyPick")}</span>
        <span><kbd>Tab</kbd> {t("qp.keyFilter")}</span>
        <span><kbd>Esc</kbd> {t("qp.keyClose")}</span>
        {hotkey?.current && <span><kbd>{formatHotkey(hotkey.current, navigator.userAgent.includes("Mac"))}</kbd> {t("hotkey.togglePanel")}</span>}
      </div>
    </div>
  );
}
