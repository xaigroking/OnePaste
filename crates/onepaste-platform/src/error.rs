use thiserror::Error;

/// Error messages contain only a stable category and system error codes, never clipboard content or keys.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlatformError {
    #[error("not supported on this platform")]
    Unsupported,
    #[error("clipboard is busy")]
    ClipboardBusy,
    #[error("clipboard content has an unsupported format")]
    BadFormat,
    #[error("platform call failed (code {0})")]
    Os(i64),
}

impl PlatformError {
    /// A stable, displayable error code.
    pub fn code(&self) -> String {
        match self {
            PlatformError::Unsupported => "unsupported".into(),
            PlatformError::ClipboardBusy => "clipboard_busy".into(),
            PlatformError::BadFormat => "bad_format".into(),
            PlatformError::Os(c) => format!("os_{c}"),
        }
    }
}
