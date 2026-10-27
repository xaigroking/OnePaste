use onepaste_sync::PauseReason;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("a passphrase is required to join an existing vault")]
    PassphraseRequired,
    #[error("wrong recovery kit")]
    WrongRecoveryKit,
    #[error("this directory has not joined a vault yet")]
    NotJoined,
    #[error("this directory has already joined a vault")]
    AlreadyJoined,
    #[error("invalid connection code")]
    BadConnectionCode,
    #[error("could not reach the server ({0})")]
    Transport(String),
    #[error("the server answered {status} {code}")]
    Server { status: u16, code: String },
    #[error("syncing is paused: {}", .0.as_str())]
    Paused(PauseReason),
    #[error("the sync engine failed: {0}")]
    Sync(onepaste_sync::Error),
    #[error("{0}")]
    Core(onepaste_core::Error),
    #[error("local storage failed: {0}")]
    Storage(String),
    #[error("{0}")]
    Other(String),
}

impl From<onepaste_sync::Error> for ClientError {
    fn from(e: onepaste_sync::Error) -> Self {
        ClientError::Sync(e)
    }
}

impl From<onepaste_core::Error> for ClientError {
    fn from(e: onepaste_core::Error) -> Self {
        match e {
            onepaste_core::Error::WrongPassphrase => ClientError::WrongPassphrase,
            onepaste_core::Error::WrongRecoveryKit | onepaste_core::Error::InvalidRecoveryKit => ClientError::WrongRecoveryKit,
            other => ClientError::Core(other),
        }
    }
}

impl From<rusqlite::Error> for ClientError {
    fn from(e: rusqlite::Error) -> Self {
        ClientError::Storage(e.to_string())
    }
}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        ClientError::Storage(e.to_string())
    }
}

impl ClientError {
    /// The process exit code the command line tool uses.
    pub fn exit_code(&self) -> u8 {
        match self {
            ClientError::WrongPassphrase | ClientError::WrongRecoveryKit => 3,
            ClientError::Paused(PauseReason::ServerRollback) => 4,
            ClientError::Paused(PauseReason::Revoked) => 5,
            _ => 1,
        }
    }
}
