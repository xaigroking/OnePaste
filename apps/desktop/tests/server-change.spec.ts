import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

declare global {
  interface Window {
    __serverChanges?: { code: string; passphrase: string | null; recoveryKit: string | null }[];
    __finishDeviceAction?: (fail?: boolean) => void;
  }
}

const CODE = "onepaste1:synthetic-change-test";
async function openCode(page: Page, query = "") {
  await page.goto(`/?screen=set&lang=en${query}`);
  await page.getByRole("button", { name: "Change server", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Enter a new code" }).click();
  return dialog;
}

test("change server: keyboard flow keeps mutations behind confirmation and restores focus", async ({ page }) => {
  await page.goto("/?screen=main&lang=en");
  await page.getByRole("button", { name: "Settings", exact: true }).focus();
  await page.keyboard.press("Enter");
  const entry = page.getByRole("button", { name: "Change server", exact: true });
  await entry.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "Enter a new code" })).toBeFocused();
  await page.keyboard.press("Enter");
  const input = dialog.getByLabel("Connection code", { exact: true });
  await expect(input).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(dialog.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(input).toBeFocused();
  await input.pressSequentially(CODE);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "Check server" })).toBeFocused();
  await expect(dialog.getByRole("button", { name: "Check server" })).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("Enter");
  await expect(dialog.getByText("The new server is empty.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Review change" })).toBeFocused();
  await page.keyboard.press("Enter");
  for (const text of ["Your local history and vault key will be kept.", "This computer's device record will remain on the old server.", "Other devices must each reconnect using a fresh code from the new server."]) {
    await expect(dialog.getByText(text, { exact: true })).toBeVisible();
  }
  expect(await page.evaluate(() => window.__serverChanges ?? [])).toEqual([]);
  await expect(dialog.getByRole("button", { name: "Confirm change" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(dialog.getByRole("heading", { name: "Server changed" })).toBeVisible();
  expect(await page.evaluate(() => window.__serverChanges)).toEqual([{ code: CODE, passphrase: null, recoveryKit: null }]);
  expect(await page.evaluate(() => window.__called)).toEqual(["probeServer", "changeServer"]);
  await expect(dialog.getByRole("button", { name: "Done", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(dialog).toBeHidden();
  await expect(entry).toBeFocused();
  await expect(page.getByText("https://new-server.example:8443", { exact: true })).toBeVisible();
});

for (const recovery of [false, true]) {
  test(`change server: existing vault uses only the selected credential (kit=${recovery})`, async ({ page }) => {
    const dialog = await openCode(page, "&serverChangeVault=existing");
    await dialog.getByLabel("Connection code", { exact: true }).fill(CODE);
    await dialog.getByRole("button", { name: "Check server" }).click();
    await expect(dialog.getByText("The new server already has a vault.")).toBeVisible();
    if (recovery) await dialog.getByRole("radio", { name: "Use recovery kit" }).check();
    await dialog.getByLabel(recovery ? "Recovery kit" : "Passphrase", { exact: true }).fill("synthetic-credential");
    await dialog.getByRole("button", { name: "Review change" }).click();
    expect(await page.evaluate(() => window.__serverChanges ?? [])).toEqual([]);
    await dialog.getByRole("button", { name: "Confirm change" }).click();
    await expect(dialog.getByRole("heading", { name: "Server changed" })).toBeVisible();
    expect(await page.evaluate(() => window.__serverChanges)).toEqual([{ code: CODE, passphrase: recovery ? null : "synthetic-credential", recoveryKit: recovery ? "synthetic-credential" : null }]);
  });
}

test("change server: failed credentials keep the old address and explain recovery", async ({ page }) => {
  const dialog = await openCode(page, "&serverChangeVault=existing&serverChangeCommit=error");
  await dialog.getByLabel("Connection code", { exact: true }).fill(CODE);
  await dialog.getByRole("button", { name: "Check server" }).click();
  await dialog.getByLabel("Passphrase", { exact: true }).fill("synthetic-wrong-passphrase");
  await dialog.getByRole("button", { name: "Review change" }).click();
  await dialog.getByRole("button", { name: "Confirm change" }).click();
  await expect(dialog.getByRole("alert")).toHaveText("The passphrase did not unlock that vault. Your local history and current connection are unchanged. Go back and correct the passphrase, or use the recovery kit.");
  await expect(dialog.getByRole("heading", { name: "Server changed" })).toHaveCount(0);
  await expect(page.getByText("https://192.168.1.20:8443", { exact: true })).toBeVisible();
});

test("change server: cancelling a pending probe never changes the connection", async ({ page }) => {
  const dialog = await openCode(page, "&serverChange=loading");
  await dialog.getByLabel("Connection code", { exact: true }).fill(CODE);
  await dialog.getByRole("button", { name: "Check server" }).click();
  await expect(dialog.getByRole("status")).toHaveText("Checking the new server...");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(await page.evaluate(() => window.__serverChanges ?? [])).toEqual([]);
});

test("change server: the device footer remains usable when the old server is offline", async ({ page }) => {
  await page.goto("/?screen=dev&lang=en&state=error");
  await expect(page.getByRole("alert")).toBeVisible();
  await page.getByRole("button", { name: "Change server", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("change server: local-only settings and devices expose no entry", async ({ page }) => {
  for (const screen of ["set", "dev"]) {
    await page.goto(`/?screen=${screen}&lang=en&mode=local`);
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    await expect(page.getByRole("button", { name: "Change server", exact: true })).toHaveCount(0);
  }
});

test("change server: unexpected invoke errors never expose their contents", async ({ page }) => {
  const dialog = await openCode(page, "&serverChangeCommit=unexpected");
  await dialog.getByLabel("Connection code", { exact: true }).fill(CODE);
  await dialog.getByRole("button", { name: "Check server" }).click();
  await dialog.getByRole("button", { name: "Review change" }).click();
  await dialog.getByRole("button", { name: "Confirm change" }).click();
  await expect(dialog.getByRole("alert")).toHaveText("The server change could not be confirmed. Your local history is kept. Close this dialog and reopen Settings to check the saved server address before retrying.");
  await expect(dialog).not.toContainText("do-not-show-error");
});

for (const action of ["code", "code-error", "revoke-error"]) {
  test(`change server: late old-server ${action} responses cannot change the new view`, async ({ page }) => {
    await page.goto("/?screen=dev&lang=en&deviceAction=pending");
    if (action === "revoke-error") {
      await page.getByRole("button", { name: "Remove", exact: true }).click();
      await page.getByRole("alertdialog").getByRole("button", { name: "Remove device", exact: true }).click();
    } else {
      await page.getByRole("button", { name: "Make a connection code", exact: true }).click();
    }
    await page.getByRole("button", { name: "Change server", exact: true }).click();
    const dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: "Enter a new code" }).click();
    await dialog.getByLabel("Connection code", { exact: true }).fill(CODE);
    await dialog.getByRole("button", { name: "Check server" }).click();
    await dialog.getByRole("button", { name: "Review change" }).click();
    await dialog.getByRole("button", { name: "Confirm change" }).click();
    await dialog.getByRole("button", { name: "Done", exact: true }).click();
    await expect(page.getByText("https://new-server.example:8443", { exact: false })).toBeVisible();
    await page.evaluate((fail) => window.__finishDeviceAction?.(fail), action !== "code");
    await expect(page.getByTestId("connection-code")).toHaveCount(0);
    await expect(page.getByRole("alert")).toHaveCount(0);
  });
}
