//! 系统安全存储（Windows 凭据管理器 / macOS 钥匙串）里保存 32 字节密钥。
//! 读出的值不会被记录到任何日志或错误信息里。

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

/// 写入（覆盖）。
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

/// 读取；不存在返回 `NotFound`。
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

/// 删除；不存在返回 `NotFound`。
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
