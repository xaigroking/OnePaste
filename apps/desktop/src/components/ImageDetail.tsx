import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { ImageData, ItemView } from "../api";
import { formatRelative, formatSize } from "../format";
import { ImageViewer } from "./ImageViewer";
import { Button } from "./ui";

function fileName(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `OnePaste-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.png`;
}

/** The detail of an image item: the picture, what can be done with it, and what is known about it. */
export function ImageDetail({ item, act }: { item: ItemView; act: (work: Promise<void>) => Promise<void> }) {
  const { t, i18n } = useTranslation();
  const [image, setImage] = useState<ImageData | null>(null);
  const [failed, setFailed] = useState(false);
  const [viewing, setViewing] = useState(false);
  const [actual, setActual] = useState(false);
  const [saved, setSaved] = useState<"idle" | "saved" | "failed">("idle");

  useEffect(() => {
    let cancelled = false;
    setImage(null);
    setFailed(false);
    setSaved("idle");
    setActual(false);
    api
      .itemImage(item.itemId)
      .then((i) => !cancelled && setImage(i))
      .catch(() => !cancelled && setFailed(true));
    return () => {
      cancelled = true;
    };
  }, [item.itemId]);

  const save = async () => {
    try {
      const done = await api.saveImage(item.itemId, fileName());
      setSaved(done ? "saved" : "idle");
    } catch {
      setSaved("failed");
    }
  };

  const meta = [
    `${item.width}×${item.height}`,
    formatSize(item.size),
    "PNG",
    item.sourceDevice,
    item.sourceApp,
    item.createdAtMs ? formatRelative(item.createdAtMs, i18n.language) : undefined,
  ].filter(Boolean);

  return (
    <>
      <div className={`image-preview${actual ? " image-actual" : ""}`}>
        {image ? (
          <img
            src={`data:image/png;base64,${image.data}`}
            alt={t("image.alt", { width: item.width, height: item.height })}
            onClick={() => setViewing(true)}
          />
        ) : (
          <span className="muted">{failed ? t("image.failed") : t("image.loading")}</span>
        )}
      </div>
      <div className="image-tools" role="toolbar" aria-label={t("image.tools")}>
        <Button disabled={!image} onClick={() => setViewing(true)}>
          {t("image.viewLarger")}
        </Button>
        <Button disabled={!image} aria-pressed={actual} onClick={() => setActual((a) => !a)}>
          {actual ? t("image.fitWindow") : t("image.actual")}
        </Button>
      </div>
      <p className="muted small" data-testid="image-meta">
        {meta.join(" · ")}
      </p>
      <div className="detail-actions">
        <Button variant="primary" className="grow" onClick={() => void act(api.copy(item.itemId))}>
          {t("image.copy")}
        </Button>
        <Button disabled={!image} onClick={() => void save()}>
          {t("image.saveAs")}
        </Button>
      </div>
      {saved === "saved" && <p className="notice notice-ok" role="status">{t("image.saved")}</p>}
      {saved === "failed" && (
        <p className="notice notice-danger" role="alert">
          {t("image.saveFailed")}
        </p>
      )}
      {viewing && image && <ImageViewer image={image} onClose={() => setViewing(false)} />}
    </>
  );
}
