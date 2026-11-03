import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
const readLocale = (lang: string) => JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8"));

test("tags can be added and removed independently of notes", async ({ page }) => {
  await page.goto("/?lang=en");
  await page.getByRole("textbox", { name: "Tag name", exact: true }).fill("  Work  ");
  await page.getByRole("button", { name: "Add tag", exact: true }).click();
  await expect(page.getByRole("button", { name: "Remove tag Work", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Remove tag Work", exact: true }).click();
  await expect(page.getByRole("button", { name: "Remove tag Work", exact: true })).toHaveCount(0);
});

for (const lang of ["en", "zh"]) test(`tag editing with keyboard normalizes duplicates ${lang}`, async ({ page }) => {
  await page.goto(`/?lang=${lang}`);
  const locale = readLocale(lang);
  const name = locale.tags.name;
  const input = page.getByRole("textbox", { name, exact: true });
  await input.fill("  Project   Alpha  "); await input.press("Enter");
  const remove = page.getByRole("button", { name: locale.tags.remove.replace("{{tag}}", "Project Alpha"), exact: true });
  await expect(remove).toHaveCount(1);
  await input.fill("project alpha"); await input.press("Enter");
  await expect(remove).toHaveCount(1);
});

test("failed tag save retains the actual saved collection and draft", async ({ page }) => {
  await page.goto("/?lang=en&tagFixture=yes&tagsSave=error");
  await page.getByRole("textbox", { name: "Tag name", exact: true }).fill("New tag");
  await page.getByRole("button", { name: "Add tag", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Could not save tags");
  await expect(page.getByRole("textbox", { name: "Tag name", exact: true })).toHaveValue("New tag");
  await expect(page.getByRole("button", { name: "Remove tag Work", exact: true })).toBeVisible();
});

for (const quick of [false, true]) test(`tag and type filters combine in ${quick ? "quick panel" : "main history"}`, async ({ page }) => {
  await page.goto(`/?lang=en&tagFixture=yes${quick ? "&screen=qp#/quick" : ""}`);
  await expect(page.getByRole("button", { name: "Filter by tag", exact: true })).toBeVisible();
  await page.keyboard.press("Alt+t");
  await expect(page.getByRole("searchbox", { name: "Find a tag", exact: true })).toBeFocused();
  await page.getByRole("button", { name: "Work (2)", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(2);
  await page.getByRole("button", { name: "Links", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
  await page.getByRole("button", { name: "Filter by tag", exact: true }).click();
  await page.getByRole("searchbox", { name: "Find a tag", exact: true }).fill("Engineering");
  await page.getByRole("button", { name: "Engineering (1)", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(0);
  await page.getByRole("button", { name: "Clear tag filter", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
});

test("tag suggestion failures and Escape preserve the current history filter", async ({ page }) => {
  await page.goto("/?lang=en&tagsLoad=error");
  await expect(page.getByRole("button", { name: "Filter by tag", exact: true })).toBeVisible();
  await page.keyboard.press("Alt+t");
  await expect(page.getByRole("alert")).toHaveText("Could not load tags. Try again.");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Filter by tag", exact: true })).toBeFocused();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(6);
});
