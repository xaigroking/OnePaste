import { test, expect } from "@playwright/test";

test("links and code chips query actual classified items in the main window", async ({ page }) => {
  await page.goto("/?lang=en");
  await page.getByRole("button", { name: "Links", exact: true }).click();
  await expect(page.getByRole("option")).toHaveCount(1);
  await expect(page.getByRole("option")).toContainText("https://example.org");
  await page.getByRole("button", { name: "Code", exact: true }).click();
  await expect(page.getByRole("option")).toHaveCount(1);
  await expect(page.getByRole("option")).toContainText("docker run");
});

test("quick panel links use the same type filtering", async ({ page }) => {
  await page.goto("/?lang=en&screen=qp#/quick");
  await page.getByRole("button", { name: "Links", exact: true }).click();
  await expect(page.getByRole("option")).toHaveCount(1);
  await expect(page.getByRole("option")).toContainText("https://example.org");
});

test("a type correction is saved separately from note and favorite", async ({ page }) => {
  await page.goto("/?lang=en");
  await page.getByText("Change content type", { exact: true }).click();
  await page.getByLabel("Content type", { exact: true }).selectOption("code");
  await page.getByLabel("Code language", { exact: true }).selectOption("yaml");
  await page.getByRole("button", { name: "Save type", exact: true }).click();
  await page.getByRole("button", { name: "Code", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(2);
  await expect(page.getByRole("listbox").getByRole("option").first()).toContainText("Review moved");
  await page.getByText("Change content type", { exact: true }).click();
  await expect(page.getByLabel("Code language", { exact: true })).toHaveValue("yaml");
});

test("failed classification save reports an error and retains the saved type", async ({ page }) => {
  await page.goto("/?lang=en&classificationSave=error");
  await page.getByText("Change content type", { exact: true }).click();
  await page.getByLabel("Content type", { exact: true }).selectOption("code");
  await page.getByRole("button", { name: "Save type", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Could not save the type. Try again.");
  await page.getByRole("button", { name: "Code", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
});

test("type filtering and load more include items beyond the first page", async ({ page }) => {
  await page.goto("/?lang=en&manyCodes=yes");
  await page.getByRole("button", { name: "Code", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(50);
  await page.getByRole("button", { name: "Load more", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(63);
  await expect(page.getByRole("button", { name: "Load more", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Links", exact: true }).click();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
});
