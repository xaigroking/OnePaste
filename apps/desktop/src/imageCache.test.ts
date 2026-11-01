import { afterEach, expect, it, vi } from "vitest";
import { api } from "./api";
import { cachedBytes, clearImageCache, thumbnail } from "./imageCache";
vi.mock("./api", () => ({ api: { itemImage: vi.fn() } }));
afterEach(() => { clearImageCache(); vi.unstubAllGlobals(); });
function canvas() {
  vi.stubGlobal("createImageBitmap", async () => ({ width: 1, height: 1, close() {} }));
  vi.stubGlobal("document", { createElement: () => ({ getContext: () => ({ drawImage() {} }), toDataURL: () => "data:image/jpeg;base64,cHJpdmF0ZQ==" }) });
}
it("locking clears completed thumbnails from process memory", async () => {
  canvas(); vi.mocked(api.itemImage).mockResolvedValue({ data: "YQ==", width: 1, height: 1, size: 1, format: "png" });
  await thumbnail("one"); expect(cachedBytes()).toBeGreaterThan(0);
  clearImageCache(); expect(cachedBytes()).toBe(0);
});
it("an image response from before locking cannot refill the cleared cache", async () => {
  canvas(); let finish!: (value: any) => void;
  vi.mocked(api.itemImage).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const old = thumbnail("one"); clearImageCache();
  finish({ data: "YQ==", width: 1, height: 1, size: 1, format: "png" });
  await expect(old).rejects.toThrow("locked"); expect(cachedBytes()).toBe(0);
});
