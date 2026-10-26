//! Platform adapters: clipboard monitoring and access, system secure storage, wake-from-sleep and network change events.
//!
//! Windows and macOS have real implementations; on every other platform (Linux included) all functions return
//! [`PlatformError::Unsupported`], so the whole workspace still builds and tests on Linux.
//! This crate writes no logs and never puts clipboard content or keys into any error message.

pub mod clipboard;
pub mod network;
pub mod power;
pub mod secure_store;
pub mod system;

mod error;
mod png_info;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(not(any(windows, target_os = "macos")))]
mod unsupported;
#[cfg(windows)]
mod win;

pub use error::PlatformError;
