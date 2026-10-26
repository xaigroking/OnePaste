//! 系统信息（用于诊断）。

/// 操作系统版本的可读描述；取不到时返回平台名。
pub fn os_version() -> String {
    #[cfg(windows)]
    {
        crate::win::system::os_version()
    }
    #[cfg(target_os = "macos")]
    {
        crate::mac::system::os_version()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::consts::OS.to_string()
    }
}
