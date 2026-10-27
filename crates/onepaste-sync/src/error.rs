use crate::store::StoreError;
use thiserror::Error;

/// Errors carry stable codes only, never keys, tokens or plaintext.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("storage failure")]
    Store(#[from] StoreError),
    #[error("{0}")]
    Core(#[from] onepaste_core::Error),
    #[error("item not found")]
    NotFound,
    #[error("the body of an item cannot be changed after creation")]
    BodyImmutable,
    #[error("the item is deleted")]
    Deleted,
    #[error("malformed response")]
    MalformedResponse,
}

impl Error {
    /// A stable code for display and exit-code mapping.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Store(_) => "storage_failure",
            Error::Core(e) => e.code(),
            Error::NotFound => "not_found",
            Error::BodyImmutable => "body_immutable",
            Error::Deleted => "deleted",
            Error::MalformedResponse => "malformed_response",
        }
    }
}
