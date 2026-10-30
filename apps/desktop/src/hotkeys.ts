/** Shortcut names use physical KeyboardEvent codes, matching the native shortcut API. */
export function fromKeyEvent(event: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string | null {
  const modifiers = [event.ctrlKey && "Control", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Super"].filter(Boolean);
  if (!modifiers.length || !event.code || !/^[A-Za-z0-9]+$/.test(event.code) || /^(Escape|Unidentified|Unknown|Control|AltGraph|Alt|Shift|Meta|OS)(Left|Right)?$/.test(event.code)) return null;
  return [...modifiers, event.code].join("+");
}

export function formatHotkey(binding: string, mac = false): string {
  const parts = binding.split("+");
  const code = parts.pop() ?? "";
  const key = code.replace(/^Key/i, "").replace(/^Digit/i, "").replace(/^Numpad/i, "Num ");
  const names: Record<string, string> = mac
    ? { control: "\u2303", ctrl: "\u2303", alt: "\u2325", shift: "\u21e7", super: "\u2318", meta: "\u2318" }
    : { control: "Ctrl", ctrl: "Ctrl", alt: "Alt", shift: "Shift", super: "Win", meta: "Win" };
  return [...parts.map((part) => names[part.toLowerCase()] ?? part), key].join(mac ? "" : "+");
}

export function isPasteShortcut(binding: string, mac = false): boolean {
  return binding === (mac ? "Super+KeyV" : "Control+KeyV");
}
