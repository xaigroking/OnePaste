use thiserror::Error;

/// 对外只暴露**稳定错误码**（`code()`），文案由客户端本地化。
/// 错误信息不得包含密钥、口令或明文。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("invalid argument")]
    InvalidArgument(&'static str),
    #[error("unsupported format version {0}")]
    UnsupportedVersion(u8),
    #[error("malformed envelope")]
    MalformedEnvelope,
    #[error("payload too large")]
    TooLarge,
    #[error("decryption failed")]
    DecryptFailed,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("kdf parameters below minimum")]
    WeakKdf,
    #[error("randomness unavailable")]
    Rng,
    #[error("internal error")]
    Internal,
}

impl Error {
    /// 稳定、可序列化的错误码；新增只增不改。
    pub fn code(&self) -> &'static str {
        match self {
            Error::InvalidArgument(_) => "invalid_argument",
            Error::UnsupportedVersion(_) => "unsupported_version",
            Error::MalformedEnvelope => "malformed_envelope",
            Error::TooLarge => "too_large",
            Error::DecryptFailed => "decrypt_failed",
            Error::WrongPassphrase => "wrong_passphrase",
            Error::WeakKdf => "weak_kdf",
            Error::Rng => "rng_unavailable",
            Error::Internal => "internal",
        }
    }
}
