import { describe, expect, it } from "vitest";
import { formatHotkey, fromKeyEvent, isPasteShortcut } from "./hotkeys";

const event = (code: string, ctrlKey = false, altKey = false, shiftKey = false, metaKey = false) => ({ code, ctrlKey, altKey, shiftKey, metaKey });

describe("shortcut input", () => {
  it("requires a modifier and a non-modifier key", () => {
    for (const value of [event("KeyK"), event("ControlLeft", true), event("Escape", true), event("Unidentified", true)]) expect(fromKeyEvent(value)).toBeNull();
    expect(fromKeyEvent(event("KeyK", true, true))).toBe("Control+Alt+KeyK");
    expect(fromKeyEvent(event("Digit1", false, false, true))).toBe("Shift+Digit1");
    expect(fromKeyEvent(event("ArrowUp", false, false, false, true))).toBe("Super+ArrowUp");
  });
  it("reserves the platform clipboard and quick-panel paste combinations", () => {
    expect(isPasteShortcut("Control+KeyV")).toBe(true);
    expect(isPasteShortcut("Super+KeyV", true)).toBe(true);
    expect(isPasteShortcut("Control+Enter")).toBe(true);
    expect(isPasteShortcut("Super+Enter", true)).toBe(true);
    expect(isPasteShortcut("Control+NumpadEnter")).toBe(true);
    expect(isPasteShortcut("Control+Enter", true)).toBe(false);
    expect(isPasteShortcut("Control+Alt+KeyV")).toBe(false);
    expect(isPasteShortcut("Control+KeyV", true)).toBe(false);
    expect(isPasteShortcut("Super+KeyV")).toBe(false);
  });
  it("formats the active key rather than assuming the default", () => {
    expect(formatHotkey("Control+Shift+KeyK")).toBe("Ctrl+Shift+K");
    expect(formatHotkey("Super+Digit1")).toBe("Win+1");
    expect(formatHotkey("Control+Alt+KeyK", true)).toBe("\u2303\u2325K");
    expect(formatHotkey("Super+KeyK", true)).toBe("\u2318K");
  });
});
