//! Windows 和 macOS 上的真实平台集成测试：剪贴板、安全存储、事件监听。
#![cfg(any(windows, target_os = "macos"))]

use onepaste_platform::clipboard::{self, Content, Event, Image, Monitor, WriteOptions};
use onepaste_platform::network::{NetworkState, NetworkWatcher};
use onepaste_platform::power::WakeWatcher;
use onepaste_platform::{secure_store, system, PlatformError};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// 剪贴板是全局资源，涉及剪贴板的测试必须串行。
static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    CLIPBOARD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn start_monitor() -> (Monitor, Receiver<Event>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let monitor = Monitor::start(move |ev| {
        let _ = tx.lock().unwrap().send(ev);
    })
    .expect("monitor should start");
    (monitor, rx)
}

/// 在限定时间内等一个满足条件的事件。
fn wait_for(rx: &Receiver<Event>, timeout: Duration, pred: impl Fn(&Event) -> bool) -> Option<Event> {
    let deadline = Instant::now() + timeout;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ev) if pred(&ev) => return Some(ev),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
    None
}

fn sample_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255]).collect();
        writer.write_image_data(&px).unwrap();
    }
    out
}

const WAIT: Duration = Duration::from_secs(8);

#[test]
fn text_write_then_read_back() {
    let _g = lock();
    let text = "第一行 héllo\n第二行 \u{1F600}".to_string();
    clipboard::write(&Content::Text(text.clone())).unwrap();
    let snap = clipboard::read().unwrap();
    assert_eq!(snap.content, Some(Content::Text(text)));
    assert!(!snap.sensitive);
    assert!(snap.from_self, "own writes carry the origin marker");
}

#[test]
fn image_write_then_read_back() {
    let _g = lock();
    let png = sample_png(6, 4);
    let img = Image { png: png.clone(), width: 6, height: 4 };
    clipboard::write(&Content::Image(img)).unwrap();
    match clipboard::read().unwrap().content {
        Some(Content::Image(back)) => {
            assert_eq!((back.width, back.height), (6, 4));
            assert!(back.png.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        other => panic!("expected an image, got {other:?}"),
    }
}

#[test]
fn own_write_does_not_trigger_capture() {
    let _g = lock();
    let (_monitor, rx) = start_monitor();
    clipboard::write(&Content::Text("written by onepaste".into())).unwrap();
    let skipped = wait_for(&rx, WAIT, |e| matches!(e, Event::SkippedSelf | Event::Captured(_)));
    assert_eq!(skipped, Some(Event::SkippedSelf), "an own write must be skipped, never captured");
    assert!(wait_for(&rx, Duration::from_secs(1), |e| matches!(e, Event::Captured(_))).is_none());
}

#[test]
fn foreign_write_is_captured() {
    let _g = lock();
    let (_monitor, rx) = start_monitor();
    let opts = WriteOptions { origin_marker: false, sensitive_marker: false };
    clipboard::write_with(&Content::Text("copied elsewhere".into()), opts).unwrap();
    let got = wait_for(&rx, WAIT, |e| matches!(e, Event::Captured(_)));
    assert_eq!(got, Some(Event::Captured(Content::Text("copied elsewhere".into()))));
}

#[test]
fn sensitive_content_is_skipped_and_never_read() {
    let _g = lock();
    let (_monitor, rx) = start_monitor();
    let opts = WriteOptions { origin_marker: false, sensitive_marker: true };
    clipboard::write_with(&Content::Text("hunter2-password".into()), opts).unwrap();
    let got = wait_for(&rx, WAIT, |e| matches!(e, Event::SkippedSensitive | Event::Captured(_)));
    assert_eq!(got, Some(Event::SkippedSensitive));
    let snap = clipboard::read().unwrap();
    assert!(snap.sensitive);
    assert_eq!(snap.content, None, "flagged content must not be read");
}

#[test]
fn secure_store_write_read_overwrite_delete() {
    let account = format!("onepaste-test-{}", std::process::id());
    let first = [7u8; 32];
    let second = [9u8; 32];
    secure_store::write(&account, &first).unwrap();
    assert_eq!(secure_store::read(&account).unwrap(), first);
    secure_store::write(&account, &second).unwrap();
    assert_eq!(secure_store::read(&account).unwrap(), second);
    secure_store::delete(&account).unwrap();
    assert_eq!(secure_store::read(&account).unwrap_err(), PlatformError::NotFound);
    assert_eq!(secure_store::delete(&account).unwrap_err(), PlatformError::NotFound);
}

#[test]
fn network_watcher_reports_current_state_on_start() {
    let (tx, rx) = channel::<NetworkState>();
    let tx = Mutex::new(tx);
    let _watcher = NetworkWatcher::start(move |s| {
        let _ = tx.lock().unwrap().send(s);
    })
    .expect("network watcher should start");
    assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok(), "expected an initial state callback");
}

#[test]
fn wake_watcher_starts_and_stops_cleanly() {
    let watcher = WakeWatcher::start(|| {}).expect("wake watcher should start");
    drop(watcher);
}

#[test]
fn os_version_is_reported() {
    assert!(!system::os_version().is_empty());
}
