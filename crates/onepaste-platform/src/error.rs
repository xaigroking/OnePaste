use thiserror::Error;

/// 错误信息只含稳定的分类和系统错误码，不含剪贴板内容或密钥。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PlatformError {
    #[error("not supported on this platform")]
    Unsupported,
    #[error("clipboard is busy")]
    ClipboardBusy,
    #[error("clipboard content has an unsupported format")]
    BadFormat,
    #[error("secure store entry not found")]
    NotFound,
    #[error("secure store value has an unexpected length")]
    BadLength,
    #[error("platform call failed (code {0})")]
    Os(i64),
}

impl PlatformError {
    /// 稳定、可显示的错误码。
    pub fn code(&self) -> String {
        match self {
            PlatformError::Unsupported => "unsupported".into(),
            PlatformError::ClipboardBusy => "clipboard_busy".into(),
            PlatformError::BadFormat => "bad_format".into(),
            PlatformError::NotFound => "not_found".into(),
            PlatformError::BadLength => "bad_length".into(),
            PlatformError::Os(c) => format!("os_{c}"),
        }
    }
}
