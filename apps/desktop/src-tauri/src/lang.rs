//! UI language: system detection, the user preference and its persistence.
//!
//! The preference lives in `settings.json` as `{"language": "system" | "zh" | "en"}`. A missing,
//! corrupt or unrecognized file always means `system`; nothing here ever fails loudly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// Must match `identifier` in `tauri.conf.json`; the app data directory is named after it.
const APP_ID: &str = "dev.onepaste.spike";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preference {
    System,
    Zh,
    En,
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    language: Preference,
}

/// Maps a raw system locale (such as `zh-Hans-CN`, `zh_TW.UTF-8` or `en-US`) to `"zh"` or `"en"`.
/// Anything that starts with the language code `zh` is Chinese; everything else, including an
/// empty string, is English.
pub fn map_system_locale(raw: &str) -> &'static str {
    let l = raw.trim().to_ascii_lowercase();
    if l == "zh" || l.starts_with("zh-") || l.starts_with("zh_") {
        "zh"
    } else {
        "en"
    }
}

/// The language the UI actually uses for a preference and a (possibly unreadable) system locale.
pub fn effective(pref: Preference, system_locale: Option<&str>) -> &'static str {
    match pref {
        Preference::Zh => "zh",
        Preference::En => "en",
        Preference::System => system_locale.map(map_system_locale).unwrap_or("en"),
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
pub fn save(dir: &Path, pref: Preference) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string(&SettingsFile { language: pref }).map_err(io::Error::other)?;
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

    #[test]
    fn maps_every_chinese_spelling_to_zh() {
        for raw in ["zh", "zh-CN", "zh-Hans-CN", "zh-TW", "zh-HK", "zh-Hant-TW", "ZH-cn", "zh_CN.UTF-8", "  zh-SG "] {
            assert_eq!(map_system_locale(raw), "zh", "{raw}");
        }
    }

    #[test]
    fn maps_everything_else_to_en() {
        for raw in ["en-US", "en", "ja-JP", "ko-KR", "de-DE", "", " ", "zha", "zhx-CN"] {
            assert_eq!(map_system_locale(raw), "en", "{raw}");
        }
    }

    #[test]
    fn effective_language_follows_the_preference() {
        assert_eq!(effective(Preference::Zh, Some("en-US")), "zh");
        assert_eq!(effective(Preference::En, Some("zh-CN")), "en");
        assert_eq!(effective(Preference::System, Some("zh-TW")), "zh");
        assert_eq!(effective(Preference::System, Some("fr-FR")), "en");
        assert_eq!(effective(Preference::System, None), "en");
    }

    #[test]
    fn broken_settings_mean_system() {
        assert_eq!(parse_settings(r#"{"language":"zh"}"#), Preference::Zh);
        assert_eq!(parse_settings(r#"{"language":"en"}"#), Preference::En);
        assert_eq!(parse_settings(r#"{"language":"system"}"#), Preference::System);
        for bad in ["", "not json", "{}", r#"{"language":"klingon"}"#, r#"{"language":5}"#, "[1,2", "\u{0}\u{1}"] {
            assert_eq!(parse_settings(bad), Preference::System, "{bad:?}");
        }
    }

    #[test]
    fn save_then_load_roundtrips_and_survives_garbage() {
        let dir = std::env::temp_dir().join(format!("onepaste-lang-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), Preference::System, "missing file");
        save(&dir, Preference::Zh).unwrap();
        assert_eq!(load(&dir), Preference::Zh);
        save(&dir, Preference::En).unwrap();
        assert_eq!(load(&dir), Preference::En);
        assert!(!dir.join("settings.json.tmp").exists(), "temporary file is renamed away");
        fs::write(dir.join("settings.json"), b"{ broken").unwrap();
        assert_eq!(load(&dir), Preference::System);
        let _ = fs::remove_dir_all(&dir);
    }
}
