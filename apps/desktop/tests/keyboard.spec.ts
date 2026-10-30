import { expect, test } from "@playwright/test";
import type { AppSettings } from "../src/api";

// Keyboard walkthroughs. The page is the quick panel and the first-run screens with the stand-in api.

declare global {
  interface Window {
    __copied?: string[];
    __hidden?: boolean;
    __called?: string[];
    __settingsCalls?: AppSettings[];
  }
}

test("suspected secret filter switches with the keyboard and preserves other settings", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const toggle = page.getByRole("switch", { name: /Do not record things that look like keys or tokens/ });
  await expect(toggle).toBeEnabled();
  await expect(toggle).toBeChecked();
  await toggle.focus();
  await page.keyboard.press("Space");
  await expect(toggle).not.toBeChecked();
  const original = { autoApplyRemote: true, recordClipboard: true, maxItemMiB: 8, checkUpdates: true, updateChannel: null, skippedUpdate: null, trayNoticeDismissed: false, textPreviewHeight: null };
  await expect.poll(() => page.evaluate(() => window.__settingsCalls)).toEqual([{ ...original, skipSuspectedSecrets: false }]);
  await page.keyboard.press("Space");
  await expect(toggle).toBeChecked();
  await expect.poll(() => page.evaluate(() => window.__settingsCalls?.at(-1))).toEqual({ ...original, skipSuspectedSecrets: true });
  const scope = page.getByText("What this detects", { exact: true });
  await scope.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("details").filter({ has: scope })).toHaveAttribute("open", "");
  await expect(page.getByText(/These are offline heuristics/)).toBeVisible();
  await expect(page.getByText(/Turning this off still respects system sensitive markers/)).toBeVisible();
});

test("suspected secret filter reports a failed save and keeps the saved value", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&settingsSave=error");
  const toggle = page.getByRole("switch", { name: /Do not record things that look like keys or tokens/ });
  await expect(toggle).toBeEnabled();
  await toggle.click();
  await expect(page.getByRole("alert")).toHaveText("Could not save this setting. Try again.");
  await expect(toggle).toBeChecked();
  await expect.poll(() => page.evaluate(() => window.__settingsCalls?.length)).toBe(1);
  await page.reload();
  await expect(toggle).toBeChecked();
});

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

test("picture: the viewer zooms with + - 0, switches on double click and closes with Escape; save as asks for a place", async ({ page }) => {
  await page.goto("/?screen=img&lang=en");
  await page.getByRole("option").nth(4).click();
  await expect(page.getByTestId("image-meta")).toContainText("1280\u00d7720");
  await page.getByRole("button", { name: "Save as\u2026" }).click();
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["saveImage"]);
  await page.getByRole("button", { name: "View larger" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const level = page.getByTestId("zoom-level");
  await expect(level).toHaveText("Fit");
  await page.keyboard.press("+");
  await expect(level).toHaveText("125%");
  await page.keyboard.press("-");
  await page.keyboard.press("-");
  await expect(level).toHaveText("80%");
  await page.keyboard.press("0");
  await expect(level).toHaveText("Fit");
  await dialog.locator(".viewer-pane").dblclick();
  await expect(level).toHaveText("100%");
  await dialog.locator(".viewer-pane").dblclick();
  await expect(level).toHaveText("Fit");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});

test("long text: the box can be dragged taller and remembers its height, Expand shows everything", async ({ page }) => {
  await page.goto("/?screen=longtext&lang=en");
  const box = page.getByTestId("text-box");
  await expect(box).toBeVisible();
  // The corner handle: the box is resizable vertically, never below three lines
  expect(await box.evaluate((el) => getComputedStyle(el).resize)).toBe("vertical");
  expect(await box.evaluate((el) => parseFloat(getComputedStyle(el).minHeight))).toBeGreaterThanOrEqual(72);
  // Only the first lines are in the box; the rest is behind "Expand"
  await expect(box).not.toContainText("Line 30");
  await page.getByRole("button", { name: "Expand" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Line 34");
  await page.getByRole("button", { name: "Copy all" }).click();
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i1"]);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
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

for (const failure of [false, true]) {
  test(`self-hosted help preserves input without pairing (failure=${failure})`, async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator.clipboard, "readText", { value: async () => {
        window.__called = [...(window.__called ?? []), "readClipboard"];
        return "test-only-clipboard";
      } });
    });
    await page.goto(`/?screen=ob1&lang=en&help=${failure ? "error" : "ok"}`);
    await page.locator('input[value="server"]').check();
    await page.getByRole("button", { name: "Next", exact: true }).click();
    await page.getByLabel("Connection code", { exact: true }).fill("onepaste1:test-only");
    await page.getByLabel("Name of this device").fill("Test computer");
    const summary = page.locator("summary");
    await expect(page.locator("details")).not.toHaveAttribute("open", "");
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Shift+Tab");
    await expect(summary).toBeFocused();
    await expect(summary).toHaveCSS("outline-style", "solid");
    await page.keyboard.press("Enter");
    await expect(page.locator("details")).toHaveAttribute("open", "");
    await expect(page.locator("details")).toContainText("onepaste1:");
    await summary.click();
    await expect(page.locator("details")).not.toHaveAttribute("open", "");
    expect(await page.evaluate(() => window.__called ?? [])).toEqual([]);
    await page.getByRole("button", { name: "View self-hosting steps" }).click();
    await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["openServerHelp"]);
    await expect(page.getByLabel("Connection code", { exact: true })).toHaveValue("onepaste1:test-only");
    await expect(page.getByLabel("Name of this device")).toHaveValue("Test computer");
    if (failure) await expect(page.getByRole("alert")).toContainText("Could not open");
  });
}

for (const vault of ["empty", "existing"]) {
  test(`first run: ${vault} vault follows the correct registration path`, async ({ page }) => {
    await page.goto(`/?screen=ob1&lang=en&vault=${vault}`);
    await page.locator('input[value="server"]').check();
    await page.getByRole("button", { name: "Next", exact: true }).click();
    await page.getByLabel("Connection code", { exact: true }).fill("onepaste1:test-only");
    await page.getByRole("button", { name: "Next", exact: true }).click();
    expect(await page.evaluate(() => window.__called)).toEqual(["probeServer"]);
    if (vault === "existing") {
      await page.getByLabel("Passphrase", { exact: true }).fill("test-only-original-passphrase");
      await page.getByRole("button", { name: "Join", exact: true }).click();
      await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["probeServer", "join"]);
    } else {
      await page.getByLabel("Passphrase", { exact: true }).fill("test-only-new-passphrase");
      await page.getByLabel("Type it again").fill("test-only-new-passphrase");
      await page.getByRole("button", { name: "Next", exact: true }).click();
      await expect(page.getByTestId("kit")).toBeVisible();
      await expect(page.getByRole("button", { name: "Done", exact: true })).toBeDisabled();
      expect(await page.evaluate(() => window.__called)).toEqual(["probeServer", "createVault"]);
      const download = page.waitForEvent("download");
      await page.getByRole("button", { name: "Save as file" }).click();
      expect((await download).suggestedFilename()).toBe("onepaste-recovery-kit.txt");
      await page.getByRole("checkbox").check();
      expect(await page.evaluate(() => window.__called)).toEqual(["probeServer", "createVault"]);
      await page.getByRole("button", { name: "Done", exact: true }).click();
      await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["probeServer", "createVault", "confirmKitSaved"]);
    }
  });
}

test("devices: new code explains joining and reuses the copy action", async ({ page }) => {
  await page.goto("/?screen=dev&lang=en");
  await page.getByRole("button", { name: "Make a connection code" }).click();
  await expect(page.getByTestId("connection-code")).toBeVisible();
  await expect(page.getByText(/On the second desktop/)).toContainText("single-use");
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["newConnectionCode", "copySecret"]);
});
