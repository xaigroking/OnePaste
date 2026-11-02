//! OnePaste desktop application logic.
//!
//! Everything that is not drawing a window lives here, so it can be tested on any machine: joining a vault with a
//! connection code, keeping the master key in the application key storage, capturing the clipboard, writing items that
//! arrive from other devices, driving the sync, searching the encrypted history. Application-owned key access and the platform clipboard are injected through the traits in [`ports`].

mod app;
mod error;
mod deletions;
pub mod ports;
pub mod instance;
mod settings;
mod secrets;
mod server_change;
pub mod update;

pub use app::{App, AppConfig, ImageData, UpdateSetup, DeviceList, DeviceView, ItemView, LogEntry, Mode, ServerVault, Status, TrayState};
pub use app::{CaptureHealth, CaptureState};
pub use error::AppError;
pub use deletions::{DeletionClock, DeletionStatus, PendingDeletion, DELETE_GRACE};
pub use onepaste_history::{Filter, TagOption};
pub use ports::{ClipEvent, ClipboardPort, Content, FakeClipboard, MemoryKeyStore, MonitorGuard, KeyStore};
pub use settings::{hotkeys, Settings};
pub use update::{Channel, UpdateInfo};

/// Parses the filter names the interface uses.
pub fn parse_filter(name: &str) -> Result<Filter, AppError> {
    Ok(
    match name {
        "all" => Filter::All,
        "favorites" => Filter::Favorites,
        "text" => Filter::Text,
        "images" => Filter::Images,
        "links" => Filter::Links,
        "code" => Filter::Code,
        "files" => Filter::Files,
        _ => return Err(AppError::InvalidFilter),
    })
}

mod key_storage;
mod unlock;
pub use key_storage::{EncryptedKeyStore, KeyState, KEY_FILE};

mod session_clock;
pub use session_clock::AutoLockClock;
