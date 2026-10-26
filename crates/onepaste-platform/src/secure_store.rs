//! Keeps a 32-byte secret in the system secure store (Windows Credential Manager / macOS Keychain).
//! Values read back are never written to any log or error message.

use crate::error::PlatformError;

pub const SERVICE: &str = "OnePaste";
pub const SECRET_LEN: usize = 32;

#[cfg(any(windows, target_os = "macos"))]
fn entry(account: &str) -> Result<keyring::Entry, PlatformError> {
    keyring::Entry::new(SERVICE, account).map_err(map_err)
}

#[cfg(any(windows, target_os = "macos"))]
fn map_err(e: keyring::Error) -> PlatformError {
    match e {
        keyring::Error::NoEntry => PlatformError::NotFound,
        keyring::Error::TooLong(..) | keyring::Error::Invalid(..) => PlatformError::BadFormat,
        _ => PlatformError::Os(-1),
    }
}

/// Writes (overwrites).
pub fn write(account: &str, secret: &[u8; SECRET_LEN]) -> Result<(), PlatformError> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        entry(account)?.set_secret(secret).map_err(map_err)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (account, secret);
        Err(PlatformError::Unsupported)
    }
}

/// Reads; returns `NotFound` if absent.
pub fn read(account: &str) -> Result<[u8; SECRET_LEN], PlatformError> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let bytes = entry(account)?.get_secret().map_err(map_err)?;
        bytes.try_into().map_err(|_| PlatformError::BadLength)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = account;
        Err(PlatformError::Unsupported)
    }
}

/// Deletes; returns `NotFound` if absent.
pub fn delete(account: &str) -> Result<(), PlatformError> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        entry(account)?.delete_credential().map_err(map_err)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = account;
        Err(PlatformError::Unsupported)
    }
}
