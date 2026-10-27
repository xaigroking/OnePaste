import type { ReactNode } from "react";

export type IconName =
  | "clipboard" | "monitor" | "settings" | "search" | "star" | "pin" | "trash" | "copy"
  | "image" | "text" | "link" | "lock" | "check" | "alert" | "wifi-off" | "refresh";

const PATHS: Record<IconName, ReactNode> = {
  clipboard: (<><rect x="6" y="4" width="12" height="17" rx="2" /><path d="M9 4h6v3H9z" /></>),
  monitor: (<><rect x="3" y="5" width="18" height="12" rx="2" /><path d="M8 21h8M12 17v4" /></>),
  settings: (<><circle cx="12" cy="12" r="3" /><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></>),
  search: (<><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.2-4.2" /></>),
  star: (<path d="M12 3.5l2.6 5.4 5.9.8-4.3 4.1 1 5.9-5.2-2.8-5.2 2.8 1-5.9-4.3-4.1 5.9-.8z" />),
  pin: (<><path d="M9 4h6l-1 6 3 3H7l3-3z" /><path d="M12 13v7" /></>),
  trash: (<><path d="M4 7h16M10 7V4h4v3M6 7l1 13h10l1-13" /></>),
  copy: (<><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></>),
  image: (<><rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="9" cy="10" r="1.5" /><path d="M21 16l-5-5-8 8" /></>),
  text: (<path d="M6 6h12M12 6v13M9 19h6" />),
  link: (<><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" /><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></>),
  lock: (<><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></>),
  check: (<path d="M5 12.5l4.5 4.5L19 7.5" />),
  alert: (<><path d="M12 4l9 16H3z" /><path d="M12 10v4M12 17v.5" /></>),
  "wifi-off": (<><path d="M3 3l18 18" /><path d="M5 10a10 10 0 0 1 4-2.4M9 14a5 5 0 0 1 3-1.2M13 8.2A10 10 0 0 1 19 10" /><path d="M12 18v.5" /></>),
  refresh: (<><path d="M20 6v5h-5" /><path d="M20 11a8 8 0 1 0-2 6" /></>),
};

/** Line icons drawn for this project: 1.8 px stroke, no external icon set. */
export function Icon({ name, size = 18, filled = false }: { name: IconName; size?: number; filled?: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
