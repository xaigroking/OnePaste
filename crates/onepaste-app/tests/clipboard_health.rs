use onepaste_app::*;
use std::{path::PathBuf, sync::Arc};

struct UnavailableClipboard;
impl ClipboardPort for UnavailableClipboard {
    fn write(&self, _: &Content, _: bool) -> Result<(), AppError> {
        Ok(())
    }
    fn start_monitor(
        &self,
        _: Box<dyn Fn(ClipEvent) + Send + Sync>,
    ) -> Result<MonitorGuard, AppError> {
        Err(AppError::Clipboard("os_5".into()))
    }
}
fn app() -> App {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "onepaste-capture-health-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    App::new(
        dir,
        Arc::new(MemoryKeyStore::new()),
        Arc::new(UnavailableClipboard),
        AppConfig {
            background_sync: false,
            listen_for_changes: false,
            ..AppConfig::default()
        },
    )
}

#[tokio::test]
async fn failed_native_listener_is_not_reported_as_healthy() {
    let app = app();
    app.choose_local_only().await.unwrap();
    let status = serde_json::to_value(app.status().await).unwrap();
    app.shutdown();
    assert_eq!(status["capture"]["state"], "start_failed");
}

#[tokio::test]
async fn diagnostics_distinguish_the_enabled_switch_from_an_unavailable_listener() {
    let app = app();
    app.choose_local_only().await.unwrap();
    let text = app.diagnostics().await;
    app.shutdown();
    assert!(text.contains("record clipboard: true"));
    assert!(text.contains("clipboard capture: start_failed"), "{text}");
}

struct ReconnectingClipboard {
    fail: std::sync::atomic::AtomicBool,
    inner: FakeClipboard,
}
impl ClipboardPort for ReconnectingClipboard {
    fn write(&self, content: &Content, origin: bool) -> Result<(), AppError> {
        self.inner.write(content, origin)
    }
    fn start_monitor(
        &self,
        callback: Box<dyn Fn(ClipEvent) + Send + Sync>,
    ) -> Result<MonitorGuard, AppError> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::Clipboard("os_5".into()));
        }
        self.inner.start_monitor(callback)
    }
}

#[tokio::test]
async fn retry_recovers_actual_encrypted_capture_and_read_failure_is_visible_without_content() {
    use std::sync::atomic::Ordering;
    let dir = std::env::temp_dir().join(format!(
        "onepaste-capture-retry-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    let keys = Arc::new(EncryptedKeyStore::new(dir.clone()));
    let clip = Arc::new(ReconnectingClipboard {
        fail: std::sync::atomic::AtomicBool::new(true),
        inner: FakeClipboard::new(),
    });
    let app = App::new(
        dir.clone(),
        keys.clone(),
        clip.clone(),
        AppConfig {
            background_sync: false,
            listen_for_changes: false,
            ..AppConfig::default()
        },
    );
    app.prepare_local_passphrase("independent health test passphrase")
        .unwrap();
    app.choose_local_only().await.unwrap();
    assert_eq!(app.status().await.capture.state, CaptureState::StartFailed);
    clip.fail.store(false, Ordering::SeqCst);
    app.retry_clipboard_monitor().await.unwrap();
    assert_eq!(app.status().await.capture.state, CaptureState::Listening);
    app.on_clip_event(ClipEvent::ReadFailed("clipboard_busy".into()))
        .await;
    let status = app.status().await;
    assert_eq!(status.capture.state, CaptureState::ReadError);
    assert_eq!(status.capture.read_failures, 1);
    app.on_clip_event(ClipEvent::Captured(Content::Text(
        "ordinary diagnostic privacy fixture".into(),
    )))
    .await;
    let status = app.status().await;
    assert_eq!(status.capture.state, CaptureState::Listening);
    assert_eq!(status.capture.recorded, 1);
    assert_eq!(
        app.search("privacy fixture", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    app.on_clip_event(ClipEvent::ReadFailed("untrusted private error text".into()))
        .await;
    let diagnostics = app.diagnostics().await;
    assert!(!diagnostics.contains("ordinary diagnostic privacy fixture"));
    assert!(!diagnostics.contains("untrusted private error text"));
    assert!(diagnostics.contains("clipboard error: unavailable"));
    app.lock_session().await.unwrap();
    assert_eq!(app.status().await.capture.state, CaptureState::Stopped);
    assert_eq!(
        app.retry_clipboard_monitor().await.unwrap_err(),
        AppError::Locked
    );
    assert!(keys.load().unwrap().is_none());
    app.unlock_local("independent health test passphrase")
        .await
        .unwrap();
    assert_eq!(app.status().await.capture.state, CaptureState::Listening);
    assert_eq!(
        app.search("privacy fixture", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    app.shutdown();
}

#[tokio::test(start_paused = true)]
async fn automatic_listener_retries_are_bounded_and_locking_cancels_them() {
    let app = app();
    app.choose_local_only().await.unwrap();
    tokio::task::yield_now().await;
    for seconds in [2, 5, 15] {
        tokio::time::advance(std::time::Duration::from_secs(seconds)).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(app.status().await.capture.retries, 3);
    tokio::time::advance(std::time::Duration::from_secs(120)).await;
    tokio::task::yield_now().await;
    assert_eq!(app.status().await.capture.retries, 3);
    app.retry_clipboard_monitor().await.unwrap();
    app.lock_session().await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(120)).await;
    tokio::task::yield_now().await;
    assert_eq!(app.status().await.capture.state, CaptureState::Stopped);
    assert_eq!(app.status().await.capture.retries, 3);
    app.shutdown();
}
