import { useEffect, useRef, useState } from "react";
import { thumbnail } from "../imageCache";
import { Icon } from "./Icon";

/** The small picture in a list row. It is made when the row comes into view; until then, and if it fails, the icon shows. */
export function ImageThumb({ itemId }: { itemId: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const box = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let cancelled = false;
    const load = () => {
      void thumbnail(itemId)
        .then((u) => {
          if (!cancelled) setUrl(u);
        })
        .catch(() => undefined);
    };
    if (typeof IntersectionObserver === "undefined") {
      load();
      return () => {
        cancelled = true;
      };
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer.disconnect();
        load();
      }
    });
    observer.observe(el);
    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [itemId]);

  return (
    <span ref={box} className="row-icon thumb">
      {url ? <img src={url} alt="" width={40} height={40} /> : <Icon name="image" />}
    </span>
  );
}
