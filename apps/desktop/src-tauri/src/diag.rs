//! Diagnostics: version, system, tray state and event records only. Never any clipboard content or keys.

use crate::state::{Inner, LogEntry};

/// Formats a millisecond timestamp as UTC `YYYY-MM-DD HH:MM:SS`.
pub fn format_utc(ts_ms: u64) -> String {
    let secs = ts_ms / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Gregorian date conversion (Howard Hinnant's civil_from_days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn line(e: &LogEntry) -> String {
    match &e.detail {
        Some(d) => format!("{} {} {}", format_utc(e.ts_ms), e.kind, d),
        None => format!("{} {}", format_utc(e.ts_ms), e.kind),
    }
}

pub fn build(state: &Inner, os_version: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("app_version: {}\n", env!("CARGO_PKG_VERSION")));
    // Injected by CI on release builds to tell apart pre-releases that share a version number
    out.push_str(&format!("build_tag: {}\n", option_env!("ONEPASTE_BUILD_TAG").unwrap_or("none")));
    out.push_str(&format!("os: {os_version}\n"));
    out.push_str(&format!("arch: {}\n", std::env::consts::ARCH));
    out.push_str(&format!("system_locale: {}\n", state.system_locale.as_deref().unwrap_or("unknown")));
    out.push_str(&format!("ui_language: {}\n", crate::lang::effective(&state.language, state.system_locale.as_deref())));
    out.push_str(&format!("tray_state: {}\n", state.tray.key()));
    out.push_str("recent_events (utc):\n");
    let skip = state.log.len().saturating_sub(20);
    for e in state.log.iter().skip(skip) {
        out.push_str(&line(e));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_000_000_000_000), "2001-09-09 01:46:40");
        assert_eq!(format_utc(1_782_000_123_000), "2026-06-21 00:02:03");
    }

    #[test]
    fn diagnostics_keep_last_20_events_and_no_content() {
        let AppState(m) = AppState::default();
        let mut s = m.into_inner().unwrap();
        for i in 0..30 {
            s.push_log("wake", Some(format!("n{i}")));
        }
        let text = build(&s, "TestOS 1.0");
        assert_eq!(text.lines().filter(|l| l.contains(" wake ")).count(), 20);
        assert!(text.contains("n29") && !text.contains("n9\n") && !text.contains("n0\n"));
        assert!(text.contains("os: TestOS 1.0") && text.contains("tray_state: synced"));
        assert!(text.contains("build_tag: "));
        assert!(text.contains("system_locale: unknown") && text.contains("ui_language: en"));
    }
}
