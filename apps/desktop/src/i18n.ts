import i18next from "i18next";
import { initReactI18next } from "react-i18next";

/**
 * Every `locales/<code>.json` file is a language. Adding a language means adding one file; there is no
 * list to edit. The file name is the language tag in lower case (`zh`, `pt-br`, `zh-tw`), and the
 * `meta` block of the file carries the language's own name and its text direction.
 */
interface LocaleFile {
  meta?: { nativeName?: string; dir?: "ltr" | "rtl" };
  [key: string]: unknown;
}

const files = import.meta.glob<LocaleFile>("./locales/*.json", { eager: true, import: "default" });

export interface LanguageInfo {
  code: string;
  nativeName: string;
  dir: "ltr" | "rtl";
}

const resources: Record<string, { translation: LocaleFile }> = {};
export const LANGUAGES: LanguageInfo[] = [];

for (const [path, content] of Object.entries(files)) {
  const code = path.replace(/^.*\//, "").replace(/\.json$/, "").toLowerCase().replace(/_/g, "-");
  resources[code] = { translation: content };
  LANGUAGES.push({ code, nativeName: content.meta?.nativeName ?? code, dir: content.meta?.dir === "rtl" ? "rtl" : "ltr" });
}
LANGUAGES.sort((a, b) => a.code.localeCompare(b.code));

export const FALLBACK_LANGUAGE = "en";

/**
 * Maps any language tag (`zh-hans-cn`, `pt-br`, `fr`) to the closest translation we have: the full tag,
 * then shorter and shorter prefixes, then English.
 */
export function resolveLanguage(tag: string | null | undefined): string {
  const parts = (tag ?? "").toLowerCase().replace(/_/g, "-").split("-").filter(Boolean);
  for (let n = parts.length; n > 0; n--) {
    const candidate = parts.slice(0, n).join("-");
    if (candidate in resources) return candidate;
  }
  return FALLBACK_LANGUAGE;
}

export function languageInfo(code: string): LanguageInfo | undefined {
  return LANGUAGES.find((l) => l.code === code);
}

/** Keeps the document's language and direction in step with the UI language. */
export function applyDocumentLanguage(code: string): void {
  document.documentElement.lang = code;
  document.documentElement.dir = languageInfo(code)?.dir ?? "ltr";
}

/** Starts i18next in the language the Rust side decided on (system locale or saved preference). */
export function initI18n(tag: string) {
  const lng = resolveLanguage(tag);
  applyDocumentLanguage(lng);
  return i18next.use(initReactI18next).init({
    resources,
    lng,
    fallbackLng: FALLBACK_LANGUAGE,
    lowerCaseLng: true,
    interpolation: { escapeValue: false },
  });
}

export default i18next;
