//! The platform clipboard adapter. Key management is owned by the application, independently of platform services. These implement the traits of `onepaste-app` on top of `onepaste-platform`.

use onepaste_app::{AppError, ClipEvent, ClipboardPort, Content, MonitorGuard};
use onepaste_platform::clipboard::{self, Content as PlatformContent, Event as PlatformEvent, Image, WriteOptions};

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

/// Tests against the real system: only meaningful on Windows and macOS, where CI has a native clipboard.
#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    /// The clipboard is global, so tests that use it run one at a time.
    static CLIPBOARD: Mutex<()> = Mutex::new(());

    pub(super) fn exclusive() -> MutexGuard<'static, ()> {
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

}

/// Uses the same key provider as the packaged entrypoint, plus the real OS clipboard on supported desktop runners.
#[cfg(all(test, any(windows, target_os = "macos")))]
mod key_access_tests {
    use super::*;
    use onepaste_app::{App, AppConfig, EncryptedKeyStore, KeyState, KeyStore, Filter, KEY_FILE};
    use std::{sync::Arc,time::Duration};
    #[test]
    fn native_local_profile_is_locked_after_restart_and_uses_only_its_own_encrypted_key_file() {
        let _clipboard = super::tests::exclusive();
        tauri::async_runtime::block_on(async {
            let dir=std::env::temp_dir().join(format!("onepaste-native-key-{}",onepaste_core::id::new_id().unwrap()));
            let store=Arc::new(EncryptedKeyStore::new(dir.clone()));
            let core=App::new(dir.clone(),store.clone(),Arc::new(PlatformClipboard),AppConfig { background_sync:false,listen_for_changes:false,..AppConfig::default() });
            assert_eq!(core.key_state().unwrap(),KeyState::New); assert!(!core.start().await.unwrap());
            core.prepare_local_passphrase("native independent application passphrase").unwrap(); core.choose_local_only().await.unwrap();
            PlatformClipboard.write(&Content::Text("native encrypted history fixture".into()),false).unwrap();
            wait_for_history(&core,"native encrypted").await;
            let kit=core.show_kit().unwrap(); let key=store.load().unwrap().unwrap();
            let raw=std::fs::read(dir.join(KEY_FILE)).unwrap(); assert!(!raw.windows(32).any(|part|part==key.as_bytes()));
            core.shutdown(); drop(core);
            for _ in 0..200 { if Arc::strong_count(&store)==1 { break; } std::thread::sleep(Duration::from_millis(50)); }
            assert_eq!(Arc::strong_count(&store),1); assert!(store.load().unwrap().is_none()); drop(store);
            let again_store=Arc::new(EncryptedKeyStore::new(dir.clone()));
            let again=App::new(dir.clone(),again_store.clone(),Arc::new(PlatformClipboard),AppConfig { background_sync:false,listen_for_changes:false,..AppConfig::default() });
            assert_eq!(again.key_state().unwrap(),KeyState::Locked); assert!(!again.start().await.unwrap());
            assert!(again.unlock_local("wrong independent password").await.is_err());
            again.unlock_local("native independent application passphrase").await.unwrap();
            assert_eq!(again.search("native encrypted",Filter::All,10,0).await.unwrap().len(),1); assert_eq!(again.show_kit().unwrap(),kit);
            again.wipe_local_data().await.unwrap(); assert!(!dir.join(KEY_FILE).exists()); assert_eq!(again.key_state().unwrap(),KeyState::New);
            // Missing old credentials must not block an explicit fresh start on the native clipboard path.
            again.prepare_local_passphrase("another native application passphrase").unwrap();
            again.choose_local_only().await.unwrap();
            PlatformClipboard.write(&Content::Text("previous profile before fresh start".into()),false).unwrap();
            wait_for_history(&again,"previous profile").await;
            again.lock_session().await.unwrap();
            std::fs::write(dir.join(KEY_FILE), "corrupt previous unlock record").unwrap();
            again.restart_local_setup().await.unwrap();
            assert_eq!(again.key_state().unwrap(),KeyState::New);
            again.prepare_local_passphrase("fresh native application passphrase").unwrap();
            again.choose_local_only().await.unwrap();
            assert!(again.search("previous profile",Filter::All,10,0).await.unwrap().is_empty());
            PlatformClipboard.write(&Content::Text("fresh profile remains usable".into()),false).unwrap();
            wait_for_history(&again,"fresh profile").await;
            again.shutdown();
        });
    }
    async fn wait_for_history(core:&App,query:&str) {
        for _ in 0..200 {
            if !core.search(query,Filter::All,10,0).await.unwrap().is_empty() { return; }
            // This helper is used in the blocking native test thread; the async runtime continues on its workers.
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the native clipboard item did not reach encrypted history");
    }
}
