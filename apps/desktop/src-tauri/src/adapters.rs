//! The platform side of the application logic: the system secure store holds the master key, the system clipboard is
//! read and written. These implement the traits of `onepaste-app` on top of `onepaste-platform`.

use onepaste_app::{AppError, ClipEvent, ClipboardPort, Content, MonitorGuard, SecretStore};
use onepaste_core::crypto::Key;
use onepaste_platform::clipboard::{self, Content as PlatformContent, Event as PlatformEvent, Image, WriteOptions};
use onepaste_platform::{secure_store, PlatformError};

/// The account name of the master key in the system secure store.
pub const MASTER_KEY_ACCOUNT: &str = "master-key";

pub struct PlatformSecretStore {
    account: String,
}

impl PlatformSecretStore {
    pub fn new(account: &str) -> PlatformSecretStore {
        PlatformSecretStore { account: account.to_string() }
    }
}

fn secret_error(e: PlatformError) -> AppError {
    AppError::SecretStore(e.code())
}

impl SecretStore for PlatformSecretStore {
    fn save(&self, key: &Key) -> Result<(), AppError> {
        secure_store::write(&self.account, key.as_bytes()).map_err(secret_error)
    }

    fn load(&self) -> Result<Option<Key>, AppError> {
        match secure_store::read(&self.account) {
            Ok(bytes) => Ok(Some(Key::from_bytes(bytes))),
            Err(PlatformError::NotFound) => Ok(None),
            Err(e) => Err(secret_error(e)),
        }
    }

    fn delete(&self) -> Result<(), AppError> {
        match secure_store::delete(&self.account) {
            Ok(()) | Err(PlatformError::NotFound) => Ok(()),
            Err(e) => Err(secret_error(e)),
        }
    }
}

pub struct PlatformClipboard;

fn to_platform(c: &Content) -> PlatformContent {
    match c {
        Content::Text(t) => PlatformContent::Text(t.clone()),
        Content::Image { png, width, height } => PlatformContent::Image(Image { png: png.clone(), width: *width, height: *height }),
    }
}

fn from_platform(c: PlatformContent) -> Content {
    match c {
        PlatformContent::Text(t) => Content::Text(t),
        PlatformContent::Image(i) => Content::Image { png: i.png, width: i.width, height: i.height },
    }
}

fn map_event(e: PlatformEvent) -> ClipEvent {
    match e {
        PlatformEvent::Captured(c) => ClipEvent::Captured(from_platform(c)),
        PlatformEvent::SkippedSensitive => ClipEvent::SkippedSensitive,
        PlatformEvent::SkippedSelf => ClipEvent::SkippedSelf,
    }
}

/// Dropping this stops the monitor thread.
struct StopOnDrop(#[allow(dead_code)] std::sync::mpsc::Sender<()>);

impl ClipboardPort for PlatformClipboard {
    fn write(&self, content: &Content, origin_marker: bool) -> Result<(), AppError> {
        let platform = to_platform(content);
        let result = if origin_marker {
            clipboard::write(&platform)
        } else {
            clipboard::write_with(&platform, WriteOptions { origin_marker: false, sensitive_marker: false })
        };
        result.map_err(|e| AppError::Clipboard(e.code()))
    }

    fn start_monitor(&self, on_event: Box<dyn Fn(ClipEvent) + Send + Sync>) -> Result<MonitorGuard, AppError> {
        // The monitor lives on a thread of its own, so it never has to be moved between threads
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), AppError>>();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || match clipboard::Monitor::start(move |ev| on_event(map_event(ev))) {
            Ok(monitor) => {
                let _ = ready_tx.send(Ok(()));
                // Returns when the guard is dropped
                let _ = stop_rx.recv();
                drop(monitor);
            }
            Err(e) => {
                let _ = ready_tx.send(Err(AppError::Clipboard(e.code())));
            }
        });
        ready_rx.recv().map_err(|_| AppError::Clipboard("monitor_thread".into()))??;
        Ok(MonitorGuard::new(StopOnDrop(stop_tx)))
    }
}

/// Tests against the real system: only meaningful on Windows and macOS, where CI has the secure store and a clipboard.
#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    /// The clipboard is global, so tests that use it run one at a time.
    static CLIPBOARD: Mutex<()> = Mutex::new(());

    fn exclusive() -> MutexGuard<'static, ()> {
        CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn next_event(rx: &Receiver<ClipEvent>, wanted: impl Fn(&ClipEvent) -> bool) -> Option<ClipEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(ev) if wanted(&ev) => return Some(ev),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    }

    #[test]
    fn the_master_key_survives_a_round_trip_through_the_real_secure_store() {
        let store = PlatformSecretStore::new(&format!("onepaste-adapter-test-{}", std::process::id()));
        assert!(store.load().unwrap().is_none(), "nothing is stored under a fresh account");
        let key = Key::from_bytes([0x5a; 32]);
        store.save(&key).unwrap();
        assert_eq!(store.load().unwrap().unwrap().as_bytes(), key.as_bytes());
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none(), "deleted for good");
        store.delete().unwrap();
    }

    #[test]
    fn the_real_clipboard_reports_a_foreign_copy_as_captured_and_our_own_write_as_skipped_self() {
        let _g = exclusive();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let port = PlatformClipboard;
        let _monitor = port
            .start_monitor(Box::new(move |ev| {
                let _ = tx.lock().unwrap().send(ev);
            }))
            .expect("the clipboard monitor starts");
        // Something copied by another program: no origin marker
        port.write(&Content::Text("copied by another program".into()), false).unwrap();
        let got = next_event(&rx, |e| matches!(e, ClipEvent::Captured(_) | ClipEvent::SkippedSelf));
        assert_eq!(got, Some(ClipEvent::Captured(Content::Text("copied by another program".into()))));
        // Our own write, with the origin marker: reported as ours, never as a capture
        port.write(&Content::Text("written by onepaste".into()), true).unwrap();
        let got = next_event(&rx, |e| matches!(e, ClipEvent::Captured(_) | ClipEvent::SkippedSelf));
        assert_eq!(got, Some(ClipEvent::SkippedSelf));
    }

    #[test]
    fn local_only_mode_keeps_its_random_key_in_the_real_secure_store_and_reopens_after_a_restart() {
        use onepaste_app::{App, AppConfig, FakeClipboard, Filter, Mode};
        use std::sync::Arc;
        let id = std::process::id();
        let dir = std::env::temp_dir().join(format!("onepaste-adapter-local-{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        let account = format!("onepaste-local-test-{id}");
        tauri::async_runtime::block_on(async {
            let clip = Arc::new(FakeClipboard::new());
            let app = App::new(dir.clone(), Arc::new(PlatformSecretStore::new(&account)), clip.clone(), AppConfig::default());
            assert_eq!(app.first_run_state().await.unwrap(), Mode::Unset);
            app.choose_local_only().await.unwrap();
            let key = PlatformSecretStore::new(&account).load().unwrap().expect("the random key is in the real secure store");
            clip.user_copies(Content::Text("kept across a restart".into()));
            std::thread::sleep(Duration::from_millis(600));
            assert_eq!(app.search("across a restart", Filter::All, 5, 0).await.unwrap().len(), 1);
            app.shutdown();
            drop(app);
            // The program starts again: the same directory, the key read back from the real secure store
            let again = App::new(dir.clone(), Arc::new(PlatformSecretStore::new(&account)), clip, AppConfig::default());
            assert_eq!(again.first_run_state().await.unwrap(), Mode::LocalOnly);
            assert_eq!(again.search("across a restart", Filter::All, 5, 0).await.unwrap().len(), 1, "the history opens after the restart");
            assert_eq!(PlatformSecretStore::new(&account).load().unwrap().unwrap().as_bytes(), key.as_bytes());
            again.shutdown();
            // Wiping removes the key from the real secure store too
            again.wipe_local_data().await.unwrap();
            assert!(PlatformSecretStore::new(&account).load().unwrap().is_none());
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
