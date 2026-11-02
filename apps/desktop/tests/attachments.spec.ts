import { test, expect } from "@playwright/test";

test("native import creates a real file row and opens its file actions", async ({ page }) => {
  await page.goto("/?lang=en");
  await page.getByRole("button", { name: "Import file", exact: true }).click({ timeout: 5000 });
  await expect(page.getByRole("dialog")).toContainText("sample.txt");
  await expect(page.getByRole("dialog").getByRole("button", { name: "Save as…", exact: true })).toBeVisible();
});

test("cancelling native import leaves the history unchanged", async ({ page }) => {
  await page.goto("/?lang=en&fileImport=cancel");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(6);
  await page.getByRole("button", { name: "Import file", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(6);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("an oversized file reports an actionable error without adding an item", async ({ page }) => {
  await page.goto("/?lang=en&fileImport=error");
  await page.getByRole("button", { name: "Import file", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("This file exceeds the 64 MiB limit.");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(6);
});

test("download and save cancellation have distinct outcomes", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileCached=no&fileSave=cancel");
  const detail=page.getByRole("complementary").getByRole("region", { name: "File", exact: true });
  await expect(detail).toContainText("Download required");
  await detail.getByRole("button", { name: "Download", exact: true }).click();
  await expect(detail).toContainText("Available offline");
  await detail.getByRole("button", { name: "Save as…", exact: true }).click();
  await expect(detail.getByText("File saved.", { exact: true })).toHaveCount(0);
});

test("a save error never claims the file was saved", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileSave=error");
  const detail=page.getByRole("complementary").getByRole("region", { name: "File", exact: true });
  await detail.getByRole("button", { name: "Save as…", exact: true }).click();
  await expect(detail.getByRole("alert")).toContainText("Could not save the file");
  await expect(detail.getByText("File saved.", { exact: true })).toHaveCount(0);
});

test("an active upload can be cancelled and retried", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&fileState=uploading&fileDirection=upload");
  const detail=page.getByRole("complementary").getByRole("region", { name: "File", exact: true });
  await expect(detail).toContainText("Uploading 50%");
  await detail.getByRole("button", { name: "Cancel transfer", exact: true }).click();
  await expect(detail).toContainText("Transfer cancelled");
  await detail.getByRole("button", { name: "Retry upload", exact: true }).click();
  await expect(detail.getByText("Transfer cancelled", { exact: false })).toHaveCount(0);
});

test("quick panel file picks open file actions instead of copying a path", async ({ page }) => {
  await page.goto("/?lang=en&attachment=yes&screen=qp#/quick");
  await expect(page.getByRole("option").first()).toContainText("sample.txt");
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => (window as any).__called)).toContain("openItem");
  expect(await page.evaluate(() => (window as any).__copied ?? [])).toEqual([]);
});
