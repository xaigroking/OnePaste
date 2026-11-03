import { test, expect } from "@playwright/test";

for (const theme of ["light", "dark"]) for (const lang of ["en", "zh"]) for (const screen of ["main", "set", "qp"]) test(`actual colors and keyboard contrast ${theme} ${lang} ${screen}`, async ({ page }) => {
  await page.setViewportSize({ width: screen === "qp" ? 560 : 1040, height: 720 });
  await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
  await page.goto(`/?screen=${screen}&lang=${lang}${screen === "qp" ? "#/quick" : ""}`);
  await expect(page.locator("button").first()).toBeVisible();
  const result = await page.evaluate(() => {
    const rgb = (s: string) => (s.match(/[\d.]+/g) ?? []).map(Number);
    const light = (a: number[]) => a.slice(0,3).map((v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }).reduce((sum,v,i) => sum + v * [0.2126,0.7152,0.0722][i], 0);
    const ratio = (a: string, b: string) => { const x = light(rgb(a)), y = light(rgb(b)); return (Math.max(x,y)+0.05)/(Math.min(x,y)+0.05); };
    const background = (element: Element | null): string => {
      for (let node = element; node; node = node.parentElement) { const value = getComputedStyle(node).backgroundColor; if ((rgb(value)[3] ?? 1) > 0.99) return value; }
      return "rgb(255,255,255)";
    };
    const failures: string[] = [];
    let texts = 0, controls = 0;
    for (const el of document.querySelectorAll<HTMLElement>("button,input,textarea,select,.muted,.badge,.row-title,.row-meta,.strength")) {
      if (!el.getBoundingClientRect().width || el.closest(":disabled,[aria-disabled='true'],.unavailable")) continue;
      const style = getComputedStyle(el);
      if (Number(style.opacity) < 1 || style.visibility === "hidden" || el.matches("input[type=checkbox],input[type=radio]")) continue;
      if (ratio(style.color, background(el)) < 4.5) failures.push(`text ${el.tagName}.${el.className}: ${style.color} / ${background(el)}`);
      texts++;
      if (el.matches("button,input,textarea,select") && parseFloat(style.borderTopWidth) > 0 && Math.max(ratio(style.borderTopColor, background(el)), ratio(style.borderTopColor, background(el.parentElement))) < 3) failures.push(`control ${el.tagName}.${el.className}: ${style.borderTopColor} / ${background(el)}`);
      if (el.matches("button,input,textarea,select") && parseFloat(style.borderTopWidth) > 0) controls++;
      if (el.matches("input[placeholder],textarea[placeholder]")) {
        const placeholder = getComputedStyle(el, "::placeholder");
        if (Number(placeholder.opacity) !== 1 || ratio(placeholder.color,background(el)) < 4.5) failures.push(`placeholder ${placeholder.color} opacity ${placeholder.opacity}`);
      }
    }
    return { failures, texts, controls, accent: getComputedStyle(document.documentElement).getPropertyValue("--accent").trim(), bg: getComputedStyle(document.body).backgroundColor };
  });
  expect(result.texts).toBeGreaterThan(3);
  expect(result.controls).toBeGreaterThan(0);
  expect(result.failures).toEqual([]);
  expect(result.accent).toBe(theme === "dark" ? "#a8c4ff" : "#1d4ed8");
  expect(result.bg).toBe(theme === "dark" ? "rgb(15, 20, 32)" : "rgb(244, 246, 250)");
  const target = page.locator("button:enabled").first();
  await target.focus();
  await expect(target).toHaveCSS("outline-style", "solid");
  await expect(target).toHaveCSS("outline-width", "2px");
  if (screen === "main" || screen === "qp") {
    await page.getByRole("searchbox").focus();
    const search = page.locator(screen === "main" ? ".searchbar" : ".qp-search");
    await expect(search).toHaveCSS("outline-style", "solid");
    await expect(search).toHaveCSS("outline-width", "2px");
  }
});

for (const system of ["light", "dark"] as const) test(`system preference uses the same ${system} palette`, async ({ page }) => {
  await page.emulateMedia({ colorScheme: system });
  await page.addInitScript(() => localStorage.removeItem("onepaste.theme"));
  await page.goto("/?lang=en");
  await expect(page.locator("body")).toHaveCSS("background-color", system === "dark" ? "rgb(15, 20, 32)" : "rgb(244, 246, 250)");
});

for (const theme of ["light", "dark"]) test(`switch states have distinguishable tracks and thumbs ${theme}`, async ({ page }) => {
  await page.addInitScript((value) => localStorage.setItem("onepaste.theme", value), theme);
  await page.goto("/?screen=set&lang=en");
  const input = page.getByRole("switch", { name: "Record what I copy on this computer", exact: true });
  const track = input.locator("+ .switch-track");
  await expect(track).toBeVisible();
  await input.focus();
  await expect(track).toHaveCSS("outline-style", "solid");
  const thumb = await track.evaluate((el) => getComputedStyle(el,"::after").backgroundColor);
  expect(thumb).toBe(theme === "dark" ? "rgb(16, 27, 50)" : "rgb(255, 255, 255)");
  await input.uncheck({ force: true });
  await expect(track).toHaveCSS("background-color", theme === "dark" ? "rgb(129, 144, 170)" : "rgb(119, 131, 153)");
});
