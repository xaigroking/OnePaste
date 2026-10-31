import { expect, test } from "@playwright/test";

declare global { interface Window { __deleteStages?: string[][]; __deleteUndos?: string[]; __deleted?: string[]; __batchCopies?: string[][]; } }

test("Control-click adds to selection, Shift-click selects a range, and Escape clears it", async ({ page }) => {
  await page.goto("/?screen=main&lang=en");
  const rows = page.getByRole("option");
  await rows.nth(0).click(); await rows.nth(1).click({ modifiers: ["Control"] });
  await expect(page.getByRole("option", { selected: true })).toHaveCount(2);
  await rows.nth(3).click({ modifiers: ["Shift"] });
  await expect(page.getByRole("option", { selected: true })).toHaveCount(3);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("option", { selected: true })).toHaveCount(0);
});

test("batch delete confirms its scope and can be undone before any deletion is submitted", async ({ page }) => {
  await page.goto("/?screen=main&lang=en");
  const rows = page.getByRole("option");
  await rows.nth(0).click(); await rows.nth(1).click({ modifiers: ["Control"] });
  await page.getByRole("button", { name: "Delete selected (syncs to all devices)", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("at least 10 seconds");
  await dialog.getByRole("button", { name: "Delete (syncs to all devices)", exact: true }).click();
  await expect(page.getByRole("button", { name: "Undo deletion", exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__deleted ?? [])).toEqual([]);
  await page.getByRole("button", { name: "Undo deletion", exact: true }).click();
  await expect(page.getByRole("button", { name: "Undo deletion", exact: true })).toHaveCount(0);
  await expect(rows).toHaveCount(6);
  expect(await page.evaluate(() => window.__deleted ?? [])).toEqual([]);
});

test("keyboard selection supports Space, ranges and Escape", async ({ page }) => {
  await page.goto("/?screen=main&lang=en"); const rows=page.getByRole("option");
  await rows.nth(0).click(); await page.keyboard.press("Control+ArrowDown");
  await expect(rows.nth(1)).toBeFocused(); await expect(page.getByRole("option",{selected:true})).toHaveCount(1);
  await page.keyboard.press("Space"); await expect(page.getByRole("option",{selected:true})).toHaveCount(2);
  await page.keyboard.press("Shift+ArrowDown"); await expect(rows.nth(2)).toBeFocused();
  await expect(page.getByRole("option",{selected:true})).toHaveCount(2);
  await page.keyboard.press("Escape"); await expect(page.getByRole("option",{selected:true})).toHaveCount(0);
  await expect(rows.nth(2)).toBeFocused();
});

test("delete confirmation defaults to cancel, traps focus and returns it on Escape", async ({ page }) => {
  await page.goto("/?screen=main&lang=en"); await page.getByRole("option").first().click(); await page.keyboard.press("Delete");
  const dialog=page.getByRole("dialog"); const cancel=dialog.getByRole("button",{name:"Cancel",exact:true});
  await expect(cancel).toBeFocused(); await page.keyboard.press("Shift+Tab"); await expect(dialog.getByRole("button",{name:"Delete (syncs to all devices)",exact:true})).toBeFocused();
  await page.keyboard.press("Tab"); await expect(cancel).toBeFocused(); await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0); await expect(page.getByRole("option").first()).toBeFocused();
  expect(await page.evaluate(()=>window.__deleteStages??[])).toEqual([]);
});

test("batch copy preserves list order and explicitly skips images", async ({ page }) => {
  await page.goto("/?screen=main&lang=en"); const rows=page.getByRole("option");
  await rows.nth(4).click(); await rows.nth(0).click({modifiers:["Control"]}); await rows.nth(1).click({modifiers:["Control"]});
  await expect(page.getByText("Copies text in list order, separated by blank lines. Images are skipped.")).toBeVisible();
  await page.getByRole("button",{name:"Copy text",exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>window.__batchCopies)).toEqual([["i1","i2","i5"]]);
  await expect(page.getByRole("status").filter({hasText:"Copied"})).toContainText("Copied 2 text items.");
});

test("pending single deletion stays visible until ten seconds and then commits", async ({ page }) => {
  await page.clock.install(); await page.goto("/?screen=main&lang=en"); const rows=page.getByRole("option");
  await rows.first().click(); await page.clock.pauseAt(new Date(Date.now()+2000));
  await page.getByRole("button",{name:"Delete (syncs to all devices)",exact:true}).click();
  await page.getByRole("dialog").getByRole("button",{name:"Delete (syncs to all devices)",exact:true}).click();
  await expect(page.getByRole("button",{name:"Undo deletion",exact:true})).toBeVisible();
  await page.clock.runFor(9999); expect(await page.evaluate(()=>window.__deleted??[])).toEqual([]); await expect(rows).toHaveCount(6);
  await page.clock.runFor(1); await expect.poll(()=>page.evaluate(()=>window.__deleted)).toEqual(["i1"]);
  await expect(rows).toHaveCount(5); await expect(page.getByRole("button",{name:"Undo deletion",exact:true})).toHaveCount(0);
});

test("local-only deletion labels its scope and can be canceled without scheduling", async ({ page }) => {
  await page.goto("/?screen=main&lang=en&mode=local"); const rows=page.getByRole("option");
  await rows.nth(0).click(); await rows.nth(1).click({modifiers:["Control"]});
  await page.getByRole("button",{name:"Delete selected (this computer)",exact:true}).click();
  const dialog=page.getByRole("dialog"); await expect(dialog).toContainText("Deletion affects this computer.");
  await expect(dialog).not.toContainText("all devices"); await dialog.getByRole("button",{name:"Cancel",exact:true}).click();
  expect(await page.evaluate(()=>window.__deleteStages??[])).toEqual([]);
});

test("favorite and unfavorite apply to every selected item", async ({ page }) => {
  await page.goto("/?screen=main&lang=en"); const rows=page.getByRole("option");
  await rows.nth(0).click(); await rows.nth(2).click({modifiers:["Control"]});
  await page.getByRole("button",{name:"Favorite selected",exact:true}).click();
  await expect(rows.nth(0).getByLabel("Favorite",{exact:true})).toBeVisible();
  await expect(rows.nth(2).getByLabel("Favorite",{exact:true})).toBeVisible();
  await page.getByRole("button",{name:"Unfavorite selected",exact:true}).click();
  await expect(rows.nth(0).getByLabel("Favorite",{exact:true})).toHaveCount(0);
  await expect(rows.nth(2).getByLabel("Favorite",{exact:true})).toHaveCount(0);
});
