//! 平台适配：剪贴板监听与读写、系统安全存储、睡眠唤醒与网络变化事件。
//!
//! Windows 和 macOS 有真实实现；其他平台（包括 Linux）所有函数都返回
//! [`PlatformError::Unsupported`]，这样整个工作区仍然可以在 Linux 上编译和测试。
//! 本 crate 不写日志，也不把剪贴板内容、密钥打进任何错误信息。

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
