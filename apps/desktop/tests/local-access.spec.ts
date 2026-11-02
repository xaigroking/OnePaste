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

for (const entry of ["keyState=recovery", "keyState=locked", "keyRead=error"]) {
  test(`starting over needs no old credentials (${entry})`, async ({ page }) => {
    await page.goto(`/?screen=main&lang=en&${entry}`);
    const restart = page.getByRole("button", { name: "Start fresh", exact: true });
    await expect(restart).toBeVisible({ timeout: 5000 });
    await restart.click();
    await expect(page.getByRole("heading", { name: "Start fresh on this computer?" })).toBeVisible();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(await page.evaluate(() => window.__called ?? [])).not.toContain("restartLocalSetup");
    await restart.click();
    await page.getByRole("button", { name: "Start fresh", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Set a OnePaste passphrase" })).toBeVisible();
    await page.getByLabel("New OnePaste passphrase", { exact: true }).fill("new independent passphrase");
    await page.getByLabel("Confirm OnePaste passphrase", { exact: true }).fill("new independent passphrase");
    await page.getByRole("button", { name: "Continue", exact: true }).click();
    await page.getByRole("radio", { name: /Only on this computer/ }).check();
    await page.getByRole("button", { name: "Next", exact: true }).click();
    await expect.poll(() => page.evaluate(() => window.__called ?? [])).toContain("chooseLocalOnly");
    const calls = await page.evaluate(() => window.__called ?? []);
    expect(calls).toContain("restartLocalSetup");
    expect(calls).not.toContain("recoverLocal");
    expect(calls).not.toContain("unlockLocal");
  });
}

test("a restart error remains actionable and cancellation leaves the access state unchanged", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyState=recovery&restart=error");
  await page.getByRole("button", { name: "Start fresh", exact: true }).click();
  await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Start fresh", exact: true })).toBeFocused();
  expect(await page.evaluate(() => window.__called ?? [])).not.toContain("restartLocalSetup");
  await page.getByRole("button", { name: "Start fresh", exact: true }).click();
  await page.getByRole("button", { name: "Start fresh", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Could not start fresh");
  await expect(page.getByRole("button", { name: "Start fresh", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Recover existing OnePaste data" })).toBeVisible();
  await page.reload();
  expect(await page.evaluate(() => window.__called ?? [])).not.toContain("restartLocalSetup");
});

test("starting over is submitted once and reports progress", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&keyState=locked&restart=pending");
  await page.getByRole("button", { name: "Start fresh", exact: true }).click();
  await page.getByRole("button", { name: "Start fresh", exact: true }).click();
  await expect(page.getByRole("button", { name: "Working…", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Cancel", exact: true })).toBeDisabled();
  await page.keyboard.press("Escape");
  expect((await page.evaluate(() => window.__called ?? [])).filter((c) => c === "restartLocalSetup")).toHaveLength(1);
  await page.evaluate(() => (window as any).__finishRestart());
  await expect(page.getByRole("heading", { name: "Set a OnePaste passphrase" })).toBeVisible();
});

test("new synced setup can continue without displaying or saving a recovery kit", async ({ page }) => {
  await page.goto("/?screen=ob1&lang=en");
  await page.getByRole("radio", { name: /Connect to my own server/ }).check();
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await page.getByLabel("Connection code", { exact: true }).fill("onepaste1:fixture");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await page.getByLabel("Passphrase", { exact: true }).fill("new separate vault passphrase");
  await page.getByLabel("Type it again", { exact: true }).fill("new separate vault passphrase");
  await page.getByRole("button", { name: "Continue without a recovery kit", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__called ?? [])).toEqual(["probeServer", "createVault", "confirmKitSaved"]);
  await expect(page.getByTestId("kit")).toHaveCount(0);
});

test("displaying a kit does not require saving it to finish", async ({ page }) => {
  await page.goto("/?screen=ob1&lang=en");
  await page.getByRole("radio", { name: /Connect to my own server/ }).check();
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await page.getByLabel("Connection code", { exact: true }).fill("onepaste1:fixture");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await page.getByLabel("Passphrase", { exact: true }).fill("new separate vault passphrase");
  await page.getByLabel("Type it again", { exact: true }).fill("new separate vault passphrase");
  await page.getByRole("button", { name: "Next", exact: true }).click();
  await expect(page.getByRole("checkbox")).not.toBeChecked();
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__called ?? [])).toEqual(["probeServer", "createVault", "confirmKitSaved"]);
});
