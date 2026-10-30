import { useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { HotkeyInfo } from "../api";
import { formatHotkey, fromKeyEvent, isPasteShortcut } from "../hotkeys";
import { Button } from "./ui";

export function HotkeyEditor({ value, onChanged }: { value: HotkeyInfo | null; onChanged: (info: HotkeyInfo) => void }) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const section = useRef<HTMLDivElement>(null);
  const mac = navigator.userAgent.includes("Mac");
  const locked = value?.warning === "cleanup";
  const finish = () => {
    setEditing(false); setPending(null); setError(null);
    requestAnimationFrame(() => section.current?.querySelector<HTMLButtonElement>("[data-change-shortcut]")?.focus());
  };
  const capture = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); finish(); return; }
    if (event.key === "Tab" || event.nativeEvent.isComposing) return;
    event.preventDefault(); event.stopPropagation();
    if (["Control", "Alt", "AltGraph", "Shift", "Meta"].includes(event.key)) return;
    const binding = fromKeyEvent(event.nativeEvent);
    const reserved = binding !== null && isPasteShortcut(binding, mac);
    setPending(reserved ? null : binding); setError(reserved ? "reserved_hotkey" : binding ? null : "invalid_hotkey"); setSaved(false);
  };
  const save = async (binding: string | null) => {
    setSaving(true); setError(null); setSaved(false);
    try {
      onChanged(await api.setHotkey(binding));
      finish(); setSaved(true);
    } catch (failure) {
      const code = String(failure);
      setError(["invalid_hotkey", "reserved_hotkey", "hotkey_busy", "hotkey_storage", "hotkey_cleanup"].includes(code) ? code : "other");
      try { onChanged(await api.hotkey()); } catch { setError("other"); }
    } finally { setSaving(false); }
  };
  const message = error ?? (value?.warning === "fallback" ? "fallback" : value?.warning === "cleanup" ? "hotkey_cleanup" : null);
  const errorKey = message && !value?.current && ["hotkey_busy", "hotkey_storage", "fallback"].includes(message) ? `${message}Unavailable` : message;

  return <div ref={section}>
    <div className="setting">
      <span>{t("settings.showWindow")}</span>
      {value?.current ? <kbd>{formatHotkey(value.current, mac)}</kbd> : <span className="muted">{t(value ? "hotkey.unavailable" : "hotkey.reading")}</span>}
    </div>
    <p className="muted small">{t("settingsMore.hotkeyHint")}</p>
    {!editing ? <div className="setting">
      <Button data-change-shortcut disabled={!value || locked || saving} onClick={() => { setEditing(true); setError(null); setSaved(false); }}>{t("hotkey.change")}</Button>
      <Button disabled={!value || locked || saving} onClick={() => void save(null)}>{t("hotkey.restore")}</Button>
    </div> : <div onKeyDown={(event) => { if (event.key === "Escape" && !saving) { event.preventDefault(); finish(); } }}>
      <label className="field">
        <span>{t("hotkey.new")}</span>
        <input autoFocus readOnly disabled={saving || locked} value={pending ? formatHotkey(pending, mac) : ""} placeholder={t("hotkey.pressKeys")} onKeyDown={capture} />
      </label>
      <p className="muted small">{t("hotkey.captureHint")}</p>
      <div className="setting">
        <Button variant="primary" disabled={!pending || saving || locked} onClick={() => void save(pending)}>{t("hotkey.save")}</Button>
        <Button disabled={saving} onClick={finish}>{t("hotkey.cancel")}</Button>
      </div>
    </div>}
    {message && <p className="notice notice-warn" role="alert">{t(`hotkey.errors.${errorKey}`)}</p>}
    {saved && <p className="muted small" role="status">{t("hotkey.saved")}</p>}
  </div>;
}
