//! Real production key files, encrypted databases and HTTPS sessions; no system credential provider is linked.
use onepaste_app::*;
use onepaste_core::{crypto::Key, recovery};
use std::{path::PathBuf, sync::Arc};

const LOCAL: &str = "independent-local-passphrase-42";
const NEXT: &str = "replacement-local-passphrase-43";
struct QuietClipboard;
impl ClipboardPort for QuietClipboard {
    fn write(&self, _: &Content, _: bool) -> Result<(), AppError> {
        Ok(())
    }
    fn start_monitor(
        &self,
        _: Box<dyn Fn(ClipEvent) + Send + Sync>,
    ) -> Result<MonitorGuard, AppError> {
        Ok(MonitorGuard::new(()))
    }
}
struct Fixture {
    app: App,
    store: Arc<EncryptedKeyStore>,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "onepaste-key-access-{}",
            onepaste_core::id::new_id().unwrap()
        ));
        Self::at(dir)
    }
    fn at(dir: PathBuf) -> Self {
        let store = Arc::new(EncryptedKeyStore::new(dir.clone()));
        let app = App::new(
            dir.clone(),
            store.clone(),
            Arc::new(QuietClipboard),
            AppConfig {
                background_sync: false,
                listen_for_changes: false,
                ..AppConfig::default()
            },
        );
        Self { app, store, dir }
    }
    async fn local(&self) {
        self.app.prepare_local_passphrase(LOCAL).unwrap();
        self.app.choose_local_only().await.unwrap();
        self.app
            .on_clip_event(ClipEvent::Captured(Content::Text(
                "persistent private fixture content".into(),
            )))
            .await;
    }
    fn bytes(&self) -> Vec<(String, Vec<u8>)> {
        ["onepaste.db", "history.db", "app-state.json", KEY_FILE]
            .iter()
            .filter_map(|name| {
                std::fs::read(self.dir.join(name))
                    .ok()
                    .map(|b| (name.to_string(), b))
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.app.shutdown();
    }
}

#[tokio::test]
async fn production_store_requires_explicit_setup_and_every_new_process_starts_locked() {
    let d = Fixture::new();
    assert_eq!(d.app.key_state().unwrap(), KeyState::New);
    assert!(!d.app.start().await.unwrap());
    assert!(!d.dir.exists());
    assert_eq!(
        d.app.choose_local_only().await.unwrap_err(),
        AppError::Locked
    );
    assert!(!d.dir.exists());
    d.local().await;
    let kit = d.app.show_kit().unwrap();
    let key = d.store.load().unwrap().unwrap();
    let raw = std::fs::read(d.dir.join(KEY_FILE)).unwrap();
    assert!(!raw.windows(LOCAL.len()).any(|w| w == LOCAL.as_bytes()));
    assert!(!raw.windows(32).any(|w| w == key.as_bytes()));
    let directory = d.dir.clone();
    drop(d);
    let next = Fixture::at(directory);
    let before = next.bytes();
    assert_eq!(next.app.key_state().unwrap(), KeyState::Locked);
    assert!(!next.app.start().await.unwrap());
    assert_eq!(
        next.app.unlock_local("wrong password").await.unwrap_err(),
        AppError::WrongPassphrase
    );
    assert_eq!(next.bytes(), before);
    assert!(!next.app.is_joined());
    next.app.unlock_local(LOCAL).await.unwrap();
    assert_eq!(next.app.mode(), Mode::LocalOnly);
    assert_eq!(
        next.app
            .search("persistent", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(next.app.show_kit().unwrap(), kit);
    assert_eq!(next.store.load().unwrap().unwrap(), key);
}

#[tokio::test]
async fn legacy_files_without_a_key_are_preserved_by_every_setup_entry() {
    let d = Fixture::new();
    d.local().await;
    let before = d.bytes();
    d.app.shutdown();
    std::fs::remove_file(d.dir.join(KEY_FILE)).unwrap();
    let old = Fixture::at(d.dir.clone());
    let protected = old.bytes();
    assert_eq!(old.app.key_state().unwrap(), KeyState::Recovery);
    assert!(!old.app.start().await.unwrap());
    assert!(old.app.prepare_local_passphrase(NEXT).is_err());
    assert_eq!(
        old.app.choose_local_only().await.unwrap_err(),
        AppError::RecoveryRequired
    );
    assert_eq!(
        old.app.join("invalid", LOCAL, "device").await.unwrap_err(),
        AppError::RecoveryRequired
    );
    assert_eq!(
        old.app
            .join_with_kit("invalid", "invalid", "device")
            .await
            .unwrap_err(),
        AppError::RecoveryRequired
    );
    assert_eq!(
        old.app
            .create_vault("invalid", LOCAL, "device")
            .await
            .unwrap_err(),
        AppError::RecoveryRequired
    );
    assert_eq!(old.bytes(), protected);
    assert!(before.len() > protected.len());
}

#[tokio::test]
async fn explicit_kit_recovery_validates_existing_ciphertext_and_keeps_the_same_master_key() {
    let d = Fixture::new();
    d.local().await;
    let kit = d.app.show_kit().unwrap();
    d.app.shutdown();
    std::fs::remove_file(d.dir.join(KEY_FILE)).unwrap();
    let next = Fixture::at(d.dir.clone());
    let before = next.bytes();
    let wrong = recovery::encode(&Key::random().unwrap());
    assert_eq!(
        next.app
            .recover_local(&wrong, true, NEXT)
            .await
            .unwrap_err(),
        AppError::WrongRecoveryKit
    );
    assert_eq!(next.bytes(), before);
    assert!(!next.dir.join(KEY_FILE).exists());
    next.app.recover_local(&kit, true, NEXT).await.unwrap();
    assert_eq!(next.app.show_kit().unwrap(), kit);
    assert_eq!(
        next.app
            .search("persistent", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    let directory = next.dir.clone();
    drop(next);
    drop(d);
    let final_app = Fixture::at(directory);
    assert!(final_app.app.unlock_local(LOCAL).await.is_err());
    final_app.app.unlock_local(NEXT).await.unwrap();
    assert_eq!(final_app.app.show_kit().unwrap(), kit);
    final_app.app.wipe_local_data().await.unwrap();
    for name in ["onepaste.db", "history.db", KEY_FILE, "app-state.json"] {
        assert!(!final_app.dir.join(name).exists(), "{name}");
    }
    assert_eq!(final_app.app.key_state().unwrap(), KeyState::New);
}

#[tokio::test]
async fn corrupted_or_unknown_key_record_never_falls_back_to_new_setup() {
    let d = Fixture::new();
    d.local().await;
    d.app.shutdown();
    for value in [
        "not json",
        r#"{"v":99,"salt":"x","kdf":{"m_kib":65536,"t":3,"p":1},"sealed_key":"x"}"#,
    ] {
        std::fs::write(d.dir.join(KEY_FILE), value).unwrap();
        let next = Fixture::at(d.dir.clone());
        let before = next.bytes();
        assert_eq!(next.app.key_state().unwrap(), KeyState::Locked);
        assert!(next.app.unlock_local(LOCAL).await.is_err());
        assert!(next.app.choose_local_only().await.is_err());
        assert_eq!(next.bytes(), before);
    }
}

#[tokio::test]
async fn synced_create_join_restart_recovery_and_cloud_passphrase_change_use_only_owned_files() {
    use onepaste_server::{config::Config, Server};
    let server_dir = Fixture::new().dir.clone();
    let mut cfg = Config::new(server_dir);
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    let server = Server::start(cfg).await.unwrap();
    // Use the actual chosen port in connection codes, without replacing its pinned certificate or token.
    let mut code: serde_json::Value = serde_json::from_slice(
        &base64::Engine::decode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            server
                .first_connect_code
                .as_ref()
                .unwrap()
                .strip_prefix("onepaste1:")
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    code["u"] = format!("https://127.0.0.1:{}", server.addr.port()).into();
    let code = format!(
        "onepaste1:{}",
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            serde_json::to_vec(&code).unwrap()
        )
    );
    let a = Fixture::new();
    a.app.prepare_local_passphrase(LOCAL).unwrap();
    let kit = a
        .app
        .create_vault(&code, "a separate vault passphrase", "first")
        .await
        .unwrap();
    a.app.confirm_kit_saved().await.unwrap();
    a.app
        .on_clip_event(ClipEvent::Captured(Content::Text(
            "synced protected item".into(),
        )))
        .await;
    a.app.sync_now().await.unwrap();
    let b = Fixture::new();
    b.app.prepare_local_passphrase(NEXT).unwrap();
    b.app
        .join_with_kit(&a.app.new_connection_code().await.unwrap(), &kit, "second")
        .await
        .unwrap();
    b.app.sync_now().await.unwrap();
    assert_eq!(
        b.app
            .search("protected", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    a.app
        .change_passphrase("a separate vault passphrase", "changed vault passphrase")
        .await
        .unwrap();
    a.app.shutdown();
    let again = Fixture::at(a.dir.clone());
    again.app.unlock_local(LOCAL).await.unwrap();
    assert_eq!(again.app.show_kit().unwrap(), kit);
    again.app.shutdown();
    std::fs::remove_file(a.dir.join(KEY_FILE)).unwrap();
    let recovery = Fixture::at(a.dir.clone());
    recovery
        .app
        .recover_local("changed vault passphrase", false, NEXT)
        .await
        .unwrap();
    assert_eq!(recovery.app.show_kit().unwrap(), kit);
    assert_eq!(
        recovery
            .app
            .search("protected", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    b.app.shutdown();
    recovery.app.shutdown();
    server.shutdown().await;
}

#[tokio::test]
async fn an_empty_or_truncated_history_file_cannot_validate_a_recovery_key() {
    let d = Fixture::new();
    std::fs::create_dir_all(&d.dir).unwrap();
    std::fs::write(d.dir.join("history.db"), []).unwrap();
    std::fs::write(d.dir.join("app-state.json"), br#"{"local_only":true}"#).unwrap();
    let before = d.bytes();
    let kit = recovery::encode(&Key::random().unwrap());
    assert_eq!(d.app.key_state().unwrap(), KeyState::Recovery);
    assert_eq!(
        d.app.recover_local(&kit, true, NEXT).await.unwrap_err(),
        AppError::WrongRecoveryKit
    );
    assert_eq!(d.bytes(), before);
    assert!(!d.dir.join(KEY_FILE).exists());
}

#[test]
fn concurrent_initial_key_saves_cannot_replace_each_other() {
    let d = Fixture::new();
    let a = Arc::new(EncryptedKeyStore::new(d.dir.clone()));
    let b = Arc::new(EncryptedKeyStore::new(d.dir.clone()));
    a.prepare(LOCAL).unwrap();
    b.prepare(NEXT).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = [a.clone(), b.clone()]
        .into_iter()
        .map(|store| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let key = Key::random().unwrap();
                barrier.wait();
                store.save(&key).is_ok()
            })
        })
        .collect();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(outcomes.iter().filter(|&&ok| ok).count(), 1);
    let check = EncryptedKeyStore::new(d.dir.clone());
    if outcomes[0] {
        check.unlock(LOCAL).unwrap();
        assert_eq!(check.load().unwrap(), a.load().unwrap());
    } else {
        check.unlock(NEXT).unwrap();
        assert_eq!(check.load().unwrap(), b.load().unwrap());
    }
}
