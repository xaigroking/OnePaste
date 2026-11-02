use onepaste_app::*;
use std::sync::Arc;

#[tokio::test]
async fn history_dto_exposes_the_actual_saved_tag_collection() {
    let dir = std::env::temp_dir().join(format!(
        "onepaste-tags-dto-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    let app = App::new(
        dir.clone(),
        Arc::new(MemoryKeyStore::new()),
        Arc::new(FakeClipboard::new()),
        AppConfig::default(),
    );
    app.choose_local_only().await.unwrap();
    app.on_clip_event(ClipEvent::Captured(Content::Text("taggable sample".into())))
        .await;
    let rows = app.search("", Filter::All, 10, 0).await.unwrap();
    let value = serde_json::to_value(&rows[0]).unwrap();
    app.shutdown();
    assert_eq!(value["tags"], serde_json::json!([]));
    drop(app);
    let _ = std::fs::remove_dir_all(dir);
}

fn quiet() -> AppConfig {
    AppConfig {
        background_sync: false,
        listen_for_changes: false,
        ..AppConfig::default()
    }
}

#[tokio::test]
async fn tags_persist_in_encrypted_history_and_combine_with_type_text_and_pagination() {
    let dir = std::env::temp_dir().join(format!(
        "onepaste-tags-persist-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    let keys = Arc::new(EncryptedKeyStore::new(dir.clone()));
    let app = App::new(
        dir.clone(),
        keys.clone(),
        Arc::new(FakeClipboard::new()),
        quiet(),
    );
    app.prepare_local_passphrase("tags independent passphrase")
        .unwrap();
    app.choose_local_only().await.unwrap();
    for i in 0..7 {
        let text = format!("[project]\nname = \"tag-sample-{i}\"");
        app.on_clip_event(ClipEvent::Captured(Content::Text(text)))
            .await;
    }
    let rows = app.search("", Filter::Code, 20, 0).await.unwrap();
    assert_eq!(rows.len(), 7);
    for row in &rows {
        app.set_tags(
            &row.item_id,
            &[
                "  TagPrivacyProbe ".into(),
                "tagprivacyprobe".into(),
                "École".into(),
            ],
        )
        .await
        .unwrap();
    }
    app.set_flags(
        &rows[0].item_id,
        Some(true),
        Some(true),
        Some("independent note".into()),
    )
    .await
    .unwrap();
    assert!(app
        .set_tags(&rows[0].item_id, &["x".repeat(65)])
        .await
        .is_err());
    let first = app
        .search_with_tag("project", Filter::Code, Some(" tagprivacyprobe "), 3, 0)
        .await
        .unwrap();
    let next = app
        .search_with_tag("project", Filter::Code, Some("TagPrivacyProbe"), 3, 3)
        .await
        .unwrap();
    assert_eq!((first.len(), next.len()), (3, 3));
    assert!(first
        .iter()
        .all(|a| next.iter().all(|b| a.item_id != b.item_id)));
    assert!(app
        .search_with_tag("", Filter::Links, Some("TagPrivacyProbe"), 10, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(app.tag_options("é").await.unwrap()[0].count, 7);
    assert_eq!(
        app.search("TagPrivacyProbe", Filter::All, 20, 0)
            .await
            .unwrap()
            .len(),
        7
    );
    app.shutdown();
    drop(app);
    drop(keys);
    let app = App::new(
        dir.clone(),
        Arc::new(EncryptedKeyStore::new(dir.clone())),
        Arc::new(FakeClipboard::new()),
        quiet(),
    );
    app.unlock_local("tags independent passphrase")
        .await
        .unwrap();
    let rows = app
        .search_with_tag("", Filter::Favorites, Some("école"), 20, 0)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tags, vec!["TagPrivacyProbe", "École"]);
    assert!(rows[0].pinned);
    assert_eq!(rows[0].note, "independent note");
    app.set_tags(&rows[0].item_id, &[]).await.unwrap();
    assert_eq!(
        app.tag_options("TagPrivacyProbe").await.unwrap()[0].count,
        6
    );
    assert!(!app.diagnostics().await.contains("TagPrivacyProbe"));
    app.shutdown();
    drop(app);
    for file in std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
    {
        let data = std::fs::read(file.path()).unwrap();
        assert!(!data
            .windows(b"TagPrivacyProbe".len())
            .any(|v| v == b"TagPrivacyProbe"));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn tags_and_type_metadata_converge_through_real_https_without_leaking_tag_names_to_the_server(
) {
    let dir = std::env::temp_dir().join(format!(
        "onepaste-tags-sync-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = onepaste_server::config::Config::new(dir.join("server"));
    config.listen = format!("127.0.0.1:{port}").parse().unwrap();
    config.public_url = Some(format!("https://127.0.0.1:{port}"));
    let server = onepaste_server::Server::start(config).await.unwrap();
    let make = |name: &str| {
        let path = dir.join(name);
        App::new(
            path.clone(),
            Arc::new(EncryptedKeyStore::new(path)),
            Arc::new(FakeClipboard::new()),
            quiet(),
        )
    };
    let a = make("a");
    a.prepare_local_passphrase("first independent passphrase")
        .unwrap();
    a.create_vault(
        server.first_connect_code.as_deref().unwrap(),
        "independent vault passphrase",
        "first",
    )
    .await
    .unwrap();
    a.confirm_kit_saved().await.unwrap();
    let b = make("b");
    b.prepare_local_passphrase("second independent passphrase")
        .unwrap();
    b.join(
        &a.new_connection_code().await.unwrap(),
        "independent vault passphrase",
        "second",
    )
    .await
    .unwrap();
    a.on_clip_event(ClipEvent::Captured(Content::Text(
        "[project]\nname = \"tag-network-sample\"".into(),
    )))
    .await;
    a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    let id = b.search("", Filter::Code, 10, 0).await.unwrap()[0]
        .item_id
        .clone();
    a.set_flags(&id, Some(true), None, Some("separate note".into()))
        .await
        .unwrap();
    b.set_tags(&id, &["  TagNetworkPrivacyProbe  ".into(), "Work".into()])
        .await
        .unwrap();
    a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    let rows = a
        .search_with_tag(
            "project",
            Filter::Code,
            Some("tagnetworkprivacyprobe"),
            10,
            0,
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].favorite);
    assert_eq!(rows[0].note, "separate note");
    assert_eq!(rows[0].language.as_deref(), Some("toml"));
    {
        let store = onepaste_server::store::Store::open(&dir.join("server")).unwrap();
        let page = store
            .list_items(&a.status().await.device_id, 0, 200)
            .unwrap();
        let public = &page["items"][0]["envelope"];
        assert!(!public.to_string().contains("TagNetworkPrivacyProbe"));
        assert!(public.get("tags").is_none());
    }
    a.set_tags(&id, &[]).await.unwrap();
    b.set_text_classification(&id, "text", None).await.unwrap();
    a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    let rows = a.search("", Filter::Text, 10, 0).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].tags.is_empty());
    assert!(rows[0].favorite);
    a.shutdown();
    b.shutdown();
    drop(a);
    drop(b);
    server.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
