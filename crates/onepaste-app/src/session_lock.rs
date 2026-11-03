//! Closing and opening share the exclusive connection gate; sensitive commands hold its read side.
use super::*;
impl App {
    pub(super) fn session_opened(&self) {
        *lock(&self.0.idle) = self.0.cfg.auto_lock_clock.now();
        let mut timer = lock(&self.0.lock_timer);
        if timer.as_ref().is_some_and(|task| !task.is_finished()) { return; }
        let weak = Arc::downgrade(&self.0);
        *timer = Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(inner) = weak.upgrade() else { return; };
                let app = App(inner);
                let _ = app.check_auto_lock().await;
            }
        }));
    }

    pub(crate) fn expired(&self) -> bool {
        let minutes = self.settings().auto_lock_minutes;
        minutes != 0 && self.0.cfg.auto_lock_clock.now().saturating_sub(*lock(&self.0.idle)) >= Duration::from_secs(u64::from(minutes) * 60)
    }

    pub(crate) fn require_access(&self) -> Result<(), AppError> {
        if self.0.locked.load(std::sync::atomic::Ordering::Acquire) { return Err(AppError::Locked); }
        if self.expired() || !self.is_joined() {
            return Err(if matches!(self.key_state()?, crate::KeyState::Locked) || self.expired() { AppError::Locked } else { AppError::NotJoined });
        }
        Ok(())
    }

    /// A request queued before a lock cannot cross into a later unlocked session.
    pub(crate) async fn session_access(&self) -> Result<tokio::sync::RwLockReadGuard<'_, ()>, AppError> {
        let generation = self.0.generation.load(std::sync::atomic::Ordering::Acquire);
        let guard = self.0.connection.read().await;
        if generation != self.0.generation.load(std::sync::atomic::Ordering::Acquire) { return Err(AppError::Locked); }
        self.require_access()?;
        Ok(guard)
    }

    pub(crate) async fn session_write_access(&self) -> Result<tokio::sync::RwLockWriteGuard<'_, ()>, AppError> {
        let generation = self.0.generation.load(std::sync::atomic::Ordering::Acquire);
        let guard = self.0.connection.write().await;
        if generation != self.0.generation.load(std::sync::atomic::Ordering::Acquire) { return Err(AppError::Locked); }
        self.require_access()?;
        Ok(guard)
    }

    pub fn session_epoch(&self) -> u64 { self.0.generation.load(std::sync::atomic::Ordering::Acquire) }

    pub async fn save_image(&self, id: &str, path: &std::path::Path) -> Result<(), AppError> {
        self.save_image_at(id, path, self.session_epoch()).await
    }

    pub async fn save_image_at(&self, id: &str, path: &std::path::Path, epoch: u64) -> Result<(), AppError> {
        let _guard = self.session_access().await?;
        if epoch != self.session_epoch() { return Err(AppError::Locked); }
        let image = self.image_unlocked(id).await?;
        let bytes = B64.decode(image.data).map_err(|_| AppError::NotAnImage)?;
        std::fs::write(path, bytes).map_err(|e| AppError::Storage(e.to_string()))
    }

    pub async fn copy_secret(&self, text: String) -> Result<(), AppError> {
        let _guard = self.0.connection.read().await;
        if self.expired() || matches!(self.key_state()?, crate::KeyState::Locked | crate::KeyState::Recovery) { return Err(AppError::Locked); }
        self.0.clipboard.write(&Content::Text(text), true)
    }

    /// Only trusted keyboard or pointer interaction in a OnePaste window renews the deadline.
    pub async fn user_activity(&self) -> Result<(), AppError> {
        {
            // Record arrival before waiting for any in-flight network request. The timer decides under the same mutex.
            let shared = lock(&self.0.shared);
            if !shared.joined || self.0.locked.load(std::sync::atomic::Ordering::Acquire) { return Err(AppError::Locked); }
            let now = self.0.cfg.auto_lock_clock.now();
            let mut idle = lock(&self.0.idle);
            let minutes = shared.settings.auto_lock_minutes;
            if minutes == 0 || now.saturating_sub(*idle) < Duration::from_secs(u64::from(minutes) * 60) {
                *idle = now;
                return Ok(());
            }
        }
        self.check_auto_lock().await?;
        Err(AppError::Locked)
    }

    pub async fn check_auto_lock(&self) -> Result<(), AppError> {
        // Hide stale window content even if closing must wait for an already-running command.
        if self.is_joined() && self.expired() { self.cancel_attachment_transfers(); self.notify_status(); }
        let _connection = self.0.connection.write().await;
        let due = {
            let mut shared = lock(&self.0.shared);
            let minutes = shared.settings.auto_lock_minutes;
            let due = shared.joined && minutes != 0 && self.0.cfg.auto_lock_clock.now().saturating_sub(*lock(&self.0.idle)) >= Duration::from_secs(u64::from(minutes) * 60);
            if due { self.0.locked.store(true, std::sync::atomic::Ordering::Release); shared.joined = false; }
            due
        };
        if due { self.close_session().await; }
        Ok(())
    }

    pub async fn lock_session(&self) -> Result<(), AppError> {
        self.0.locked.store(true, std::sync::atomic::Ordering::Release);
        self.cancel_attachment_transfers(); self.notify_status();
        let _connection = self.0.connection.write().await;
        self.close_session().await;
        Ok(())
    }

    pub(super) async fn close_session(&self) {
        self.cancel_attachment_transfers();
        self.0.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.0.locked.store(true, std::sync::atomic::Ordering::Release);
        // Prevent monitor callbacks from registering more tasks before dropping their guard.
        { let mut shared = lock(&self.0.shared); shared.joined = false; shared.started = false; shared.last_fp = None; }
        self.notify_status();
        self.stop_capture_monitor();
        lock(&self.0.deletions).stop();
        let sync = lock(&self.0.sync_task).take();
        if let Some(task) = sync { task.abort(); let _ = task.await; }
        // The driver can register a server event subscription until it has exited. Drain after joining it.
        let tasks: Vec<_> = lock(&self.0.tasks).drain(..).collect();
        for task in &tasks { task.abort(); }
        for task in tasks { let _ = task.await; }
        lock(&self.0.attachment_transfers).clear();
        *self.0.session.lock().await = None;
        *self.0.local.lock().await = None;
        *lock(&self.0.history) = None;
        *lock(&self.0.pending) = None;
        self.0.secrets.clear_session();
        self.notify_status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Clock(AtomicU64);
    impl crate::AutoLockClock for Clock {
        fn now(&self) -> Duration { Duration::from_secs(self.0.load(Ordering::SeqCst)) }
    }
    #[tokio::test]
    async fn interaction_arrival_is_recorded_without_waiting_for_the_network_gate() {
        let clock = Arc::new(Clock(AtomicU64::new(0)));
        let dir = std::env::temp_dir().join(format!("onepaste-lock-input-{}", onepaste_core::id::new_id().unwrap()));
        let app = App::new(dir, Arc::new(crate::MemoryKeyStore::new()), Arc::new(crate::FakeClipboard::new()), AppConfig { auto_lock_clock: clock.clone(), ..AppConfig::default() });
        app.choose_local_only().await.unwrap();
        let mut settings = app.settings(); settings.auto_lock_minutes = 1; app.set_settings(settings).unwrap();
        // This is the same real gate held by an in-flight sync round, without an unreliable network delay.
        let network = app.0.connection.read().await;
        clock.0.store(59, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_millis(100), app.user_activity()).await.unwrap().unwrap();
        drop(network);
        clock.0.store(118, Ordering::SeqCst); app.check_auto_lock().await.unwrap(); assert!(app.is_joined());
        clock.0.store(119, Ordering::SeqCst); app.check_auto_lock().await.unwrap(); assert!(!app.is_joined());
        app.shutdown();
    }
}
