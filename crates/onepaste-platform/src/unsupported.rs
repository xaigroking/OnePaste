//! 不支持的平台：所有入口都返回 `Unsupported`。

use crate::error::PlatformError;

pub(crate) mod clipboard {
    use super::*;
    use crate::clipboard::{Content, Event, Snapshot, WriteOptions};

    pub(crate) fn read() -> Result<Snapshot, PlatformError> {
        Err(PlatformError::Unsupported)
    }
    pub(crate) fn write(_: &Content, _: WriteOptions) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
    pub(crate) struct Monitor;
    impl Monitor {
        pub(crate) fn start(_: Box<dyn Fn(Event) + Send>) -> Result<Monitor, PlatformError> {
            Err(PlatformError::Unsupported)
        }
    }
}

pub(crate) mod network {
    use super::*;
    use crate::network::NetworkState;

    pub(crate) struct Watcher;
    impl Watcher {
        pub(crate) fn start(_: Box<dyn Fn(NetworkState) + Send + Sync>) -> Result<Watcher, PlatformError> {
            Err(PlatformError::Unsupported)
        }
    }
}

pub(crate) mod power {
    use super::*;

    pub(crate) struct Watcher;
    impl Watcher {
        pub(crate) fn start(_: Box<dyn Fn() + Send + Sync>) -> Result<Watcher, PlatformError> {
            Err(PlatformError::Unsupported)
        }
    }
}
