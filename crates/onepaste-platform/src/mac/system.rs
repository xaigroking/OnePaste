use objc2_foundation::NSProcessInfo;

pub(crate) fn os_version() -> String {
    format!("macOS {}", NSProcessInfo::processInfo().operatingSystemVersionString())
}
