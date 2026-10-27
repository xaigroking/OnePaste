//! UI language: system detection, the user preference and its persistence.
//!
//! The preference lives in `settings.json` as `{"language": "system" | "<language tag>"}`. A missing,
//! corrupt or unrecognized file always means `system`; nothing here ever fails loudly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// Must match `identifier` in `tauri.conf.json`; the app data directory is named after it.
const APP_ID: &str = "dev.onepaste.spike";
const SETTINGS_FILE: &str = "settings.json";

/// The language choice: follow the system, or one language tag (such as `zh` or `pt-br`) picked by the
/// user. The Rust side does not know which translations exist; the UI maps the tag to the closest
/// translation it has and falls back to English. Adding a language therefore needs no change here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Preference {
    System,
    Code(String),
}

impl Preference {
    /// Reads a stored or requested value. Anything that is not a plausible language tag means `System`.
    pub fn from_raw(raw: &str) -> Preference {
        let tag = normalize_tag(raw);
        match tag {
            Some(t) if t != "system" => Preference::Code(t),
            _ => Preference::System,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Preference::System => "system",
            Preference::Code(c) => c,
        }
    }
}

impl Serialize for Preference {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Preference {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Preference::from_raw(&raw))
    }
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    language: Preference,
}

/// Lowercases a language tag, turns `_` into `-` and drops an encoding or modifier suffix
/// (`zh_CN.UTF-8` becomes `zh-cn`). Returns `None` unless the result looks like a language tag:
/// a 2 to 3 letter language followed by optional subtags of 2 to 8 letters or digits.
fn normalize_tag(raw: &str) -> Option<String> {
    let base = raw.trim().split(['.', '@']).next().unwrap_or("");
    let tag = base.replace('_', "-").to_ascii_lowercase();
    if tag == "system" {
        return Some(tag);
    }
    let mut parts = tag.split('-');
    let lang = parts.next()?;
    if !(2..=3).contains(&lang.len()) || !lang.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let rest_ok = parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()));
    (rest_ok && tag.len() <= 35).then_some(tag)
}

/// Maps a raw system locale (such as `zh-Hans-CN`, `zh_TW.UTF-8` or `en-US`) to a normalized tag
/// (`zh-hans-cn`, `zh-tw`, `en-us`). Anything unreadable, including an empty string, becomes `en`.
pub fn map_system_locale(raw: &str) -> String {
    normalize_tag(raw).filter(|t| t != "system").unwrap_or_else(|| "en".to_string())
}

/// The language tag the UI should try for a preference and a (possibly unreadable) system locale.
pub fn effective(pref: &Preference, system_locale: Option<&str>) -> String {
    match pref {
        Preference::Code(c) => c.clone(),
        Preference::System => system_locale.map(map_system_locale).unwrap_or_else(|| "en".to_string()),
    }
}

pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

pub fn parse_settings(text: &str) -> Preference {
    serde_json::from_str::<SettingsFile>(text).map(|s| s.language).unwrap_or(Preference::System)
}

pub fn load(dir: &Path) -> Preference {
    fs::read_to_string(dir.join(SETTINGS_FILE)).map(|t| parse_settings(&t)).unwrap_or(Preference::System)
}

/// Writes a temporary file first and then renames it over the real one.
pub fn save(dir: &Path, pref: &Preference) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string(&SettingsFile { language: pref.clone() }).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    {
        use io::Write;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, dir.join(SETTINGS_FILE))
}

/// The per-user data directory, the same one the web view uses on Windows. No other location is
/// ever written to.
pub fn settings_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join(APP_ID))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(APP_ID))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = APP_ID;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(c: &str) -> Preference {
        Preference::Code(c.to_string())
    }

    #[test]
    fn system_locales_become_normalized_tags() {
        assert_eq!(map_system_locale("zh_CN.UTF-8"), "zh-cn");
        assert_eq!(map_system_locale("zh-Hans-CN"), "zh-hans-cn");
        assert_eq!(map_system_locale("  ZH-tw "), "zh-tw");
        assert_eq!(map_system_locale("en-US"), "en-us");
        assert_eq!(map_system_locale("pt_BR@euro"), "pt-br");
        assert_eq!(map_system_locale("ja-JP"), "ja-jp");
    }

    #[test]
    fn unreadable_system_locales_mean_english() {
        for raw in ["", " ", "C", "POSIX", "x", "zh--cn", "123", "not a locale", "system"] {
            assert_eq!(map_system_locale(raw), "en", "{raw:?}");
        }
    }

    #[test]
    fn effective_language_follows_the_preference() {
        assert_eq!(effective(&code("zh"), Some("en-US")), "zh");
        assert_eq!(effective(&code("en"), Some("zh-CN")), "en");
        assert_eq!(effective(&code("fr"), Some("zh-CN")), "fr");
        assert_eq!(effective(&Preference::System, Some("zh-TW")), "zh-tw");
        assert_eq!(effective(&Preference::System, Some("fr-FR")), "fr-fr");
        assert_eq!(effective(&Preference::System, None), "en");
    }

    #[test]
    fn stored_values_parse_and_broken_ones_mean_system() {
        assert_eq!(parse_settings(r#"{"language":"zh"}"#), code("zh"));
        assert_eq!(parse_settings(r#"{"language":"en"}"#), code("en"));
        assert_eq!(parse_settings(r#"{"language":"pt_BR"}"#), code("pt-br"));
        assert_eq!(parse_settings(r#"{"language":"system"}"#), Preference::System);
        for bad in ["", "not json", "{}", r#"{"language":"klingon"}"#, r#"{"language":5}"#, r#"{"language":"../x"}"#, "[1,2", "\u{0}\u{1}"] {
            assert_eq!(parse_settings(bad), Preference::System, "{bad:?}");
        }
    }

    #[test]
    fn requested_values_are_validated_too() {
        assert_eq!(serde_json::from_str::<Preference>("\"de\"").unwrap(), code("de"));
        assert_eq!(serde_json::from_str::<Preference>("\"system\"").unwrap(), Preference::System);
        assert_eq!(serde_json::from_str::<Preference>("\"drop table\"").unwrap(), Preference::System);
        assert_eq!(serde_json::to_string(&code("zh")).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&Preference::System).unwrap(), "\"system\"");
    }

    #[test]
    fn save_then_load_roundtrips_and_survives_garbage() {
        let dir = std::env::temp_dir().join(format!("onepaste-lang-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), Preference::System, "missing file");
        save(&dir, &code("zh")).unwrap();
        assert_eq!(load(&dir), code("zh"));
        save(&dir, &code("fr")).unwrap();
        assert_eq!(load(&dir), code("fr"));
        save(&dir, &Preference::System).unwrap();
        assert_eq!(load(&dir), Preference::System);
        assert!(!dir.join("settings.json.tmp").exists(), "temporary file is renamed away");
        fs::write(dir.join("settings.json"), b"{ broken").unwrap();
        assert_eq!(load(&dir), Preference::System);
        let _ = fs::remove_dir_all(&dir);
    }
}
