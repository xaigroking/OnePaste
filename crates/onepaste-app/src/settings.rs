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
        Settings { auto_apply_remote: true, record_clipboard: true, skip_suspected_secrets: true, check_updates: true, update_channel: None, skipped_update: None, tray_notice_dismissed: false, text_preview_height: None }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join(FILE_NAME)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), AppError> {
        write_private(dir, FILE_NAME, serde_json::to_string(self).unwrap_or_default().as_bytes())
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

impl StateFile {
    pub fn load(dir: &Path) -> StateFile {
        std::fs::read_to_string(dir.join(STATE_FILE_NAME)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), AppError> {
        write_private(dir, STATE_FILE_NAME, serde_json::to_string(self).unwrap_or_default().as_bytes())
    }
}
