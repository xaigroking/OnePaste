#![cfg(not(any(windows, target_os = "macos")))]

use onepaste_platform::{clipboard, network, power, secure_store, PlatformError};

#[test]
fn every_entry_point_reports_unsupported() {
    assert_eq!(clipboard::read().unwrap_err(), PlatformError::Unsupported);
    assert_eq!(clipboard::write(&clipboard::Content::Text("x".into())).unwrap_err(), PlatformError::Unsupported);
    assert!(clipboard::Monitor::start(|_| {}).is_err());
    assert!(network::NetworkWatcher::start(|_| {}).is_err());
    assert!(power::WakeWatcher::start(|| {}).is_err());
    assert_eq!(secure_store::write("a", &[0; 32]).unwrap_err(), PlatformError::Unsupported);
    assert_eq!(secure_store::read("a").unwrap_err(), PlatformError::Unsupported);
    assert_eq!(secure_store::delete("a").unwrap_err(), PlatformError::Unsupported);
}
