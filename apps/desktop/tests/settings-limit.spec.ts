import { expect, test } from "@playwright/test";

test("item size limit is keyboard accessible and saves only the chosen limit", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const limit = page.getByRole("combobox", { name: "Largest item to record", exact: true });
  await expect(limit).toHaveValue("8");
  await limit.focus();
  await expect(limit).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("Home");
  await page.keyboard.press("Enter");
  await expect(limit).toHaveValue("1");
  await expect.poll(() => page.evaluate(() => window.__settingsCalls?.at(-1)?.maxItemMiB)).toBe(1);
  expect(await page.evaluate(() => window.__settingsCalls?.at(-1)?.recordClipboard)).toBe(true);
  await limit.selectOption("4");
  await expect.poll(() => page.evaluate(() => window.__settingsCalls?.at(-1)?.maxItemMiB)).toBe(4);
});

test("item size limit reports save failure and retains the saved choice", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&settingsSave=error");
  const limit = page.getByRole("combobox", { name: "Largest item to record", exact: true });
  await limit.selectOption("1");
  await expect(page.getByRole("alert")).toHaveText("Could not save this setting. Try again.");
  await expect(limit).toHaveValue("8");
});

test("a rejected copy has a dismissible notice outside the settings log", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&captureLimit=1");
  await expect(page.getByRole("alert")).toContainText("This copy exceeded the item size limit and was not saved or synced.");
  await page.getByRole("button", { name: "Dismiss size notice" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
