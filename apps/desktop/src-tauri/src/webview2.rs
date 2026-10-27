//! Windows only: detect a missing WebView2 runtime at startup, show a system message box with a
//! download link, and exit with code 2. The message comes from the UI translation files, which are
//! embedded at build time, so this source contains no translated text.

const EN: &str = include_str!("../../src/locales/en.json");
const ZH: &str = include_str!("../../src/locales/zh.json");
const DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
/// Used only if neither translation file has the message, which the tests rule out.
const FALLBACK: &str = "The Microsoft Edge WebView2 runtime is missing.";

fn message_in(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("webview2")?.get("missing")?.as_str().map(str::to_string)
}

/// The full message box text for a language tag; Chinese tags (`zh`, `zh-tw`, ...) get the Chinese text,
/// every other tag falls back to English.
pub fn missing_message(language: &str) -> String {
    let is_zh = language == "zh" || language.starts_with("zh-");
    let text = if is_zh { message_in(ZH) } else { None }
        .or_else(|| message_in(EN))
        .unwrap_or_else(|| FALLBACK.to_string());
    format!("{text}\n\n{DOWNLOAD_URL}")
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::lang;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    const CLIENT_ID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads the runtime version (`pv`) from one of the registry locations the runtime installer
    /// uses. Only reads the registry; nothing is ever written.
    fn version_at(root: HKEY, subkey: &str) -> Option<String> {
        let key = wide(&format!("{subkey}\\{CLIENT_ID}"));
        let name = wide("pv");
        let mut buf = [0u16; 64];
        let mut size = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                root,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn runtime_present() -> bool {
        let candidates = [
            (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients"),
            (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients"),
            (HKEY_CURRENT_USER, "Software\\Microsoft\\EdgeUpdate\\Clients"),
        ];
        candidates
            .iter()
            .filter_map(|(root, sub)| version_at(*root, sub))
            .any(|v| !v.is_empty() && v != "0.0.0.0")
    }

    pub fn ensure_or_exit() {
        if runtime_present() {
            return;
        }
        let pref = lang::settings_dir().map(|d| lang::load(&d)).unwrap_or(lang::Preference::System);
        let language = lang::effective(&pref, lang::system_locale().as_deref());
        let text = wide(&missing_message(&language));
        let caption = wide("OnePaste");
        unsafe {
            MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_OK | MB_ICONERROR);
        }
        std::process::exit(2);
    }
}

#[cfg(windows)]
pub use imp::ensure_or_exit;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_translation_files_carry_the_message() {
        assert!(message_in(EN).is_some());
        assert!(message_in(ZH).is_some());
        assert_ne!(message_in(EN), message_in(ZH));
    }

    #[test]
    fn message_ends_with_the_download_link() {
        for language in ["en", "zh", "zh-tw", "fr"] {
            assert!(missing_message(language).ends_with(DOWNLOAD_URL), "{language}");
        }
        assert_eq!(missing_message("fr"), missing_message("en"));
    }
}
