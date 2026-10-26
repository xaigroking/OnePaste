use thiserror::Error;

/// Only **stable error codes** (`code()`) are exposed; clients localize the wording.
/// Error messages must never contain keys, passphrases or plaintext.
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
    #[error("invalid recovery kit")]
    InvalidRecoveryKit,
    #[error("recovery kit does not belong to this vault")]
    WrongRecoveryKit,
    #[error("kdf parameters below minimum")]
    WeakKdf,
    #[error("randomness unavailable")]
    Rng,
    #[error("internal error")]
    Internal,
}

impl Error {
    /// A stable, serializable error code; new codes may be added but existing ones never change.
    pub fn code(&self) -> &'static str {
        match self {
            Error::InvalidArgument(_) => "invalid_argument",
            Error::UnsupportedVersion(_) => "unsupported_version",
            Error::MalformedEnvelope => "malformed_envelope",
            Error::TooLarge => "too_large",
            Error::DecryptFailed => "decrypt_failed",
            Error::WrongPassphrase => "wrong_passphrase",
            Error::InvalidRecoveryKit => "invalid_recovery_kit",
            Error::WrongRecoveryKit => "wrong_recovery_kit",
            Error::WeakKdf => "weak_kdf",
            Error::Rng => "rng_unavailable",
            Error::Internal => "internal",
        }
    }
}
