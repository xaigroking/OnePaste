import { expect, test } from "@playwright/test";

// Keyboard walkthroughs. The page is the quick panel and the first-run screens with the stand-in api.

declare global {
  interface Window {
    __copied?: string[];
    __hidden?: boolean;
    __called?: string[];
  }
}

test("quick panel: search, move down, Enter copies the selected item and closes the panel", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("searchbox")).toBeFocused();
  await page.keyboard.type("re");
  // Two items contain "re": the review note first, the report second
  await expect(page.getByRole("option")).toHaveCount(2);
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("option").nth(1)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i6"]);
  await expect.poll(() => page.evaluate(() => window.__hidden)).toBe(true);
});

test("quick panel: Enter without moving copies the first item", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
});

test("quick panel: a digit picks that row when the search box is empty", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("2");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i2"]);
});

test("quick panel: Tab moves through the type chips and Escape closes", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect(page.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Text", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => window.__hidden)).toBe(true);
});

test("first run: choosing local-only works with the keyboard alone", async ({ page }) => {
  await page.goto("/?screen=ob1&lang=en");
  await page.getByRole("radio", { name: /Only on this computer/ }).focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  // The disabled hosted option is skipped; the next stop is the Next button
  await expect(page.getByRole("button", { name: "Next" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["chooseLocalOnly"]);
});
