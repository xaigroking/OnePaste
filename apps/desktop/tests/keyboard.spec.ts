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

test("update dialog: Escape means later, and the three buttons do what they say", async ({ page }) => {
  await page.goto("/?screen=update&lang=en");
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Update", exact: true })).toBeFocused();
  // The note is text, not markup
  await expect(dialog.getByText("<b>Not markup</b>")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(await page.evaluate(() => window.__called ?? [])).toEqual([]);
  await page.reload();
  await page.getByRole("button", { name: "Skip this version" }).click();
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["skipUpdate"]);
  await page.reload();
  await page.getByRole("button", { name: "Update", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["openRelease"]);
});

test("first run: choosing local-only works with the keyboard alone", async ({ page }) => {
  await page.goto("/?screen=ob1&lang=en");
  await page.getByRole("radio", { name: /Only on this computer/ }).focus();
  await page.keyboard.press("Space");
  await page.keyboard.press("Tab");
  // The radio group is one stop and the disabled hosted option is skipped: the next stop is the Next button
  await expect(page.getByRole("button", { name: "Next" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["chooseLocalOnly"]);
});
