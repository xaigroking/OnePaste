import { expect, test } from "@playwright/test";

test("passphrase dialog reuses strength feedback, checks confirmation, and supports keyboard cancellation", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const opener = page.getByRole("button", { name: "Change passphrase", exact: true });
  await opener.click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByLabel("Current passphrase", { exact: true })).toBeFocused();
  await dialog.getByLabel("New passphrase", { exact: true }).fill("short");
  await expect(dialog.locator(".strength")).toContainText("Weak");
  await dialog.getByLabel("Confirm new passphrase", { exact: true }).fill("different");
  await expect(dialog.getByRole("button", { name: "Save new passphrase", exact: true })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(opener).toBeFocused();
});

test("passphrase change succeeds without printing credentials and clears the form", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  await page.getByRole("button", { name: "Change passphrase", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Current passphrase", { exact: true }).fill("old-probe-passphrase");
  await dialog.getByLabel("New passphrase", { exact: true }).fill("new-probe-passphrase");
  await dialog.getByLabel("Confirm new passphrase", { exact: true }).fill("new-probe-passphrase");
  await dialog.getByRole("button", { name: "Save new passphrase", exact: true }).click();
  await expect(dialog.getByRole("status")).toContainText("Passphrase changed");
  await expect(dialog.locator("input")).toHaveCount(0);
});

test("a lost result stays visible after closing and can be explicitly confirmed", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&passphraseError=passphrase_pending");
  const opener = page.getByRole("button", { name: "Change passphrase", exact: true });
  await opener.click(); const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Current passphrase", { exact: true }).fill("old-probe");
  await dialog.getByLabel("New passphrase", { exact: true }).fill("new-probe");
  await dialog.getByLabel("Confirm new passphrase", { exact: true }).fill("new-probe");
  await dialog.getByRole("button", { name: "Save new passphrase", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("server result could not be confirmed");
  await expect(dialog.locator("input")).toHaveCount(0);
  await page.keyboard.press("Escape"); await expect(dialog).toBeHidden();
  await expect(page.getByRole("status").filter({ hasText: "A previous save has not been confirmed" })).toBeVisible();
  await opener.click();
  await dialog.getByRole("button", { name: "Confirm previous change", exact: true }).click();
  await expect(dialog.getByRole("status")).toContainText("Passphrase changed");
});

test("unexpected failures cannot echo passphrases into a notice", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&passphraseError=private-probe-secret");
  await page.getByRole("button", { name: "Change passphrase", exact: true }).click(); const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Current passphrase", { exact: true }).fill("old");
  await dialog.getByLabel("New passphrase", { exact: true }).fill("new");
  await dialog.getByLabel("Confirm new passphrase", { exact: true }).fill("new");
  await dialog.getByRole("button", { name: "Save new passphrase", exact: true }).click();
  await expect(dialog.getByRole("alert")).not.toContainText("private-probe-secret");
  await expect(dialog.getByRole("alert")).toContainText("Could not confirm the change");
});

test("local-only settings have no passphrase action", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&mode=local");
  await expect(page.getByRole("button", { name: "Change passphrase", exact: true })).toHaveCount(0);
  await expect(page.getByText("Change passphrase", { exact: true })).toHaveCount(0);
});

test("generated passphrases can be read, hidden, and copied through the existing secret-copy command", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  await page.getByRole("button", { name: "Change passphrase", exact: true }).click(); const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "Make one for me", exact: true }).click();
  const field = dialog.getByLabel("New passphrase", { exact: true });
  await expect(field).toHaveAttribute("type", "text");
  await expect(field).toHaveValue("7K3M-QX9D-4HFA-W2JB-N8RT");
  await expect(dialog.locator(".strength")).toHaveText("Strong.");
  await dialog.getByRole("button", { name: "Copy new passphrase", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__called)).toEqual(["copySecret"]);
  await dialog.getByRole("button", { name: "Hide new passphrase", exact: true }).click();
  await expect(field).toHaveAttribute("type", "password");
});
