import { api } from "./api";

/**
 * Thumbnails of image items. They are made when a row comes into view, kept in memory only (nothing is written to disk:
 * the local data is encrypted and a plain thumbnail file would undo that), and the oldest are dropped when the total
 * passes the limit.
 */
const SIZE = 80;
const LIMIT_BYTES = 64 * 1024 * 1024;

const cache = new Map<string, string>();
let total = 0;
const pending = new Map<string, Promise<string>>();

function remember(id: string, url: string) {
  cache.set(id, url);
  total += url.length;
  // Map keeps insertion order: the first keys are the oldest
  for (const key of cache.keys()) {
    if (total <= LIMIT_BYTES) break;
    const old = cache.get(key);
    cache.delete(key);
    total -= old?.length ?? 0;
  }
}

function toBlob(base64: string): Blob {
  const bin = atob(base64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type: "image/png" });
}

/** A square thumbnail (cropped from the middle) as a data address. */
export function thumbnail(itemId: string): Promise<string> {
  const known = cache.get(itemId);
  if (known) return Promise.resolve(known);
  const running = pending.get(itemId);
  if (running) return running;
  const work = (async () => {
    const image = await api.itemImage(itemId);
    // Decoded off the main thread, so a large picture does not freeze the window
    const bitmap = await createImageBitmap(toBlob(image.data));
    const side = Math.min(bitmap.width, bitmap.height);
    const canvas = document.createElement("canvas");
    canvas.width = SIZE;
    canvas.height = SIZE;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no canvas");
    ctx.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, SIZE, SIZE);
    bitmap.close();
    const url = canvas.toDataURL("image/jpeg", 0.8);
    remember(itemId, url);
    return url;
  })().finally(() => pending.delete(itemId));
  pending.set(itemId, work);
  return work;
}

/** For tests of the limit. */
export function cachedBytes(): number {
  return total;
}
