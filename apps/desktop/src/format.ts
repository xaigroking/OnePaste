export function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

export function formatClock(tsMs: number): string {
  return new Date(tsMs).toLocaleTimeString([], { hour12: false });
}

/** A short relative time such as "5 min ago", using the UI language. */
export function formatRelative(tsMs: number, lang: string): string {
  const diff = Date.now() - tsMs;
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const min = Math.round(diff / 60000);
  if (min < 1) return rtf.format(0, "second");
  if (min < 60) return rtf.format(-min, "minute");
  const hours = Math.round(min / 60);
  if (hours < 24) return rtf.format(-hours, "hour");
  return rtf.format(-Math.round(hours / 24), "day");
}

export function isLink(text: string): boolean {
  return /^https?:\/\/\S+$/i.test(text.trim());
}

/** The theme choice is a per-viewer convenience, so storage failures are ignored. */
export type ThemeChoice = "system" | "light" | "dark";

export function loadTheme(): ThemeChoice {
  try {
    const v = localStorage.getItem("onepaste.theme");
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
  try {
    localStorage.setItem("onepaste.theme", choice);
  } catch {
    /* storage can be unavailable; the choice then lasts for this session only */
  }
}
