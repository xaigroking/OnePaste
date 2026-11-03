import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
const locale = (lang: string) => JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8"));
for (const lang of ["en", "zh"]) for (const theme of ["light", "dark"]) test(`settings and about omit telemetry claims and the empty advanced card ${lang} ${theme}`, async ({ page }) => {
  await page.setViewportSize({ width: 1040, height: 720 });
  await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
  await page.goto(`/?screen=set&lang=${lang}`);
  await expect(page.locator("body")).not.toContainText(/telemetry|\u9065\u6d4b/iu);
  await expect(page.getByRole("heading", { name: lang === "en" ? "Advanced" : "\u9ad8\u7ea7", exact: true })).toHaveCount(0);
  await page.getByText(locale(lang).settings.aboutBody, { exact: true }).scrollIntoViewIfNeeded();
  await expect(page).toHaveScreenshot(`settings-about-${theme}-${lang}.png`);
});
