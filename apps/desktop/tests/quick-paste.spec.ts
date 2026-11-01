import { expect, test } from "@playwright/test";

declare global { interface Window { __pasted?: string[]; } }

test("Control+Enter requests copy-and-paste rather than a copy-only command", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("searchbox")).toBeFocused();
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Control+Enter");
  await expect.poll(() => page.evaluate(() => window.__pasted)).toEqual(["i1"]);
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
  await expect.poll(() => page.evaluate(() => window.__hidden)).toBe(true);
});

test("macOS permission guidance is inline and Command+Enter falls back to copy-and-close", async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(navigator, "userAgent", { value: "Macintosh" }));
  await page.goto("/?screen=qp&lang=en&pastePermission=no#/quick");
  await expect(page.getByText(/Enable Accessibility for OnePaste/)).toBeVisible();
  await page.keyboard.press("F6");
  await expect(page.getByRole("button", { name: "Open Accessibility Settings", exact: true })).toBeFocused();
  await expect(page.getByRole("button", { name: "Open Accessibility Settings", exact: true })).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["openAccessibilitySettings"]);
  await page.keyboard.press("Meta+Enter");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
  await expect.poll(() => page.evaluate(() => window.__hidden)).toBe(true);
  expect(await page.evaluate(() => window.__pasted ?? [])).toEqual([]);
});

test("Enter continues to copy-and-close without simulating paste", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("searchbox")).toBeFocused();
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
  expect(await page.evaluate(() => window.__pasted ?? [])).toEqual([]);
});

test("failed foreground restoration keeps the copy and explains manual paste", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en&pasteTarget=missing#/quick");
  await expect(page.getByRole("searchbox")).toBeFocused();
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Control+Enter");
  await expect(page.getByRole("note")).toContainText("Check the destination before pasting manually");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
  expect(await page.evaluate(() => window.__pasted ?? [])).toEqual([]);
});

test("a keyboard pick uses the list that was just committed to the page", async ({ page }) => {
  await page.addInitScript(() => {
    let picked = false;
    new MutationObserver(() => {
      if (!picked && document.querySelectorAll('[role="option"]').length >= 2) {
        picked = true;
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "2", bubbles: true }));
      }
    }).observe(document, { childList: true, subtree: true });
  });
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i2"]);
});
