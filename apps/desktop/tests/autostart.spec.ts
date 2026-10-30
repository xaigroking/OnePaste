import { expect, test } from "@playwright/test";

test("login startup is off by default and supports keyboard on-off-on", async ({ page }) => {
  await page.goto("/?screen=set&lang=en");
  const toggle = page.getByRole("switch", { name: /Launch at login/ });
  await expect(toggle).toBeEnabled();
  await expect(toggle).not.toBeChecked();
  await toggle.focus();
  for (const enabled of [true, false, true]) {
    await page.keyboard.press("Space");
    await expect(toggle).toBeChecked({ checked: enabled });
  }
});

test("returning to settings reads external deletion instead of a cached startup preference", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&autostart=on");
  const toggle = page.getByRole("switch", { name: /Launch at login/ });
  await expect(toggle).toBeChecked();
  await page.evaluate(() => { window.dispatchEvent(new Event("onepaste-test-remove-autostart")); window.dispatchEvent(new Event("focus")); });
  await expect(toggle).not.toBeChecked();
});

test("a registration failure reports recovery and keeps the actual saved state", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&autostartSave=error");
  const toggle = page.getByRole("switch", { name: /Launch at login/ });
  await toggle.click();
  await expect(page.getByRole("alert")).toContainText("Could not update login startup");
  await expect(toggle).not.toBeChecked();
});

test("a temporary macOS download location explains why startup cannot be enabled", async ({ page }) => {
  await page.goto("/?screen=set&lang=en&autostart=unstable");
  await expect(page.getByRole("switch", { name: /Launch at login/ })).toBeDisabled();
  await expect(page.getByText(/Move OnePaste to a permanent folder/)).toBeVisible();
});
