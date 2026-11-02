import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
const locale = (lang: string) => JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8")).files;
for (const format of ["txt", "doc", "docx"]) test(`${format} body is readable and never interpreted as application HTML`, async ({ page }) => {
  await page.goto(`/?lang=en&attachment=yes&fileFormat=${format}&unsafeText=yes`);
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: "Preview content", exact: true }).click();
  await expect(detail.locator(".document-text")).toContainText("Actual document body: Alpha & Beta.");
  await expect(detail.locator(".document-text")).toContainText("<script>");
  expect(await page.evaluate(() => (window as any).attachmentExecuted)).toBeUndefined();
  await detail.getByRole("button", { name: "Close preview", exact: true }).click();
  await expect(detail.locator(".document-text")).toHaveCount(0);
});
test("real PDF pages render, navigate and zoom without external network requests", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (request) => { if (/^https?:/.test(request.url()) && new URL(request.url()).hostname !== "127.0.0.1") external.push(request.url()); });
  await page.goto("/?lang=en&attachment=yes&fileFormat=pdf");
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: "Preview content", exact: true }).click();
  const canvas = detail.getByRole("img", { name: /OnePaste document sample/ });
  await expect(canvas).toBeVisible();
  expect(await canvas.evaluate((element) => { const c = element as HTMLCanvasElement; return Array.from(c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data).some((v, i) => i % 4 !== 3 && v < 100); })).toBe(true);
  await expect(detail).toContainText("Page 1 of 2");
  const width = await canvas.evaluate((element) => (element as HTMLCanvasElement).width);
  await detail.getByRole("combobox").selectOption("1.5");
  await expect.poll(() => detail.locator("canvas").evaluate((element) => element.width)).toBeGreaterThan(width);
  await expect(detail.getByRole("button", { name: "Next page", exact: true })).toBeEnabled();
  await detail.getByRole("button", { name: "Next page", exact: true }).click();
  await expect(detail.getByRole("img", { name: /Second page content/ })).toBeVisible();
  await expect(detail).toContainText("Page 2 of 2");
  expect(external).toEqual([]);
});
for (const code of ["preview_encrypted", "preview_corrupt", "preview_limit", "preview_encoding"]) test(`preview failure is explicit: ${code}`, async ({ page }) => {
  await page.goto(`/?lang=en&attachment=yes&fileFormat=docx&previewError=${code}`);
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: "Preview content", exact: true }).click();
  await expect(detail.getByRole("alert")).toHaveText(locale("en").errors[code]);
  await expect(detail.getByRole("button", { name: "Save as…", exact: true })).toBeEnabled();
});
test("unknown binary remains exportable with an honest unsupported preview", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileFormat=bin");
  await expect(page.getByRole("complementary")).toContainText("Content preview is not supported for this format");
  await expect(page.getByRole("button", { name: "Preview content", exact: true })).toHaveCount(0);
});
test("the real PDF parser rejects a corrupt document", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileFormat=pdf&badPdf=yes");
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: "Preview content", exact: true }).click();
  await expect(detail.getByRole("alert")).toHaveText(locale("en").errors.preview_corrupt);
});
for (const theme of ["light", "dark"]) for (const lang of ["en", "zh"]) for (const format of ["pdf", "doc"]) test(`file preview ${format} ${theme} ${lang}`, async ({ page }) => {
  await page.setViewportSize({ width: 1040, height: 720 });
  await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
  await page.goto(`/?lang=${lang}&theme=${theme}&attachment=yes&fileFormat=${format}`);
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: locale(lang).preview.open, exact: true }).click();
  if (format === "pdf") await expect(detail.getByRole("img", { name: /OnePaste document sample/ })).toBeVisible();
  else await expect(detail.locator(".document-text")).toContainText("Actual document body");
  await expect(page).toHaveScreenshot(`file-preview-${format}-${theme}-${lang}.png`);
});

for (const [variant, code] of [["protected", "preview_encrypted"], ["oversized", "preview_limit"], ["many", "preview_limit"]]) test(`real PDF rejects ${variant} sample with an explicit reason`, async ({ page }) => {
  await page.goto(`/?lang=en&attachment=yes&fileFormat=pdf&pdfVariant=${variant}`);
  const detail = page.getByRole("complementary");
  await detail.getByRole("button", { name: "Preview content", exact: true }).click();
  await expect(detail.getByRole("alert")).toHaveText(locale("en").errors[code]);
});

test("Enter on a file row opens the full file dialog without attempting clipboard copy", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileFormat=pdf");
  await page.getByRole("listbox").getByRole("option").first().focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog")).toContainText("sample.pdf");
  expect(await page.evaluate(() => (window as any).__copied ?? [])).toEqual([]);
});
