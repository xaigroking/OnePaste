use super::*;

#[tokio::test]
async fn a_fresh_start_needs_no_old_credentials_and_restarts_with_new_history() {
    for kind in [
        "missing-key",
        "corrupt-key",
        "locked",
        "empty-history",
        "sidecar",
        "key-directory",
    ] {
        let d = Fixture::new();
        let mut old_key = None;
        if matches!(kind, "missing-key" | "corrupt-key" | "locked") {
            d.local().await;
            old_key = d.store.load().unwrap();
            d.app.lock_session().await.unwrap();
            match kind {
                "missing-key" => std::fs::remove_file(d.dir.join(KEY_FILE)).unwrap(),
                "corrupt-key" => std::fs::write(d.dir.join(KEY_FILE), "invalid key file").unwrap(),
                _ => {}
            }
        } else {
            std::fs::create_dir_all(&d.dir).unwrap();
            match kind {
                "empty-history" => std::fs::write(d.dir.join("history.db"), []).unwrap(),
                "sidecar" => {
                    std::fs::write(d.dir.join("onepaste.db-wal"), "orphaned data").unwrap()
                }
                "key-directory" => std::fs::create_dir(d.dir.join(KEY_FILE)).unwrap(),
                _ => unreachable!(),
            }
        }
        let app = Fixture::at(d.dir.clone());
        let before = app.bytes();
        // Reading access state, retrying startup and failing recovery cannot start over implicitly.
        let _ = app.app.key_state();
        assert!(!app.app.start().await.unwrap());
        assert!(app.app.prepare_local_passphrase(NEXT).is_err());
        assert_eq!(app.bytes(), before, "{kind}");
        std::fs::write(d.dir.join("unrelated.txt"), "keep this file").unwrap();
        app.app.restart_local_setup().await.unwrap();
        assert_eq!(app.app.key_state().unwrap(), KeyState::New, "{kind}");
        assert_eq!(app.app.mode(), Mode::Unset);
        assert!(!app.app.is_joined());
        assert_eq!(
            std::fs::read_to_string(d.dir.join("unrelated.txt")).unwrap(),
            "keep this file"
        );
        let archive = std::fs::read_dir(&d.dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("previous-data-")
            })
            .unwrap();
        for (name, bytes) in before {
            assert_eq!(std::fs::read(archive.join(name)).unwrap(), bytes, "{kind}");
        }
        if let Some(old_key) = old_key {
            onepaste_history::HistoryStore::verify_existing(&archive, &old_key).unwrap();
        }
        app.app.prepare_local_passphrase(NEXT).unwrap();
        app.app.choose_local_only().await.unwrap();
        assert!(app
            .app
            .search("persistent", Filter::All, 10, 0)
            .await
            .unwrap()
            .is_empty());
        app.app
            .on_clip_event(ClipEvent::Captured(Content::Text(
                "new usable history".into(),
            )))
            .await;
        assert_eq!(
            app.app
                .search("new usable", Filter::All, 10, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        app.app.lock_session().await.unwrap();
        let again = Fixture::at(d.dir.clone());
        assert_eq!(again.app.key_state().unwrap(), KeyState::Locked);
        assert!(again.app.unlock_local(LOCAL).await.is_err());
        again.app.unlock_local(NEXT).await.unwrap();
        assert_eq!(again.app.mode(), Mode::LocalOnly);
        assert_eq!(
            again
                .app
                .search("new usable", Filter::All, 10, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        again.app.lock_session().await.unwrap();
    }
}

#[tokio::test]
async fn starting_over_closes_an_active_session_and_cancels_pending_deletions() {
    let d = Fixture::new();
    d.local().await;
    let id = d
        .app
        .search("persistent", Filter::All, 10, 0)
        .await
        .unwrap()[0]
        .item_id
        .clone();
    d.app.stage_deletion(&[id]).await.unwrap();
    d.app.restart_local_setup().await.unwrap();
    assert!(d.store.load().unwrap().is_none());
    assert!(d.app.pending_deletions().pending.is_empty());
    assert_eq!(d.app.key_state().unwrap(), KeyState::New);
    assert!(d
        .app
        .search("persistent", Filter::All, 10, 0)
        .await
        .is_err());
    d.app.prepare_local_passphrase(NEXT).unwrap();
    d.app.choose_local_only().await.unwrap();
    assert!(d
        .app
        .search("", Filter::All, 10, 0)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn starting_over_from_an_unused_directory_can_initialize_without_recovery() {
    let d = Fixture::new();
    d.app.restart_local_setup().await.unwrap();
    assert_eq!(d.app.key_state().unwrap(), KeyState::New);
    d.app.prepare_local_passphrase(NEXT).unwrap();
    d.app.choose_local_only().await.unwrap();
    assert!(d.app.is_joined());
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_key_file_and_a_failed_restart_do_not_require_old_credentials() {
    use std::os::unix::fs::PermissionsExt;
    let d = Fixture::new();
    d.local().await;
    d.app.lock_session().await.unwrap();
    let before = d.bytes();
    std::fs::set_permissions(d.dir.join(KEY_FILE), std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(d.app.unlock_local(LOCAL).await.is_err());
    std::fs::set_permissions(&d.dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    assert!(d.app.restart_local_setup().await.is_err());
    std::fs::set_permissions(&d.dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(d.dir.join(KEY_FILE), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(d.bytes(), before);
    // Retry succeeds without first unlocking the old key.
    d.app.restart_local_setup().await.unwrap();
    d.app.prepare_local_passphrase(NEXT).unwrap();
    d.app.choose_local_only().await.unwrap();
    assert!(d.app.is_joined());
}

#[tokio::test]
async fn starting_over_does_not_follow_a_key_file_symlink_or_touch_other_files() {
    let d = Fixture::new();
    std::fs::create_dir_all(&d.dir).unwrap();
    let external = Fixture::new();
    std::fs::create_dir_all(&external.dir).unwrap();
    let target = external.dir.join("unrelated-private-file");
    std::fs::write(&target, "must remain unchanged").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, d.dir.join(KEY_FILE)).unwrap();
    #[cfg(windows)]
    {
        // Creating symlinks requires privileges on Windows. Directory corruption exercises the same rename path.
        std::fs::create_dir(d.dir.join(KEY_FILE)).unwrap();
    }
    d.app.restart_local_setup().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "must remain unchanged"
    );
    d.app.prepare_local_passphrase(NEXT).unwrap();
    d.app.choose_local_only().await.unwrap();
    assert!(d.app.is_joined());
}

#[tokio::test]
async fn a_fresh_start_can_create_a_new_synced_vault_without_old_credentials() {
    use onepaste_server::{config::Config, store::Store, Server};
    async fn server() -> (Server, String, PathBuf) {
        let dir = Fixture::new().dir.clone();
        let mut cfg = Config::new(dir.clone());
        cfg.listen = "127.0.0.1:0".parse().unwrap();
        let server = Server::start(cfg).await.unwrap();
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
        (server, code, dir)
    }
    let (old_server, old_code, old_dir) = server().await;
    let (new_server, new_code, new_dir) = server().await;
    let d = Fixture::new();
    d.app.prepare_local_passphrase(LOCAL).unwrap();
    d.app
        .create_vault(&old_code, "old independent vault password", "old computer")
        .await
        .unwrap();
    d.app.confirm_kit_saved().await.unwrap();
    d.app
        .on_clip_event(ClipEvent::Captured(Content::Text(
            "old remote history".into(),
        )))
        .await;
    d.app.sync_now().await.unwrap();
    let peer = Fixture::new();
    peer.app.prepare_local_passphrase(LOCAL).unwrap();
    peer.app
        .join(
            &d.app.new_connection_code().await.unwrap(),
            "old independent vault password",
            "unchanged peer",
        )
        .await
        .unwrap();
    peer.app.sync_now().await.unwrap();
    assert_eq!(
        peer.app
            .search("old remote", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(Store::open(&old_dir).unwrap().device_count().unwrap(), 2);
    d.app.lock_session().await.unwrap();
    // The previous unlock record is unreadable, and no old credential is supplied to restart.
    std::fs::write(d.dir.join(KEY_FILE), "corrupt previous key").unwrap();
    d.app.restart_local_setup().await.unwrap();
    d.app.prepare_local_passphrase(NEXT).unwrap();
    d.app
        .create_vault(
            &new_code,
            "new independent vault password",
            "fresh computer",
        )
        .await
        .unwrap();
    // Continue without saving or confirming possession of the returned recovery kit.
    d.app.confirm_kit_saved().await.unwrap();
    assert_eq!(d.app.mode(), Mode::Synced);
    assert!(d
        .app
        .search("old remote", Filter::All, 10, 0)
        .await
        .unwrap()
        .is_empty());
    d.app
        .on_clip_event(ClipEvent::Captured(Content::Text(
            "fresh synced history".into(),
        )))
        .await;
    d.app.sync_now().await.unwrap();
    peer.app.sync_now().await.unwrap();
    assert_eq!(
        peer.app
            .search("old remote", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(peer
        .app
        .search("fresh synced", Filter::All, 10, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(Store::open(&old_dir).unwrap().device_count().unwrap(), 2);
    assert_eq!(Store::open(&new_dir).unwrap().device_count().unwrap(), 1);
    d.app.lock_session().await.unwrap();
    let again = Fixture::at(d.dir.clone());
    again.app.unlock_local(NEXT).await.unwrap();
    assert_eq!(again.app.mode(), Mode::Synced);
    assert_eq!(
        again
            .app
            .search("fresh synced", Filter::All, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    again.app.lock_session().await.unwrap();
    peer.app.lock_session().await.unwrap();
    old_server.shutdown().await;
    new_server.shutdown().await;
}
