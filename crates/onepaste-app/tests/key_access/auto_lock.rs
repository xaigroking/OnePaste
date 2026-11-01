//! Controlled wall time and production key files exercise the complete lock/reopen lifecycle.
use onepaste_app::*;
use onepaste_sync::Store;
use std::{sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Duration, path::PathBuf};
const PASS: &str = "independent locking fixture passphrase";
#[derive(Default)]
struct Clock(AtomicU64);
impl AutoLockClock for Clock { fn now(&self) -> Duration { Duration::from_secs(self.0.load(Ordering::SeqCst)) } }
impl Clock { fn advance(&self, seconds: u64) { self.0.fetch_add(seconds, Ordering::SeqCst); } }
impl DeletionClock for Clock {
    fn now(&self) -> Duration { AutoLockClock::now(self) }
    fn wait_until(&self, deadline: Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output=()> + Send + '_>> {
        Box::pin(async move { while AutoLockClock::now(self) < deadline { tokio::time::sleep(Duration::from_millis(1)).await; } })
    }
}
struct Fixture { app: App, dir: PathBuf, keys: Arc<EncryptedKeyStore>, clip: Arc<FakeClipboard>, clock: Arc<Clock> }
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("onepaste-lock-{}", onepaste_core::id::new_id().unwrap()));
        let keys = Arc::new(EncryptedKeyStore::new(dir.clone()));
        let clip = Arc::new(FakeClipboard::new()); let clock = Arc::new(Clock::default());
        let app = App::new(dir.clone(), keys.clone(), clip.clone(), AppConfig { auto_lock_clock: clock.clone(), deletion_clock: clock.clone(), background_sync: false, listen_for_changes: false, ..AppConfig::default() });
        Self { app, dir, keys, clip, clock }
    }
    async fn local(&self) { self.app.prepare_local_passphrase(PASS).unwrap(); self.app.choose_local_only().await.unwrap(); }
    async fn add(&self, text: &str) -> String {
        self.app.on_clip_event(ClipEvent::Captured(Content::Text(text.into()))).await;
        self.app.search(text, Filter::All, 10, 0).await.unwrap()[0].item_id.clone()
    }
    fn snapshot(&self) -> (Vec<onepaste_sync::ItemRow>, Vec<onepaste_sync::OutboxRow>) {
        let mut db = onepaste_client::SqliteStore::open(&onepaste_client::db_path(&self.dir)).unwrap();
        let mut result = None;
        db.transact(&mut |tx| { result = Some((tx.items()?, tx.outbox()?)); Ok(()) }).unwrap(); result.unwrap()
    }
}
impl Drop for Fixture { fn drop(&mut self) { self.app.shutdown(); } }

#[tokio::test]
async fn expiry_counts_tray_and_suspend_but_only_real_app_interaction_resets_it() {
    let d = Fixture::new(); d.local().await;
    let mut settings = d.app.settings(); settings.auto_lock_minutes = 1; d.app.set_settings(settings).unwrap();
    let id = d.add("preserved private history").await;
    d.clock.advance(59); d.app.check_auto_lock().await.unwrap(); assert!(d.app.is_joined());
    d.app.user_activity().await.unwrap(); d.clock.advance(59);
    d.app.notify_network(true); d.app.notify_wake(); d.app.sync_now().await.unwrap();
    d.app.on_clip_event(ClipEvent::Captured(Content::Text("background clipboard".into()))).await;
    let before = d.snapshot();
    // No visibility or global-input callback is needed. A suspended clock jump expires the same deadline.
    d.clock.advance(1); d.app.notify_wake();
    assert!(d.app.status().await.locked);
    d.app.on_clip_event(ClipEvent::Captured(Content::Text("must not resume after sleep".into()))).await;
    assert_eq!(d.snapshot(), before);
    assert_eq!(d.app.user_activity().await, Err(AppError::Locked));
    assert!(d.app.status().await.locked);
    assert_eq!(d.app.key_state().unwrap(), KeyState::Locked); assert!(d.keys.load().unwrap().is_none());
    assert!(!d.app.start().await.unwrap());
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap_err(), AppError::Locked);
    assert_eq!(d.app.item_text(&id).await, Err(AppError::Locked));
    assert_eq!(d.app.item_image(&id).await.unwrap_err(), AppError::Locked);
    assert_eq!(d.app.copy(&id).await, Err(AppError::Locked));
    assert_eq!(d.app.copy_items(&[id.clone()]).await, Err(AppError::Locked));
    assert_eq!(d.app.set_flags(&id, Some(true), None, None).await, Err(AppError::Locked));
    assert_eq!(d.app.delete(&id).await, Err(AppError::Locked));
    assert_eq!(d.app.devices().await.unwrap_err(), AppError::Locked);
    assert_eq!(d.app.change_server("invalid", None, None).await, Err(AppError::Locked));
    assert_eq!(d.app.show_kit(), Err(AppError::Locked));
    assert_eq!(d.app.sync_now().await, Err(AppError::Locked));
    assert_eq!(d.app.copy_secret("old kit".into()).await, Err(AppError::Locked));
    assert_eq!(d.app.save_image(&id, &d.dir.join("must-not-exist.png")).await, Err(AppError::Locked));
    assert!(!d.dir.join("must-not-exist.png").exists());
    assert_eq!(d.snapshot(), before);
    d.app.unlock_local(PASS).await.unwrap();
    assert_eq!(d.app.item_text(&id).await.unwrap(), "preserved private history");
    d.clock.advance(59); d.app.check_auto_lock().await.unwrap(); assert!(d.app.is_joined());
    d.clock.advance(1); d.app.check_auto_lock().await.unwrap(); assert!(!d.app.is_joined());
}

#[tokio::test]
async fn explicit_lock_with_auto_lock_off_cancels_deletion_and_preserves_clipboard_key_and_data() {
    let d = Fixture::new(); d.local().await;
    let id = d.add("keep after cancelled delete").await;
    let kit = d.app.show_kit().unwrap(); let key = d.keys.load().unwrap().unwrap();
    d.app.copy(&id).await.unwrap(); let clipboard = d.clip.current();
    let epoch = d.app.session_epoch();
    let before = d.snapshot(); let key_file = std::fs::read(d.dir.join(KEY_FILE)).unwrap();
    d.app.stage_deletion(&[id.clone()]).await.unwrap();
    d.app.lock_session().await.unwrap(); assert!(d.app.pending_deletions().pending.is_empty());
    d.clock.advance(11); tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(d.clip.current(), clipboard);
    assert_eq!(d.app.unlock_local("wrong passphrase").await, Err(AppError::WrongPassphrase));
    assert_eq!(d.snapshot(), before); assert_eq!(std::fs::read(d.dir.join(KEY_FILE)).unwrap(), key_file);
    // Cancellation issues no unlock request; tray, wake and late capture cannot reopen the session.
    d.app.notify_wake(); d.app.notify_network(true);
    d.app.on_clip_event(ClipEvent::Captured(Content::Text("late capture".into()))).await;
    assert!(!d.app.start().await.unwrap()); assert_eq!(d.snapshot(), before);
    d.app.unlock_local(PASS).await.unwrap(); assert_eq!(d.app.mode(), Mode::LocalOnly);
    assert_eq!(d.app.save_image_at(&id, &d.dir.join("stale.png"), epoch).await, Err(AppError::Locked));
    assert!(!d.dir.join("stale.png").exists());
    assert_eq!(d.keys.load().unwrap().unwrap(), key); assert_eq!(d.app.show_kit().unwrap(), kit);
    assert_eq!(d.app.item_text(&id).await.unwrap(), "keep after cancelled delete");
    d.clock.advance(86_400); d.app.check_auto_lock().await.unwrap(); assert!(d.app.is_joined());
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn locking_races_search_capture_and_start_without_reopening() {
    let d = Fixture::new(); d.local().await; let id = d.add("concurrent history").await;
    let app = d.app.clone();
    let work = tokio::spawn(async move { for _ in 0..25 { let _ = app.search("", Filter::All, 10, 0).await; app.on_clip_event(ClipEvent::Captured(Content::Text("racing capture".into()))).await; let _ = app.start().await; } });
    d.app.lock_session().await.unwrap(); work.await.unwrap();
    assert!(!d.app.is_joined()); assert!(d.keys.load().unwrap().is_none());
    let before = d.snapshot(); d.app.on_clip_event(ClipEvent::Captured(Content::Text("after lock".into()))).await;
    assert_eq!(d.snapshot(), before); d.app.unlock_local(PASS).await.unwrap(); assert!(d.app.item_text(&id).await.is_ok());
}

#[test]
fn interval_defaults_and_invalid_values_are_off_and_valid_choices_persist() {
    assert_eq!(serde_json::from_str::<Settings>("{}").unwrap().auto_lock_minutes, 0);
    for raw in ["null", "true", "-1", "2", "999", "\"five\""] {
        assert_eq!(serde_json::from_str::<Settings>(&format!("{{\"autoLockMinutes\":{raw}}}")).unwrap().auto_lock_minutes, 0);
    }
    let d = Fixture::new();
    for minutes in [0, 1, 5, 15, 30] { let mut settings = Settings::default(); settings.auto_lock_minutes = minutes; settings.save(&d.dir).unwrap(); assert_eq!(Settings::load(&d.dir).auto_lock_minutes, minutes); }
}

#[tokio::test]
async fn synced_lock_keeps_the_original_master_key_history_and_pending_outbox() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let mut cfg = onepaste_server::config::Config::new(Fixture::new().dir.clone());
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let server = onepaste_server::Server::start(cfg).await.unwrap();
    let mut code: serde_json::Value = serde_json::from_slice(&B64.decode(server.first_connect_code.as_ref().unwrap().strip_prefix("onepaste1:").unwrap()).unwrap()).unwrap();
    code["u"] = format!("https://127.0.0.1:{}", server.addr.port()).into();
    let code = format!("onepaste1:{}", B64.encode(serde_json::to_vec(&code).unwrap()));
    let d = Fixture::new(); d.app.prepare_local_passphrase(PASS).unwrap();
    let kit = d.app.create_vault(&code, "original server vault passphrase", "lock fixture").await.unwrap();
    d.app.confirm_kit_saved().await.unwrap();
    let key = d.keys.load().unwrap().unwrap(); let id = d.add("pending encrypted upload").await;
    let before = d.snapshot(); assert!(!before.1.is_empty());
    d.app.stage_deletion(&[id.clone()]).await.unwrap();
    let app = d.app.clone(); let sync = tokio::spawn(async move { let _ = app.sync_now().await; });
    d.app.lock_session().await.unwrap(); sync.await.unwrap();
    let locked = d.snapshot(); assert_eq!(locked, before); assert!(locked.0.iter().all(|row| !row.deleted));
    assert_eq!(d.app.key_state().unwrap(), KeyState::Locked);
    assert_eq!(d.app.unlock_local("wrong").await, Err(AppError::WrongPassphrase)); assert_eq!(d.snapshot(), locked);
    d.app.unlock_local(PASS).await.unwrap(); assert_eq!(d.app.mode(), Mode::Synced);
    assert_eq!(d.keys.load().unwrap().unwrap(), key); assert_eq!(d.app.show_kit().unwrap(), kit);
    assert_eq!(d.snapshot(), locked); assert_eq!(d.app.item_text(&id).await.unwrap(), "pending encrypted upload");
    d.app.sync_now().await.unwrap(); assert_eq!(d.app.status().await.pending, 0);
    d.app.lock_session().await.unwrap();
    // Reopen through the real background driver, clipboard callback and server event subscription as well.
    let keys = Arc::new(EncryptedKeyStore::new(d.dir.clone()));
    let background = App::new(d.dir.clone(), keys.clone(), d.clip.clone(), AppConfig { poll_interval: Duration::from_millis(50), ..AppConfig::default() });
    background.unlock_local(PASS).await.unwrap();
    d.clip.user_copies(Content::Text("background session fixture".into()));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop { if !background.search("background session fixture", Filter::All, 10, 0).await.unwrap().is_empty() { break; } tokio::task::yield_now().await; }
    }).await.unwrap();
    background.lock_session().await.unwrap(); assert!(background.status().await.locked);
    assert!(keys.load().unwrap().is_none()); let stopped = d.snapshot();
    background.notify_wake(); background.notify_network(true); d.clip.user_copies(Content::Text("must stay outside history".into()));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(d.snapshot(), stopped); assert!(!background.start().await.unwrap());
    background.shutdown(); let _ = d.clip.start_monitor(Box::new(|_| {}));
    server.shutdown().await;
}
