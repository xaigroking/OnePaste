import { expect, test } from "@playwright/test";

test("a keyboard pick uses the list that was just committed to the page", async ({ page }) => {
  await page.addInitScript(() => {
    let picked = false;
    new MutationObserver(() => {
      if (!picked && document.querySelectorAll('[role="option"]').length >= 2) {
        picked = true;
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "2", bubbles: true }));
      }
    }).observe(document, { childList: true, subtree: true });
  });
  await page.goto("/?screen=qp&lang=en#/quick");
  await expect.poll(() => page.evaluate(() => window.__copied)).toEqual(["i2"]);
});
