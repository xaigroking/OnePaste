import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { readFileSync } from "node:fs";

// One screenshot per screen, theme and language, and one per screen for the empty, loading and error states.
// Names: <screen>-<theme>-<lang>.png and <screen>-state-<state>.png (light, English).

const LANGS = ["en", "zh"] as const;
const THEMES = ["light", "dark"] as const;
const locale = (lang: string) => JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), "utf8"));

interface Screen {
  name: string;
  screen: string;
  size: { width: number; height: number };
  hash?: string;
  query?: string;
  states: ("empty" | "loading" | "error")[];
  /** Drives the page to the place the screenshot shows. */
  drive?: (page: Page, lang: string, state: string) => Promise<void>;
}

const WINDOW = { width: 1040, height: 640 };

for (const mode of ["recording", "conflict"] as const) {
  for (const theme of THEMES) for (const lang of LANGS) {
    test(`hotkey ${mode} ${theme} ${lang}`, async ({ page }) => {
      const l = locale(lang);
      await page.setViewportSize({ width: 1040, height: 900 });
      await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
      await page.goto(`/?screen=set&lang=${lang}${mode === "conflict" ? "&hotkeySave=busy" : ""}`);
      await page.getByRole("button", { name: l.hotkey.change, exact: true }).click();
      await page.getByRole("textbox", { name: l.hotkey.new, exact: true }).press("Control+Alt+K");
      if (mode === "conflict") {
        await page.getByRole("button", { name: l.hotkey.save, exact: true }).click();
        await page.getByRole("alert").waitFor();
      }
      await page.locator(".card").filter({ has: page.getByRole("heading", { name: l.settings.shortcuts, exact: true }) }).scrollIntoViewIfNeeded();
      await page.evaluate(() => document.fonts.ready);
      await expect(page).toHaveScreenshot(`hotkey-${mode}-${theme}-${lang}.png`);
    });
  }
}

async function toCodeStep(page: Page, lang: string) {
  const l = locale(lang);
  await page.locator('input[value="server"]').check();
  await page.getByRole("button", { name: l.onboarding.next }).click();
  await page.locator("input").first().fill("onepaste1:example");
}

const SCREENS: Screen[] = [
  { name: "startup-on", screen: "set", size: WINDOW, query: "autostart=on", states: [] },
  { name: "startup-unavailable", screen: "set", size: WINDOW, query: "autostart=unstable", states: [] },
  { name: "startup-error", screen: "set", size: WINDOW, query: "autostartSave=error", states: [],
    drive: async (page, lang) => {
      await page.getByRole("switch", { name: new RegExp(locale(lang).settingsMore.startup) }).click();
      await page.getByRole("alert").waitFor();
    } },
  { name: "item-size-limit", screen: "set", size: { width: 1040, height: 900 }, states: [],
    drive: async (page, lang) => {
      await page.getByRole("combobox", { name: locale(lang).settingsMore.sizeLimit, exact: true }).scrollIntoViewIfNeeded();
    } },
  { name: "item-size-notice", screen: "main", size: WINDOW, states: [], query: "captureLimit=1" },
  { name: "secret-filter", screen: "set", size: { width: 1040, height: 900 }, states: [],
    drive: async (page, lang) => {
      await page.getByText(locale(lang).settingsMore.skipSecretsScope, { exact: true }).click();
      await page.getByRole("heading", { name: locale(lang).settings.clipboard, exact: true }).scrollIntoViewIfNeeded();
    } },
  { name: "server-help", screen: "ob1", size: { width: 1040, height: 900 }, states: [],
    drive: async (page, lang) => {
      await toCodeStep(page, lang);
      await page.locator("summary").click();
    } },
  { name: "device-code", screen: "dev", size: { width: 1040, height: 900 }, states: [],
    drive: async (page, lang) => {
      await page.getByRole("button", { name: locale(lang).devices.addMake }).click();
      await page.getByTestId("connection-code").waitFor();
    } },
  { name: "main", screen: "main", size: WINDOW, states: ["empty", "loading", "error"] },
  { name: "dev", screen: "dev", size: WINDOW, states: ["empty", "loading", "error"] },
  { name: "set", screen: "set", size: WINDOW, states: ["loading", "error"] },
  { name: "qp", screen: "qp", size: { width: 560, height: 520 }, hash: "#/quick", states: ["empty", "loading", "error"] },
  { name: "update", screen: "update", size: WINDOW, states: [] },
  { name: "upd", screen: "upd", size: WINDOW, states: ["error"],
    drive: async (page, lang, state) => {
      const l = locale(lang);
      await page.getByRole("heading", { name: l.updates.title }).scrollIntoViewIfNeeded();
      if (state === "error") await page.getByRole("button", { name: l.updates.checkNow }).click();
    } },
  { name: "img", screen: "img", size: WINDOW, states: [],
    drive: async (page) => {
      await page.getByRole("option").nth(4).click();
      await page.getByTestId("image-meta").waitFor();
      await page.waitForTimeout(500);
    } },
  { name: "viewer", screen: "viewer", size: WINDOW, states: [],
    drive: async (page) => {
      await page.getByRole("option").nth(4).click();
      await page.getByRole("img").last().click();
      await page.getByRole("dialog").waitFor();
      await page.waitForTimeout(300);
    } },
  { name: "longtext", screen: "longtext", size: WINDOW, states: [] },
  { name: "textview", screen: "textview", size: WINDOW, states: [],
    drive: async (page, lang) => {
      await page.getByRole("button", { name: locale(lang).text.expand }).click();
      await page.getByRole("dialog").waitFor();
    } },
  { name: "tray", screen: "tray", size: { width: 330, height: 360 }, states: [] },
  { name: "state", screen: "state", size: WINDOW, states: [] },
  { name: "ob1", screen: "ob1", size: WINDOW, states: ["loading", "error"],
    drive: async (page, lang, state) => {
      if (state === "base") return;
      const l = locale(lang);
      await toCodeStep(page, lang);
      await page.getByRole("button", { name: l.onboarding.next }).click();
    } },
  { name: "ob2", screen: "ob2", size: WINDOW, states: [],
    drive: async (page, lang) => {
      const l = locale(lang);
      await toCodeStep(page, lang);
      await page.getByRole("button", { name: l.onboarding.next }).click();
      await page.getByRole("button", { name: l.onboarding.passphraseStep.generate }).click();
      await page.getByRole("button", { name: l.onboarding.next }).click();
      await page.getByTestId("kit").waitFor();
    } },
];

async function open(page: Page, s: Screen, theme: string, lang: string, state: string) {
  await page.setViewportSize(s.size);
  await page.addInitScript((t) => window.localStorage.setItem("onepaste.theme", t), theme);
  await page.goto(`/?screen=${s.screen}&lang=${lang}&state=${state}${s.query ? `&${s.query}` : ""}${s.hash ?? ""}`);
  await page.waitForLoadState("networkidle");
  await s.drive?.(page, lang, state);
  // Let the first render and the fonts settle
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
}

for (const s of SCREENS) {
  for (const theme of THEMES) {
    for (const lang of LANGS) {
      test(`${s.name} ${theme} ${lang}`, async ({ page }) => {
        await open(page, s, theme, lang, "base");
        await expect(page).toHaveScreenshot(`${s.name}-${theme}-${lang}.png`);
      });
    }
  }
  for (const state of s.states) {
    test(`${s.name} state ${state}`, async ({ page }) => {
      await open(page, s, "light", "en", state);
      await expect(page).toHaveScreenshot(`${s.name}-state-${state}.png`);
    });
  }
}

for (const state of ["empty", "loading", "error"] as const) {
  for (const theme of THEMES) {
    for (const lang of LANGS) {
      test(`server-change ${state} ${theme} ${lang}`, async ({ page }) => {
        const l = locale(lang);
        await page.setViewportSize({ width: 1040, height: 900 });
        await page.addInitScript((value) => window.localStorage.setItem("onepaste.theme", value), theme);
        await page.goto(`/?screen=set&lang=${lang}&serverChange=${state}`);
        await page.getByRole("button", { name: l.changeServer.action, exact: true }).click();
        const dialog = page.getByRole("dialog");
        await dialog.getByRole("button", { name: l.changeServer.start }).click();
        if (state !== "empty") {
          await dialog.getByLabel(l.onboarding.code.code, { exact: true }).fill("onepaste1:synthetic-screenshot");
          await dialog.getByRole("button", { name: l.changeServer.check }).click();
          if (state === "loading") await expect(dialog.getByRole("status")).toHaveText(l.changeServer.checking);
          else await expect(dialog.getByRole("alert")).toHaveText(l.changeServer.errors.unreachable);
        }
        await page.evaluate(() => document.fonts.ready);
        await expect(page).toHaveScreenshot(`server-change-${state}-${theme}-${lang}.png`);
      });
    }
  }
}

for (const mode of ["selection", "confirmation", "undo"] as const) {
  for (const theme of THEMES) for (const lang of LANGS) {
    test(`history batch ${mode} ${theme} ${lang}`, async ({ page }) => {
      const l=locale(lang); await page.setViewportSize({width:1040,height:780});
      await page.addInitScript((value)=>localStorage.setItem("onepaste.theme",value),theme);
      await page.clock.install(); await page.goto(`/?screen=main&lang=${lang}`);
      const rows=page.getByRole("option"); await rows.nth(0).click(); await rows.nth(2).click({modifiers:["Control"]});
      await page.clock.pauseAt(new Date(Date.now()+2000));
      if (mode!=="selection") await page.getByRole("button",{name:l.batch.delete,exact:true}).click();
      if (mode==="undo") { await page.getByRole("dialog").getByRole("button",{name:l.detail.delete,exact:true}).click(); await page.getByRole("button",{name:l.batch.undo,exact:true}).waitFor(); }
      await page.evaluate(()=>document.fonts.ready);
      await expect(page).toHaveScreenshot(`history-batch-${mode}-${theme}-${lang}.png`);
    });
  }
}
