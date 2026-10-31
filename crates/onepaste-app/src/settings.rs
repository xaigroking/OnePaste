//! Application settings live next to the databases, in the same data directory.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE_NAME: &str = "app-settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Write the newest item that arrives from another device into this computer's clipboard.
    pub auto_apply_remote: bool,
    /// Record what is copied on this computer.
    pub record_clipboard: bool,
    /// Maximum JSON-encoded item content plus its envelope budget, in mebibytes.
    #[serde(rename = "maxItemMiB")]
    pub max_item_mib: u8,
    /// A validated native shortcut; None selects the default.
    pub quick_hotkey: Option<String>,
    /// Skip newly copied text with private-key markers or GitHub token prefixes.
    pub skip_suspected_secrets: bool,
    /// Look for a newer version when the program starts and every six hours.
    pub check_updates: bool,
    /// The update channel; `None` means the default for this version (see `update::default_channel`).
    pub update_channel: Option<crate::update::Channel>,
    /// The version the user chose to skip; it is not offered again, a newer one is.
    pub skipped_update: Option<String>,
    /// The user dismissed the note that closing the window keeps the program running in the tray.
    pub tray_notice_dismissed: bool,
    /// The height in pixels of the text preview in the detail pane, when the user has dragged it.
    pub text_preview_height: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_apply_remote: true, record_clipboard: true, max_item_mib: 8, quick_hotkey: None, skip_suspected_secrets: true, check_updates: true, update_channel: None, skipped_update: None, tray_notice_dismissed: false, text_preview_height: None }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        let mut settings: Settings = std::fs::read_to_string(dir.join(FILE_NAME)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if !valid_item_limit(settings.max_item_mib) { settings.max_item_mib = 8; }
        settings
    }

    pub fn save(&self, dir: &Path) -> Result<(), AppError> {
        if !valid_item_limit(self.max_item_mib) { return Err(AppError::Other("invalid item size limit".into())); }
        write_private(dir, FILE_NAME, serde_json::to_string(self).unwrap_or_default().as_bytes())
    }
}

fn valid_item_limit(mib: u8) -> bool { matches!(mib, 1 | 2 | 4 | 8) }

/// Both text and Base64 image content pass through the same JSON string encoding.
pub(crate) fn item_within_limit(content: &str, mib: u8) -> bool {
    let budget = usize::from(mib) * 1024 * 1024;
    if content.len().saturating_add(1024) > budget { return false; }
    // Quotes are not part of the content. Base64 has no escapes, so its old size calculation stays the same.
    serde_json::to_vec(content).map(|bytes| bytes.len().saturating_sub(2).saturating_add(1024) <= budget).unwrap_or(false)
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn encoded_content_accepts_equality_and_rejects_one_more_byte_at_every_supported_limit() {
        for mib in [1, 2, 4, 8] {
            let mut encoded = "A".repeat(usize::from(mib) * 1024 * 1024 - 1024);
            assert!(item_within_limit(&encoded, mib));
            encoded.push('A');
            assert!(!item_within_limit(&encoded, mib));
        }
    }

    #[test]
    fn old_settings_get_the_default_limit_and_new_settings_round_trip() {
        let old: Settings = serde_json::from_str(r#"{"recordClipboard":false}"#).unwrap();
        assert!(!old.record_clipboard);
        assert_eq!(old.max_item_mib, 8);
        for mib in [1, 2, 4, 8] {
            let value = Settings { max_item_mib: mib, ..old.clone() };
            assert_eq!(serde_json::from_slice::<Settings>(&serde_json::to_vec(&value).unwrap()).unwrap(), value);
        }
    }
}

/// Temporary file, `fsync`, rename; mode 0600 on Unix.
pub(crate) fn write_private(dir: &Path, name: &str, data: &[u8]) -> Result<(), AppError> {
    use std::io::Write;
    let io = |e: std::io::Error| AppError::Storage(e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = dir.join(format!("{name}.tmp"));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(io)?;
        f.write_all(data).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, dir.join(name)).map_err(io)
}

pub const STATE_FILE_NAME: &str = "app-state.json";

/// What the application remembers between runs besides the settings. It holds no secret.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct StateFile {
    /// The first sync after joining has finished.
    pub initial_sync_done: bool,
    /// The user chose to keep the history on this computer only.
    pub local_only: bool,
}

#[cfg(test)]
mod quick_hotkey_settings_tests {
    use super::*;

    #[test]
    fn saved_quick_hotkey_is_backward_compatible_and_round_trips() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        let mut value = serde_json::to_value(old).unwrap();
        assert!(value.as_object().unwrap().contains_key("quickHotkey"));
        assert!(value["quickHotkey"].is_null());
        value["quickHotkey"] = "Control+Shift+KeyK".into();
        let saved: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(serde_json::to_value(saved).unwrap()["quickHotkey"], "Control+Shift+KeyK");
    }
}

impl StateFile {
    pub fn load(dir: &Path) -> StateFile {
        std::fs::read_to_string(dir.join(STATE_FILE_NAME)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), AppError> {
        write_private(dir, STATE_FILE_NAME, serde_json::to_string(self).unwrap_or_default().as_bytes())
    }
}

/// Portable shortcut validation and registration transitions. The shell supplies the operating-system adapter.
pub mod hotkeys {
    pub const DEFAULT_BINDING: &str = "Control+Alt+KeyV";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Error { Invalid, Reserved, Busy, Storage, Cleanup }

    impl Error {
        pub fn code(self) -> &'static str {
            match self { Self::Invalid => "invalid_hotkey", Self::Reserved => "reserved_hotkey", Self::Busy => "hotkey_busy", Self::Storage => "hotkey_storage", Self::Cleanup => "hotkey_cleanup" }
        }
    }

    /// Checks syntax and modifier requirements. The adapter also validates the operating system's key codes.
    pub fn normalize(raw: &str) -> Result<String, Error> {
        if raw.len() > 128 { return Err(Error::Invalid); }
        let parts: Vec<_> = raw.split('+').map(str::trim).collect();
        if !(2..=5).contains(&parts.len()) { return Err(Error::Invalid); }
        let mut modifiers = [false; 4];
        for part in &parts[..parts.len() - 1] {
            let index = match part.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => 0,
                "alt" | "option" => 1,
                "shift" => 2,
                "super" | "meta" | "command" | "cmd" | "win" => 3,
                _ => return Err(Error::Invalid),
            };
            if modifiers[index] { return Err(Error::Invalid); }
            modifiers[index] = true;
        }
        let code = parts.last().copied().unwrap_or_default();
        if code.is_empty() || !code.bytes().all(|b| b.is_ascii_alphanumeric()) { return Err(Error::Invalid); }
        let lower = code.to_ascii_lowercase();
        if ["escape", "unidentified", "unknown", "control", "ctrl", "alt", "option", "shift", "meta", "super", "command", "cmd", "win", "altgraph",
            "controlleft", "controlright", "altleft", "altright", "shiftleft", "shiftright", "metaleft", "metaright", "osleft", "osright"].contains(&lower.as_str()) { return Err(Error::Invalid); }
        let code = if code.len() == 1 && code.as_bytes()[0].is_ascii_alphabetic() {
            format!("Key{}", code.to_ascii_uppercase())
        } else if code.len() == 1 && code.as_bytes()[0].is_ascii_digit() {
            format!("Digit{code}")
        } else { code.to_string() };
        let paste_modifiers = if cfg!(target_os = "macos") { [false, false, false, true] } else { [true, false, false, false] };
        if ["keyv", "enter", "return", "numpadenter"].contains(&code.to_ascii_lowercase().as_str()) && modifiers == paste_modifiers { return Err(Error::Reserved); }
        let names = ["Control", "Alt", "Shift", "Super"];
        let mut output: Vec<_> = names.iter().enumerate().filter(|(i, _)| modifiers[*i]).map(|(_, name)| (*name).to_string()).collect();
        output.push(code);
        Ok(output.join("+"))
    }

    pub trait Port {
        fn register(&mut self, binding: &str) -> Result<(), Error>;
        fn unregister(&mut self, binding: &str) -> Result<(), Error>;
        fn save(&mut self, binding: Option<&str>) -> Result<(), Error>;
    }

    #[derive(Debug, Default)]
    pub struct Controller {
        active: Option<String>,
        fallback: bool,
        needs_restart: bool,
    }

    impl Controller {
        pub fn start(saved: Option<&str>, port: &mut impl Port) -> Self {
            let requested = saved.map(normalize).transpose();
            let mut state = Self { fallback: requested.is_err(), ..Self::default() };
            let requested = requested.ok().flatten().unwrap_or_else(|| DEFAULT_BINDING.to_string());
            if port.register(&requested).is_ok() {
                state.active = Some(requested);
            } else {
                state.fallback = true;
                if requested != DEFAULT_BINDING && port.register(DEFAULT_BINDING).is_ok() { state.active = Some(DEFAULT_BINDING.to_string()); }
            }
            state
        }

        pub fn active(&self) -> Option<&str> { self.active.as_deref() }

        pub fn warning(&self) -> Option<&'static str> {
            if self.needs_restart { Some("cleanup") } else if self.fallback { Some("fallback") } else { None }
        }

        pub fn configure(&mut self, requested: Option<&str>, port: &mut impl Port) -> Result<(), Error> {
            if self.needs_restart { return Err(Error::Cleanup); }
            let next = normalize(requested.unwrap_or(DEFAULT_BINDING))?;
            let saved = requested.map(|_| next.as_str());
            if self.active() == Some(next.as_str()) {
                port.save(saved).map_err(|_| Error::Storage)?;
                self.fallback = false;
                return Ok(());
            }
            // Keep the previous shortcut registered until both registration and persistence have succeeded.
            port.register(&next)?;
            if port.save(saved).is_err() {
                if port.unregister(&next).is_err() { self.needs_restart = true; return Err(Error::Cleanup); }
                return Err(Error::Storage);
            }
            let old = self.active.replace(next);
            self.fallback = false;
            if let Some(old) = old {
                if port.unregister(&old).is_err() { self.needs_restart = true; return Err(Error::Cleanup); }
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::HashSet;

        const CUSTOM: &str = "Control+Shift+KeyK";

        #[derive(Default)]
        struct Fake {
            registered: HashSet<String>,
            blocked: HashSet<String>,
            saved: Option<String>,
            fail_save: bool,
            fail_unregister: bool,
            calls: Vec<String>,
        }

        impl Port for Fake {
            fn register(&mut self, value: &str) -> Result<(), Error> {
                self.calls.push(format!("register:{value}"));
                if self.blocked.contains(value) { return Err(Error::Busy); }
                self.registered.insert(value.to_string()); Ok(())
            }
            fn unregister(&mut self, value: &str) -> Result<(), Error> {
                self.calls.push(format!("unregister:{value}"));
                if self.fail_unregister { return Err(Error::Cleanup); }
                self.registered.remove(value); Ok(())
            }
            fn save(&mut self, value: Option<&str>) -> Result<(), Error> {
                self.calls.push(format!("save:{}", value.unwrap_or("default")));
                if self.fail_save { return Err(Error::Storage); }
                self.saved = value.map(str::to_string); Ok(())
            }
        }

        #[test]
        fn syntax_requires_a_modifier_and_a_non_modifier_key_and_normalizes_order() {
            for (input, expected) in [("Alt+Ctrl+KeyK", "Control+Alt+KeyK"), ("Shift+KeyA", "Shift+KeyA"), ("Cmd+K", "Super+KeyK"), ("Control+1", "Control+Digit1"), ("Control+ArrowUp", "Control+ArrowUp"), ("Alt+F24", "Alt+F24")] {
                assert_eq!(normalize(input).unwrap(), expected);
            }
            for input in ["", "KeyK", "Control", "Control+ShiftLeft", "Control+Escape", "Control++KeyK", "Control+Control+KeyK", "Control+KeyK+KeyL", "Control+Unidentified", "Control+bad/key"] { assert_eq!(normalize(input), Err(Error::Invalid)); }
        }

        #[test]
        fn the_platform_paste_shortcut_is_reserved_without_touching_registration_or_preferences() {
            let modifier = if cfg!(target_os = "macos") { "Command" } else { "Ctrl" };
            let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.calls.clear();
            for key in ["V", "Enter", "Return", "NumpadEnter"] { assert_eq!(state.configure(Some(&format!("{modifier}+{key}")), &mut io), Err(Error::Reserved)); }
            assert!(io.calls.is_empty()); assert_eq!(state.active(), Some(DEFAULT_BINDING));
            assert_eq!(normalize(DEFAULT_BINDING).unwrap(), DEFAULT_BINDING);
        }

        #[test]
        fn successful_change_registers_then_persists_then_releases_the_old_shortcut() {
            let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.calls.clear();
            state.configure(Some(CUSTOM), &mut io).unwrap();
            assert_eq!(io.calls.join("|"), format!("register:{CUSTOM}|save:{CUSTOM}|unregister:{DEFAULT_BINDING}"));
            assert_eq!(state.active(), Some(CUSTOM)); assert_eq!(io.saved.as_deref(), Some(CUSTOM));
            assert_eq!(io.registered.len(), 1); assert!(io.registered.contains(CUSTOM));
        }

        #[test]
        fn an_occupied_candidate_leaves_the_old_registration_and_saved_choice_untouched() {
            let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.calls.clear(); io.blocked.insert(CUSTOM.into());
            assert_eq!(state.configure(Some(CUSTOM), &mut io), Err(Error::Busy));
            assert_eq!(io.calls, vec![format!("register:{CUSTOM}")]);
            assert_eq!(state.active(), Some(DEFAULT_BINDING)); assert!(io.registered.contains(DEFAULT_BINDING)); assert_eq!(io.saved, None);
        }

        #[test]
        fn saving_failure_removes_only_the_candidate_and_keeps_the_old_shortcut_working() {
            let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.calls.clear(); io.fail_save = true;
            assert_eq!(state.configure(Some(CUSTOM), &mut io), Err(Error::Storage));
            assert_eq!(io.calls.join("|"), format!("register:{CUSTOM}|save:{CUSTOM}|unregister:{CUSTOM}"));
            assert_eq!(state.active(), Some(DEFAULT_BINDING)); assert_eq!(io.saved, None);
            assert_eq!(io.registered.len(), 1); assert!(io.registered.contains(DEFAULT_BINDING));
        }

        #[test]
        fn startup_tries_the_saved_binding_and_falls_back_without_overwriting_it() {
            let mut io = Fake { saved: Some(CUSTOM.into()), ..Fake::default() }; io.blocked.insert(CUSTOM.into());
            let state = Controller::start(Some(CUSTOM), &mut io);
            assert_eq!(state.active(), Some(DEFAULT_BINDING)); assert_eq!(state.warning(), Some("fallback")); assert_eq!(io.saved.as_deref(), Some(CUSTOM));
            io.blocked.insert(DEFAULT_BINDING.into()); io.registered.clear();
            let unavailable = Controller::start(Some(CUSTOM), &mut io);
            assert_eq!(unavailable.active(), None); assert_eq!(unavailable.warning(), Some("fallback"));
        }

        #[test]
        fn restoring_default_clears_the_saved_custom_binding() {
            let mut io = Fake { saved: Some(CUSTOM.into()), ..Fake::default() }; let mut state = Controller::start(Some(CUSTOM), &mut io); io.calls.clear();
            state.configure(None, &mut io).unwrap();
            assert_eq!(state.active(), Some(DEFAULT_BINDING)); assert_eq!(io.saved, None); assert_eq!(io.registered.len(), 1);
            assert_eq!(io.calls.join("|"), format!("register:{DEFAULT_BINDING}|save:default|unregister:{CUSTOM}"));
        }

        #[test]
        fn cleanup_failures_preserve_a_working_saved_shortcut_and_require_restart() {
            for fail_save in [false, true] {
                let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.fail_unregister = true; io.fail_save = fail_save;
                assert_eq!(state.configure(Some(CUSTOM), &mut io), Err(Error::Cleanup));
                let active = if fail_save { DEFAULT_BINDING } else { CUSTOM };
                assert_eq!(state.active(), Some(active)); assert!(io.registered.contains(active));
                assert_eq!(state.warning(), Some("cleanup"));
                assert_eq!(io.saved.as_deref(), if fail_save { None } else { Some(CUSTOM) });
                let before = io.calls.clone(); assert_eq!(state.configure(None, &mut io), Err(Error::Cleanup)); assert_eq!(io.calls, before);
            }
        }

        #[test]
        fn invalid_candidates_never_reach_the_registry_and_same_binding_needs_no_reregistration() {
            let mut io = Fake::default(); let mut state = Controller::start(None, &mut io); io.calls.clear();
            assert_eq!(state.configure(Some("KeyK"), &mut io), Err(Error::Invalid)); assert!(io.calls.is_empty());
            state.configure(None, &mut io).unwrap(); assert_eq!(io.calls, vec!["save:default"]);
        }
    }
}
