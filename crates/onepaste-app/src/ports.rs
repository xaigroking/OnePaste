//! What the application needs from the platform. The desktop shell implements these on top of `onepaste-platform`;
//! tests use the in-memory versions below.

use crate::error::AppError;
use onepaste_core::crypto::Key;
use std::any::Any;
use std::sync::{Arc, Mutex};

#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Image { png: Vec<u8>, width: u32, height: u32 },
}

// Content never appears in Debug output.
impl core::fmt::Debug for Content {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Content::Text(s) => write!(f, "Text({} bytes)", s.len()),
            Content::Image { width, height, .. } => write!(f, "Image({width}x{height})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipEvent {
    /// Another program copied this.
    Captured(Content),
    /// Carries a sensitive marker; the content was not read.
    SkippedSensitive,
    /// Written by this program itself.
    SkippedSelf,
}

/// Holds the 32-byte master key in the system secure store. The key is never written anywhere else.
pub trait SecretStore: Send + Sync {
    fn save(&self, key: &Key) -> Result<(), AppError>;
    fn load(&self) -> Result<Option<Key>, AppError>;
    fn delete(&self) -> Result<(), AppError>;
}

/// Keeps a clipboard monitor running for as long as it lives.
pub struct MonitorGuard(#[allow(dead_code)] Box<dyn Any + Send>);

impl MonitorGuard {
    pub fn new(inner: impl Any + Send) -> MonitorGuard {
        MonitorGuard(Box::new(inner))
    }
}

pub trait ClipboardPort: Send + Sync {
    /// Writes the clipboard. With `origin_marker` the write is recognizable as this program's own, so the monitor
    /// reports it as `SkippedSelf` instead of capturing it again.
    fn write(&self, content: &Content, origin_marker: bool) -> Result<(), AppError>;
    /// Starts reporting clipboard changes. Events may arrive on any thread.
    fn start_monitor(&self, on_event: Box<dyn Fn(ClipEvent) + Send + Sync>) -> Result<MonitorGuard, AppError>;
}

/// A secret store that keeps the key in memory only.
#[derive(Default)]
pub struct MemorySecretStore {
    key: Mutex<Option<[u8; 32]>>,
}

impl MemorySecretStore {
    pub fn new() -> MemorySecretStore {
        MemorySecretStore::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn save(&self, key: &Key) -> Result<(), AppError> {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = Some(*key.as_bytes());
        Ok(())
    }

    fn load(&self) -> Result<Option<Key>, AppError> {
        Ok(self.key.lock().unwrap_or_else(|e| e.into_inner()).map(Key::from_bytes))
    }

    fn delete(&self) -> Result<(), AppError> {
        *self.key.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

type Callback = Arc<dyn Fn(ClipEvent) + Send + Sync>;

#[derive(Default)]
struct FakeState {
    current: Option<Content>,
    writes: Vec<(Content, bool)>,
    callback: Option<Callback>,
}

/// A clipboard for tests and the test harness. A write with the origin marker is reported as `SkippedSelf`, a write
/// without it as `Captured`, exactly like the real monitor.
#[derive(Default)]
pub struct FakeClipboard {
    state: Mutex<FakeState>,
}

impl FakeClipboard {
    pub fn new() -> FakeClipboard {
        FakeClipboard::default()
    }

    fn fire(&self, ev: ClipEvent) {
        let cb = self.state.lock().unwrap_or_else(|e| e.into_inner()).callback.clone();
        if let Some(cb) = cb {
            cb(ev);
        }
    }

    /// The user copies something in another program.
    pub fn user_copies(&self, content: Content) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).current = Some(content.clone());
        self.fire(ClipEvent::Captured(content));
    }

    /// A password manager copies something and marks it sensitive.
    pub fn user_copies_sensitive(&self, content: Content) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).current = Some(content);
        self.fire(ClipEvent::SkippedSensitive);
    }

    pub fn current(&self) -> Option<Content> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).current.clone()
    }

    /// Every write the application made, with whether it carried the origin marker.
    pub fn writes(&self) -> Vec<(Content, bool)> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).writes.clone()
    }
}

impl ClipboardPort for FakeClipboard {
    fn write(&self, content: &Content, origin_marker: bool) -> Result<(), AppError> {
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.current = Some(content.clone());
            s.writes.push((content.clone(), origin_marker));
        }
        self.fire(if origin_marker { ClipEvent::SkippedSelf } else { ClipEvent::Captured(content.clone()) });
        Ok(())
    }

    fn start_monitor(&self, on_event: Box<dyn Fn(ClipEvent) + Send + Sync>) -> Result<MonitorGuard, AppError> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).callback = Some(Arc::from(on_event));
        Ok(MonitorGuard::new(()))
    }
}
