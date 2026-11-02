use onepaste_app::*;
use std::sync::Arc;

#[tokio::test]
async fn captured_url_yaml_and_toml_have_real_history_types_and_keep_the_original_text() {
    let dir = std::env::temp_dir().join(format!("onepaste-classify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let clip = Arc::new(FakeClipboard::new());
    let app = App::new(
        dir.clone(),
        Arc::new(MemoryKeyStore::new()),
        clip.clone(),
        AppConfig::default(),
    );
    app.choose_local_only().await.unwrap();
    for (text, kind) in [
        ("  https://example.org/path?q=hello#part\r\n", "link"),
        (include_str!("../../../redocly.yaml"), "code"),
        (include_str!("../../../Cargo.toml"), "code"),
        ("Read https://example.org tomorrow, please.", "text"),
        ("Meeting: tomorrow\nBring your notes.", "text"),
    ] {
        app.on_clip_event(ClipEvent::Captured(Content::Text(text.into())))
            .await;
        let rows = app.search("", Filter::All, 30, 0).await.unwrap();
        let row = rows
            .iter()
            .find(|r| r.preview == text.lines().next().unwrap_or(""))
            .unwrap();
        assert_eq!(row.kind, kind, "classification differs for {text:?}");
        assert_eq!(app.item_text(&row.item_id).await.unwrap(), text);
        app.copy(&row.item_id).await.unwrap();
        assert_eq!(clip.current().unwrap(), Content::Text(text.into()));
    }
    app.shutdown();
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unknown_filter_does_not_silently_select_all_history() {
    assert_eq!(
        parse_filter("unsupported-future-type").unwrap_err(),
        AppError::InvalidFilter
    );
}

#[tokio::test]
async fn explicit_correction_survives_encrypted_restart_and_unrelated_flags() {
    let dir = std::env::temp_dir().join(format!(
        "onepaste-classify-persist-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    let keys = Arc::new(EncryptedKeyStore::new(dir.clone()));
    let clip = Arc::new(FakeClipboard::new());
    let config = AppConfig {
        background_sync: false,
        listen_for_changes: false,
        ..AppConfig::default()
    };
    let app = App::new(dir.clone(), keys.clone(), clip.clone(), config.clone());
    app.prepare_local_passphrase("classification passphrase")
        .unwrap();
    app.choose_local_only().await.unwrap();
    let original = "\t https://example.org/correction-private-probe \r\n";
    app.on_clip_event(ClipEvent::Captured(Content::Text(original.into())))
        .await;
    let id = app.search("", Filter::Links, 10, 0).await.unwrap()[0]
        .item_id
        .clone();
    app.set_text_classification(&id, "code", Some("yaml"))
        .await
        .unwrap();
    app.set_flags(&id, Some(true), Some(true), Some("kept note".into()))
        .await
        .unwrap();
    assert!(app
        .search("", Filter::Links, 10, 0)
        .await
        .unwrap()
        .is_empty());
    assert!(app
        .set_text_classification(&id, "unknown", None)
        .await
        .is_err());
    app.shutdown();
    drop(app);
    drop(keys);
    let app = App::new(
        dir.clone(),
        Arc::new(EncryptedKeyStore::new(dir.clone())),
        clip.clone(),
        config,
    );
    app.unlock_local("classification passphrase").await.unwrap();
    app.start().await.unwrap();
    let rows = app.search("", Filter::Code, 10, 0).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].language.as_deref(), Some("yaml"));
    assert!(rows[0].favorite && rows[0].pinned);
    assert_eq!(rows[0].note, "kept note");
    app.copy(&id).await.unwrap();
    assert_eq!(clip.current(), Some(Content::Text(original.into())));
    assert!(!app.diagnostics().await.contains("correction-private-probe"));
    for file in std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|f| f.path().is_file())
    {
        let data = std::fs::read(file.path()).unwrap();
        assert!(!data
            .windows(b"correction-private-probe".len())
            .any(|v| v == b"correction-private-probe"));
    }
    app.shutdown();
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}
