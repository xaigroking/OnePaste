import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ImageData } from "../api";
import { Button } from "./ui";

const MIN = 0.1;
const MAX = 8;

/** The picture alone, large: zoom with the wheel, the keys + - 0 or the buttons, drag to move, double-click to switch. */
export function ImageViewer({ image, onClose }: { image: ImageData; onClose: () => void }) {
  const { t } = useTranslation();
  const [fit, setFit] = useState(true);
  const [scale, setScale] = useState(1);
  const pane = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; left: number; top: number } | null>(null);
  const src = `data:image/png;base64,${image.data}`;

  const zoom = useCallback((factor: number) => {
    setFit(false);
    setScale((s) => Math.min(MAX, Math.max(MIN, s * factor)));
  }, []);
  const toggle = useCallback(() => {
    setFit((f) => !f);
    setScale(1);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "+" || e.key === "=") {
        e.preventDefault();
        zoom(1.25);
      } else if (e.key === "-" || e.key === "_") {
        e.preventDefault();
        zoom(0.8);
      } else if (e.key === "0") {
        e.preventDefault();
        setFit(true);
        setScale(1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, zoom]);

  const style = fit ? { maxWidth: "100%", maxHeight: "100%" } : { width: image.width * scale, height: image.height * scale, maxWidth: "none" };

  return (
    <div className="modal-backdrop viewer-backdrop">
      <div className="viewer" role="dialog" aria-modal="true" aria-label={t("image.viewerTitle")}>
        <div className="viewer-bar">
          <Button onClick={() => zoom(0.8)} aria-label={t("image.zoomOut")}>
            −
          </Button>
          <span className="viewer-level" data-testid="zoom-level">
            {fit ? t("image.fit") : `${Math.round(scale * 100)}%`}
          </span>
          <Button onClick={() => zoom(1.25)} aria-label={t("image.zoomIn")}>
            +
          </Button>
          <Button onClick={toggle}>{fit ? t("image.actual") : t("image.fitWindow")}</Button>
          <span className="grow" />
          <button type="button" className="btn btn-primary" onClick={onClose}>
            {t("image.close")}
          </button>
        </div>
        <div
          ref={pane}
          className="viewer-pane"
          onWheel={(e) => zoom(e.deltaY < 0 ? 1.1 : 0.9)}
          onMouseDown={(e) => {
            if (pane.current) drag.current = { x: e.clientX, y: e.clientY, left: pane.current.scrollLeft, top: pane.current.scrollTop };
          }}
          onMouseMove={(e) => {
            const d = drag.current;
            if (d && pane.current) {
              pane.current.scrollLeft = d.left - (e.clientX - d.x);
              pane.current.scrollTop = d.top - (e.clientY - d.y);
            }
          }}
          onMouseUp={() => (drag.current = null)}
          onMouseLeave={() => (drag.current = null)}
          onDoubleClick={toggle}
        >
          <img src={src} alt="" draggable={false} style={style} />
        </div>
      </div>
    </div>
  );
}
