import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vitest";
import en from "./locales/en.json";
import zh from "./locales/zh.json";
import { ChooseMode, KitScreen, passphraseStrength } from "./onboarding";

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    resources: { en: { translation: en }, zh: { translation: zh } },
    lng: "en",
    fallbackLng: "en",
    interpolation: { escapeValue: false },
  });
});

const noop = () => undefined;

/** The opening tag of the first element of a kind whose markup contains `marker`. */
function tagOf(html: string, element: string, marker: string): string {
  const re = new RegExp(`<${element}\\b[^>]*>`, "g");
  const found = (html.match(re) ?? []).find((tag) => tag.includes(marker));
  if (!found) throw new Error(`no <${element}> with ${marker} in: ${html}`);
  return found;
}

describe("first step", () => {
  it("shows all three ways to use the program at once, and the hosted one cannot be chosen", () => {
    const html = renderToStaticMarkup(<ChooseMode choice={null} onChoice={noop} onNext={noop} />);
    for (const text of [en.onboarding.mode.local, en.onboarding.mode.server, en.onboarding.mode.hosted, en.onboarding.mode.hostedHint]) {
      expect(html).toContain(text);
    }
    expect(tagOf(html, "input", 'value="hosted"')).toContain("disabled");
    expect(tagOf(html, "input", 'value="local"')).not.toContain("disabled");
    expect(tagOf(html, "input", 'value="server"')).not.toContain("disabled");
  });

  it("keeps Next unavailable until a way is chosen", () => {
    const none = renderToStaticMarkup(<ChooseMode choice={null} onChoice={noop} onNext={noop} />);
    expect(tagOf(none, "button", 'type="button"')).toContain("disabled");
    const chosen = renderToStaticMarkup(<ChooseMode choice="local" onChoice={noop} onNext={noop} />);
    expect(tagOf(chosen, "button", 'type="button"')).not.toContain("disabled");
  });
});

describe("recovery kit screen", () => {
  const kit = "AAAA-BBBB-CCCC-DDDD";
  const screen = (saved: boolean) =>
    renderToStaticMarkup(
      <KitScreen kit={kit} saved={saved} onSavedChange={noop} onDone={noop} onCopy={noop} onSave={noop} onPrint={noop} busy={false} error={null} />,
    );

  it("keeps Done disabled until the kit is confirmed as saved", () => {
    expect(tagOf(screen(false), "button", 'type="button"')).toBeDefined();
    const doneBefore = (screen(false).match(/<button\b[^>]*>[^<]*Done<\/button>/) ?? [""])[0];
    expect(doneBefore).toContain("disabled");
    const doneAfter = (screen(true).match(/<button\b[^>]*>[^<]*Done<\/button>/) ?? [""])[0];
    expect(doneAfter).toContain("Done");
    expect(doneAfter).not.toContain("disabled");
  });

  it("shows the kit in a monospaced block, with copy, save and print, and the warning", () => {
    const html = screen(false);
    expect(html).toContain(kit);
    expect(html).toContain('class="kit"');
    for (const text of [en.onboarding.kit.copy, en.onboarding.kit.save, en.onboarding.kit.print, en.onboarding.kit.warning, en.onboarding.kit.saved]) {
      expect(html).toContain(text);
    }
  });

  it("has the same warning in Chinese", () => {
    expect(zh.onboarding.kit.warning).toBe("\u8fd9\u4e32\u5b57\u7b26\u5c31\u662f\u4f60\u7684\u94a5\u5319\u3002\u5fd8\u8bb0\u53e3\u4ee4\u53c8\u4e22\u5931\u5b83\uff0c\u6570\u636e\u65e0\u6cd5\u6062\u590d\u3002");
  });
});

describe("passphrase strength", () => {
  it("grows with length and variety", () => {
    expect(passphraseStrength("short")).toBe("weak");
    expect(passphraseStrength("onlylowercase")).toBe("fair");
    expect(passphraseStrength("Mixed-Case-123")).toBe("strong");
    expect(passphraseStrength("7K3M-QX9D-ABCD-EFGH-JKLM")).toBe("strong");
  });
});

describe("wording", () => {
  const internal = /\b(MK|KDF|vault_proof|seq|master key|argon|sqlcipher)\b/i;
  const texts = (o: unknown): string[] => (typeof o === "string" ? [o] : Object.values(o as object).flatMap(texts));
  it("uses no internal terms on the first-run and security screens", () => {
    for (const lang of [en, zh]) {
      for (const text of [...texts(lang.onboarding), ...texts(lang.security)]) {
        expect(text, text).not.toMatch(internal);
      }
    }
  });
});
