import { expect, test } from "@playwright/test";
import type { TrayTexts } from "../src/api";

declare global {
  interface Window {
    __hotkeyCalls?: (string | null)[];
    __trayTexts?: TrayTexts;
  }
}

test("shortcut capture cancels with Escape, saves with the keyboard, and updates the tray label", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const change = page.getByRole("button", { name: "Change shortcut", exact: true });
  await change.click();
  const capture = page.getByRole("textbox", { name: "New shortcut", exact: true });
  await expect(capture).toBeFocused();
  await capture.press("Control+Alt+K");
  await expect(capture).toHaveValue("Ctrl+Alt+K");
  await capture.press("Escape");
  await expect(capture).toBeHidden();
  expect(await page.evaluate(() => window.__hotkeyCalls ?? [])).toEqual([]);
  await change.click();
  await capture.press("k");
  await expect(page.getByRole("alert")).toContainText("Use a modifier and a non-modifier key.");
  await capture.press("Control+Alt+K");
  const save = page.getByRole("button", { name: "Save shortcut", exact: true });
  await save.focus();
  await expect(save).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("Enter");
  await expect(page.getByText("Ctrl+Alt+K", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__hotkeyCalls)).toEqual(["Control+Alt+KeyK"]);
  await expect.poll(() => page.evaluate(() => window.__trayTexts?.quickPanel)).toBe("Quick panel (Ctrl+Alt+K)");
});

test("an occupied shortcut keeps the previous combination and explains how to retry", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&hotkeySave=busy");
  await page.getByRole("button", { name: "Change shortcut", exact: true }).click();
  await page.getByRole("textbox", { name: "New shortcut", exact: true }).press("Control+Alt+K");
  await page.getByRole("button", { name: "Save shortcut", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Your previous shortcut still works.");
  await expect(page.getByText("Ctrl+Alt+V", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__hotkeyCalls)).toEqual(["Control+Alt+KeyK"]);
});

test("the default can be restored from a saved custom shortcut", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&hotkey=custom");
  await expect(page.getByText("Ctrl+Shift+K", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Restore default shortcut", exact: true }).click();
  await expect(page.getByText("Ctrl+Alt+V", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__hotkeyCalls)).toEqual([null]);
});

test("the quick panel shows the active custom shortcut", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en&hotkey=custom#/quick");
  await expect(page.getByText("Ctrl+Shift+K", { exact: true })).toBeVisible();
});

test("failed registration without an active shortcut explains recovery without promising an old shortcut", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&hotkey=unavailable&hotkeySave=busy");
  await expect(page.getByText("No active shortcut", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Change shortcut", exact: true }).click();
  await page.getByRole("textbox", { name: "New shortcut", exact: true }).press("Control+Alt+K");
  await page.getByRole("button", { name: "Save shortcut", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("No global shortcut is active.");
  await expect(page.getByRole("alert")).not.toContainText("previous shortcut still works");
});

test("the standard paste shortcut is reserved before a registration request", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  await page.getByRole("button", { name: "Change shortcut", exact: true }).click();
  await page.getByRole("textbox", { name: "New shortcut", exact: true }).press("Control+V");
  await expect(page.getByRole("alert")).toContainText("standard paste shortcut is reserved");
  await expect(page.getByRole("button", { name: "Save shortcut", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => window.__hotkeyCalls ?? [])).toEqual([]);
});
