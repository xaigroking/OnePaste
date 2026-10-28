import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { ItemView } from "../api";
import { Button } from "./ui";

const PREVIEW_LINES = 20;
const MIN_HEIGHT = 72;

/** The text of an item in a box that can be made taller by dragging its corner, and a dialog for the whole of a long text. */
export function TextDetail({
  item,
  height,
  onHeight,
  act,
}: {
  item: ItemView;
  height: number | null;
  onHeight: (px: number) => void;
  act: (work: Promise<void>) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    setText(null);
    api
      .itemText(item.itemId)
      .then((v) => !cancelled && setText(v))
      // If the whole text cannot be had, the first line the list knows is better than nothing
      .catch(() => !cancelled && setText(item.preview));
    return () => {
      cancelled = true;
    };
  }, [item.itemId, item.preview]);

  const lines = (text ?? item.preview).split("\n");
  const long = lines.length > PREVIEW_LINES;
  const shown = long ? lines.slice(0, PREVIEW_LINES).join("\n") + "\n…" : (text ?? item.preview);

  return (
    <>
      <div
        ref={box}
        className="preview text-box"
        data-testid="text-box"
        style={height ? { height } : undefined}
        // The height the user gives it is remembered
        onMouseUp={() => {
          const h = box.current?.offsetHeight;
          if (h && h >= MIN_HEIGHT && h !== height) onHeight(h);
        }}
      >
        <p>{shown}</p>
      </div>
      {long && (
        <div className="row">
          <Button onClick={() => setExpanded(true)}>{t("text.expand")}</Button>
          <span className="muted small">{t("text.lines", { count: lines.length })}</span>
        </div>
      )}
      {expanded && <TextViewer text={text ?? ""} onCopy={() => void act(api.copy(item.itemId))} onClose={() => setExpanded(false)} />}
    </>
  );
}

/** The whole text: scrollable, selectable, with a button that copies all of it. Escape closes. */
export function TextViewer({ text, onCopy, onClose }: { text: string; onCopy: () => void; onClose: () => void }) {
  const { t } = useTranslation();
  const [mono, setMono] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-backdrop">
      <div className="modal card text-viewer" role="dialog" aria-modal="true" aria-labelledby="text-title">
        <h2 id="text-title">{t("text.fullTitle")}</h2>
        <pre className={`full-text${mono ? " mono" : ""}`} tabIndex={0}>
          {text}
        </pre>
        <div className="row modal-actions">
          <label className="row">
            <input type="checkbox" checked={mono} onChange={(e) => setMono(e.target.checked)} />
            <span>{t("text.monospace")}</span>
          </label>
          <span className="grow" />
          <Button variant="primary" onClick={onCopy}>
            {t("text.copyAll")}
          </Button>
          <Button onClick={onClose}>{t("image.close")}</Button>
        </div>
      </div>
    </div>
  );
}
