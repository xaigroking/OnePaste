//! 剪贴板：读取、写入（带防回环标记）和变化监听。
//!
//! 带敏感标记的内容只报告「已跳过」，**不读取内容**。

use crate::error::PlatformError;

#[cfg(target_os = "macos")]
use crate::mac::clipboard as imp;
#[cfg(not(any(windows, target_os = "macos")))]
use crate::unsupported::clipboard as imp;
#[cfg(windows)]
use crate::win::clipboard as imp;

#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// PNG 文件字节。
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Image(Image),
}

// 内容不进 Debug 输出。
impl core::fmt::Debug for Content {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Content::Text(s) => write!(f, "Text({} bytes)", s.len()),
            Content::Image(i) => write!(f, "Image({}x{})", i.width, i.height),
        }
    }
}

/// 剪贴板当前状态。带敏感标记时 `content` 恒为 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub content: Option<Content>,
    pub sensitive: bool,
    pub from_self: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// 写入防回环标记（默认写入）。
    pub origin_marker: bool,
    /// 额外写入敏感标记。仅供测试：模拟密码管理器。
    pub sensitive_marker: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions { origin_marker: true, sensitive_marker: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 别的程序复制了新内容。
    Captured(Content),
    /// 带敏感标记，没有读取内容。
    SkippedSensitive,
    /// 本程序自己写入的，防回环跳过。
    SkippedSelf,
}

/// 读取当前剪贴板。
pub fn read() -> Result<Snapshot, PlatformError> {
    imp::read()
}

/// 写入剪贴板，同时写防回环标记。
pub fn write(content: &Content) -> Result<(), PlatformError> {
    imp::write(content, WriteOptions::default())
}

/// 按选项写入。`sensitive_marker` 和关闭 `origin_marker` 只用于测试。
#[doc(hidden)]
pub fn write_with(content: &Content, opts: WriteOptions) -> Result<(), PlatformError> {
    imp::write(content, opts)
}

/// 把一次剪贴板状态变化归类成事件；没有可采集内容时返回 `None`。
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

/// 剪贴板变化监听。丢弃即停止。
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
