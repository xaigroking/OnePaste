use onepaste_client::ClientError;
use onepaste_sync::PauseReason;
use thiserror::Error;

/// Every error has a stable code for the interface. No message ever holds clipboard content, a passphrase or a key.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AppError {
    #[error("this device has not joined a vault yet")]
    NotJoined,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("the server has no vault yet; creating one is not available here")]
    VaultEmpty,
    #[error("the recovery kit is not valid or belongs to another vault")]
    WrongRecoveryKit,
    #[error("this program is already set up")]
    AlreadySetUp,
    #[error("there is no setup waiting to be confirmed")]
    NoPendingSetup,
    #[error("this mode has no recovery kit")]
    NoRecoveryKit,
    #[error("this device cannot remove itself from the vault here")]
    CannotRevokeThisDevice,
    #[error("the connection code is not valid")]
    InvalidConnectionCode,
    #[error("the server cannot be reached")]
    Unreachable,
    #[error("the server answered with {0}")]
    Server(String),
    #[error("this device was revoked")]
    Revoked,
    #[error("the server reports an older state than this device has seen")]
    ServerRollback,
    #[error("the system secure store failed ({0})")]
    SecretStore(String),
    #[error("the stored key does not open this vault")]
    WrongKey,
    #[error("item not found")]
    NotFound,
    #[error("the image is too large to sync")]
    ImageTooLarge,
    #[error("clipboard failed ({0})")]
    Clipboard(String),
    #[error("storage failed ({0})")]
    Storage(String),
    #[error("looking for updates failed ({0})")]
    Update(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// The code the interface translates into a message.
    pub fn code(&self) -> String {
        match self {
            AppError::NotJoined => "not_joined".into(),
            AppError::WrongPassphrase => "wrong_passphrase".into(),
            AppError::VaultEmpty => "vault_empty".into(),
            AppError::WrongRecoveryKit => "wrong_recovery_kit".into(),
            AppError::AlreadySetUp => "already_set_up".into(),
            AppError::NoPendingSetup => "no_pending_setup".into(),
            AppError::NoRecoveryKit => "no_recovery_kit".into(),
            AppError::CannotRevokeThisDevice => "cannot_revoke_this_device".into(),
            AppError::InvalidConnectionCode => "invalid_connection_code".into(),
            AppError::Unreachable => "unreachable".into(),
            AppError::Server(c) => c.clone(),
            AppError::Revoked => "revoked".into(),
            AppError::ServerRollback => "server_rollback".into(),
            AppError::SecretStore(c) => format!("secret_store_{c}"),
            AppError::WrongKey => "wrong_key".into(),
            AppError::NotFound => "not_found".into(),
            AppError::ImageTooLarge => "image_too_large".into(),
            AppError::Clipboard(c) => format!("clipboard_{c}"),
            AppError::Storage(_) => "storage".into(),
            AppError::Update(c) => c.clone(),
            AppError::Other(_) => "internal".into(),
        }
    }
}

impl From<ClientError> for AppError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::WrongPassphrase => AppError::WrongPassphrase,
            ClientError::VaultEmpty => AppError::VaultEmpty,
            ClientError::WrongRecoveryKit => AppError::WrongRecoveryKit,
            ClientError::BadConnectionCode => AppError::InvalidConnectionCode,
            ClientError::Transport(_) => AppError::Unreachable,
            ClientError::Server { code, .. } => AppError::Server(code),
            ClientError::Paused(PauseReason::Revoked) => AppError::Revoked,
            ClientError::Paused(PauseReason::ServerRollback) => AppError::ServerRollback,
            ClientError::NotJoined => AppError::NotJoined,
            ClientError::WrongKey => AppError::WrongKey,
            ClientError::Storage(m) => AppError::Storage(m),
            other => AppError::Other(other.to_string()),
        }
    }
}

impl From<onepaste_sync::Error> for AppError {
    fn from(e: onepaste_sync::Error) -> Self {
        match e {
            onepaste_sync::Error::NotFound => AppError::NotFound,
            other => AppError::Other(other.code().to_string()),
        }
    }
}

impl From<onepaste_history::HistoryError> for AppError {
    fn from(e: onepaste_history::HistoryError) -> Self {
        AppError::Storage(e.to_string())
    }
}
