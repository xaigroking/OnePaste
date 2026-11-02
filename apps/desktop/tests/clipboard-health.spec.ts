import { expect, test } from "@playwright/test";

for (const [state, message] of [["start_failed", "Clipboard listener unavailable"], ["read_error", "Could not read the clipboard"]]) {
  test(`listener health is independent of the enabled recording switch: ${state}`, async ({ page }) => {
    await page.goto(`/?screen=main&lang=en&capture=${state}`);
    await expect(page.getByText(message, { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText("Listening", { exact: true })).toHaveCount(0);
    await page.getByRole("button", { name: "Retry listening", exact: true }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByText("Listening", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => window.__called ?? [])).toContain("retryClipboardMonitor");
  });
}


test("a failed retry remains visible and never echoes a backend error", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&capture=start_failed&captureRetry=error");
  await page.getByRole("button",{name:"Retry listening",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("Could not restart the listener");
  await expect(page.getByText("Listening",{exact:true})).toHaveCount(0);
  await expect(page.getByRole("button",{name:"Retry listening",exact:true})).toBeEnabled();
});

for (const lang of ["en","zh"]) for (const theme of ["light","dark"]) for (const state of ["start_failed","read_error","starting","stopped"]) {
  test(`capture health ${state} ${theme} ${lang}`,async({page})=>{
    await page.setViewportSize({width:1040,height:640});
    await page.addInitScript((value)=>localStorage.setItem("onepaste.theme",value),theme);
    await page.goto(`/?screen=main&lang=${lang}&capture=${state}`);
    await expect(page.getByRole("option")).toHaveCount(6);
    await page.evaluate(()=>document.fonts.ready);
    await expect(page).toHaveScreenshot(`capture-health-${state}-${theme}-${lang}.png`);
  });
}
