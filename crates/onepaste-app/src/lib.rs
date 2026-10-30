//! OnePaste desktop application logic.
//!
//! Everything that is not drawing a window lives here, so it can be tested on any machine: joining a vault with a
//! connection code, keeping the master key in the system secure store, capturing the clipboard, writing items that
//! arrive from other devices, driving the sync, searching the encrypted history. The platform (secure store,
//! clipboard) is injected through the traits in [`ports`].

mod app;
mod error;
pub mod ports;
pub mod instance;
mod settings;
mod secrets;
mod server_change;
pub mod update;

pub use app::{App, AppConfig, ImageData, UpdateSetup, DeviceList, DeviceView, ItemView, LogEntry, Mode, ServerVault, Status, TrayState};
pub use error::AppError;
pub use onepaste_history::Filter;
pub use ports::{ClipEvent, ClipboardPort, Content, FakeClipboard, MemorySecretStore, MonitorGuard, SecretStore};
pub use settings::Settings;
pub use update::{Channel, UpdateInfo};

/// Parses the filter names the interface uses.
pub fn parse_filter(name: &str) -> Filter {
    match name {
        "favorites" => Filter::Favorites,
        "text" => Filter::Text,
        "images" => Filter::Images,
        _ => Filter::All,
    }
}
