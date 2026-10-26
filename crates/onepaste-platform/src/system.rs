//! System information (for diagnostics).

/// A readable description of the OS version; falls back to the platform name.
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
