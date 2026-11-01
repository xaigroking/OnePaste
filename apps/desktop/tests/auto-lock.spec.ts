import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";

test("auto lock setting is keyboard accessible and preserves the other settings", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const choice = page.getByRole("combobox", { name: "Lock automatically after a while", exact: true });
  await expect(choice).toHaveValue("0"); await choice.focus();
  await expect(choice).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("ArrowDown"); await page.keyboard.press("Enter");
  await expect(choice).toHaveValue("1");
  await expect.poll(() => page.evaluate(() => (window as any).__settingsCalls?.at(-1)?.autoLockMinutes)).toBe(1);
  expect(await page.evaluate(() => (window as any).__settingsCalls?.at(-1)?.recordClipboard)).toBe(true);
});

test("failed auto lock save keeps the persisted interval and shows an error", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&settingsSave=error");
  const choice = page.getByRole("combobox", { name: "Lock automatically after a while", exact: true });
  await choice.selectOption("5"); await expect(page.getByRole("alert")).toContainText("Could not save or lock");
  await expect(choice).toHaveValue("0");
});

for (const lang of ["en", "zh"]) for (const theme of ["light", "dark"]) {
  test(`auto lock keyboard unlock and cancellation ${theme} ${lang}`, async ({ page }) => {
    const l = JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8"));
    await page.setViewportSize({ width: 1040, height: 900 });
    await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
    await page.goto(`/?screen=set&lang=${lang}&autoLockTest=1`);
    const lock = page.getByRole("button", { name: l.autoLock.now, exact: true });
    await lock.scrollIntoViewIfNeeded(); await expect(page.getByRole("combobox", { name: l.settingsMore.autoLock, exact: true })).toHaveValue("0");
    await expect(page).toHaveScreenshot(`auto-lock-settings-${theme}-${lang}.png`);
    await lock.focus(); await page.keyboard.press("Enter");
    const password = page.getByLabel(l.localAccess.pass, { exact: true });
    await expect(password).toBeFocused(); await expect(page.getByText(l.autoLock.paused)).toBeVisible();
    await expect(page.getByRole("combobox")).toHaveCount(0);
    await expect(page).toHaveScreenshot(`auto-lock-locked-${theme}-${lang}.png`);
    await password.fill("wrong secret must disappear"); await password.press("Enter");
    await expect(page.getByRole("alert")).toContainText(l.localAccess.wrong); await expect(password).toHaveValue("");
    await expect(page).toHaveScreenshot(`auto-lock-error-${theme}-${lang}.png`);
    await password.fill("cancelled"); await page.getByRole("button", { name: l.localAccess.cancel, exact: true }).click();
    await expect(password).toHaveValue(""); await expect(page.getByRole("combobox")).toHaveCount(0);
    await password.fill("correct-local-passphrase"); await password.press("Enter");
    await expect(page.getByRole("combobox", { name: l.settingsMore.autoLock, exact: true })).toBeVisible();
  });
}

test("locking the quick panel clears its results and a late search cannot restore them", async ({ page }) => {
  await page.goto("/?screen=qp&lang=en&autoLockTest=1#/quick");
  await expect(page.getByRole("option")).toHaveCount(6);
  await page.getByRole("searchbox").fill("late fixture");
  await expect.poll(() => page.evaluate(() => typeof (window as any).__releaseLockSearch)).toBe("function");
  await page.evaluate(() => { (window as any).__lockFixture(); });
  await expect(page.getByLabel("OnePaste passphrase", { exact: true })).toBeFocused();
  await expect(page.getByRole("option")).toHaveCount(0);
  await page.evaluate(() => { (window as any).__releaseLockSearch?.(); });
  await expect(page.getByRole("option")).toHaveCount(0);
});

for (const lang of ["en", "zh"]) for (const theme of ["light", "dark"]) for (const state of ["saving", "save-error"]) {
  test(`auto lock ${state} ${theme} ${lang}`, async ({ page }) => {
    const l = JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8"));
    await page.setViewportSize({ width: 1040, height: 900 });
    await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
    await page.goto(`/?screen=set&lang=${lang}&${state === "saving" ? "autoLockSave=pending" : "settingsSave=error"}`);
    const choice = page.getByRole("combobox", { name: l.settingsMore.autoLock, exact: true });
    await choice.selectOption("5");
    await expect(page.getByText(state === "saving" ? l.autoLock.saving : l.autoLock.failed, { exact: true })).toBeVisible();
    if (state === "saving") await expect(choice).toBeDisabled(); else await expect(choice).toHaveValue("0");
    await expect(page).toHaveScreenshot(`auto-lock-${state}-${theme}-${lang}.png`);
    if (state === "saving") { await page.evaluate(() => (window as any).__finishAutoLockSave()); await expect(choice).toHaveValue("5"); await expect(choice).toBeEnabled(); }
  });
}
