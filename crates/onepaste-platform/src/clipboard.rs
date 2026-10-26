//! Clipboard: reading, writing (with an origin marker against feedback loops) and change monitoring.
//!
//! Content that carries a sensitive marker is only reported as "skipped"; its **content is never read**.

use crate::error::PlatformError;

#[cfg(target_os = "macos")]
use crate::mac::clipboard as imp;
#[cfg(not(any(windows, target_os = "macos")))]
use crate::unsupported::clipboard as imp;
#[cfg(windows)]
use crate::win::clipboard as imp;

#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// PNG file bytes.
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Image(Image),
}

// Content never appears in Debug output.
impl core::fmt::Debug for Content {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Content::Text(s) => write!(f, "Text({} bytes)", s.len()),
            Content::Image(i) => write!(f, "Image({}x{})", i.width, i.height),
        }
    }
}

/// Current clipboard state. `content` is always `None` when a sensitive marker is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub content: Option<Content>,
    pub sensitive: bool,
    pub from_self: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Write the origin marker (on by default).
    pub origin_marker: bool,
    /// Also write a sensitive marker. For tests only: simulates a password manager.
    pub sensitive_marker: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions { origin_marker: true, sensitive_marker: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Another program copied new content.
    Captured(Content),
    /// Carries a sensitive marker; the content was not read.
    SkippedSensitive,
    /// Written by this program itself; skipped to avoid a feedback loop.
    SkippedSelf,
}

/// Reads the current clipboard.
pub fn read() -> Result<Snapshot, PlatformError> {
    imp::read()
}

/// Writes the clipboard together with the origin marker.
pub fn write(content: &Content) -> Result<(), PlatformError> {
    imp::write(content, WriteOptions::default())
}

/// Writes with explicit options. `sensitive_marker` and turning `origin_marker` off are for tests only.
#[doc(hidden)]
pub fn write_with(content: &Content, opts: WriteOptions) -> Result<(), PlatformError> {
    imp::write(content, opts)
}

/// Classifies one clipboard change into an event; returns `None` when there is nothing to capture.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub(crate) fn classify(s: Snapshot) -> Option<Event> {
    if s.sensitive {
        Some(Event::SkippedSensitive)
    } else if s.from_self {
        Some(Event::SkippedSelf)
    } else {
        s.content.map(Event::Captured)
    }
}

/// Clipboard change monitor. Dropping it stops it.
pub struct Monitor {
    _inner: imp::Monitor,
}

impl Monitor {
    pub fn start(on_event: impl Fn(Event) + Send + 'static) -> Result<Monitor, PlatformError> {
        Ok(Monitor { _inner: imp::Monitor::start(Box::new(on_event))? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(content: Option<Content>, sensitive: bool, from_self: bool) -> Snapshot {
        Snapshot { content, sensitive, from_self }
    }

    #[test]
    fn classify_priorities() {
        let t = Some(Content::Text("x".into()));
        assert_eq!(classify(snap(t.clone(), true, true)), Some(Event::SkippedSensitive));
        assert_eq!(classify(snap(t.clone(), false, true)), Some(Event::SkippedSelf));
        assert_eq!(classify(snap(t, false, false)), Some(Event::Captured(Content::Text("x".into()))));
        assert_eq!(classify(snap(None, false, false)), None);
    }

    #[test]
    fn debug_never_shows_content() {
        let shown = format!("{:?}", Content::Text("hunter2-secret".into()));
        assert!(!shown.contains("hunter2"));
    }
}
