import { expect, test } from "@playwright/test";

test("a fresh profile requires an independent application passphrase before choosing a mode", async ({ page }) => {
  await page.goto("/?screen=ob1&lang=en&keyState=new");
  await expect(page.getByRole("heading", { name: "Set a OnePaste passphrase" })).toBeVisible();
  await expect(page.getByText(/does not ask for your system login/)).toBeVisible();
  const next=page.getByRole("button", { name:"Continue",exact:true }); await expect(next).toBeDisabled();
  await page.getByLabel("New OnePaste passphrase",{exact:true}).fill("independent fixture password");
  await page.getByLabel("Confirm OnePaste passphrase",{exact:true}).fill("different fixture"); await expect(next).toBeDisabled();
  await page.getByLabel("Confirm OnePaste passphrase",{exact:true}).fill("independent fixture password");
  await next.click(); await expect(page.getByRole("radio",{name:/Only on this computer/})).toBeVisible();
  expect(await page.evaluate(()=>window.__called)).toContain("prepareLocalPassphrase");
});

test("locked startup never flashes onboarding and cancellation performs no key operation", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyState=locked");
  const input=page.getByLabel("OnePaste passphrase",{exact:true}); await expect(input).toBeFocused();
  await expect(page.getByRole("option")).toHaveCount(0);
  await input.fill("cancelled fixture"); await page.getByRole("button",{name:"Cancel",exact:true}).click(); await expect(input).toHaveValue("");
  expect(await page.evaluate(()=>window.__called??[])).not.toContain("unlockLocal");
  await input.fill("valid fixture password"); await input.press("Enter"); await expect(page.getByRole("option")).toHaveCount(6);
});

test("an unlock failure cannot echo arbitrary credentials and keeps access locked", async ({ page }) => {
  const marker="private-probe-never-echo-this";
  await page.goto(`/?screen=main&lang=en&keyState=locked&localError=${marker}`);
  await page.getByLabel("OnePaste passphrase",{exact:true}).fill(marker);
  await page.getByRole("button",{name:"Unlock",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("Your files have been kept");
  await expect(page.getByRole("alert")).not.toContainText(marker); await expect(page.getByRole("option")).toHaveCount(0);
});

test("legacy data offers explicit recovery without initializing a new profile", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyState=recovery");
  await expect(page.getByText(/Existing encrypted data was found/)).toBeVisible();
  await expect(page.getByRole("radio")).toHaveCount(0); await expect(page.getByRole("option")).toHaveCount(0);
  await page.getByRole("button",{name:"Recover existing data",exact:true}).click();
  await page.getByLabel("Recovery kit",{exact:true}).fill("fixture kit");
  await page.getByLabel("New OnePaste passphrase",{exact:true}).fill("recovered fixture password");
  await page.getByLabel("Confirm OnePaste passphrase",{exact:true}).fill("recovered fixture password");
  await page.getByRole("button",{name:"Recover existing data",exact:true}).click();
  await expect(page.getByRole("option")).toHaveCount(6);
  const calls=await page.evaluate(()=>window.__called??[]); expect(calls).toContain("recoverLocal"); expect(calls).not.toContain("prepareLocalPassphrase");
});

test("an access read failure blocks both onboarding and history", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyRead=error"); await expect(page.getByRole("alert")).toContainText("existing files have been kept");
  await expect(page.getByRole("option")).toHaveCount(0); await expect(page.getByRole("radio")).toHaveCount(0);
});


test("vault recovery preserves every passphrase character, including surrounding spaces", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyState=recovery&credentialFixture=spaces");
  await page.getByRole("button", { name: "Recover existing data", exact: true }).click();
  await page.getByRole("combobox", { name: "Recover using", exact: true }).selectOption("vault");
  await page.getByLabel("Saved vault passphrase", { exact: true }).fill("  fixture vault passphrase  ");
  await page.getByLabel("New OnePaste passphrase", { exact: true }).fill("independent recovery fixture");
  await page.getByLabel("Confirm OnePaste passphrase", { exact: true }).fill("independent recovery fixture");
  await page.getByRole("button", { name: "Recover existing data", exact: true }).click();
  await expect(page.getByRole("option")).toHaveCount(6);
});
