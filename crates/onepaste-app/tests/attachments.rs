use onepaste_app::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn quiet() -> AppConfig {
    AppConfig {
        background_sync: false,
        listen_for_changes: false,
        ..AppConfig::default()
    }
}
fn directory() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "onepaste-file-tests-{}",
        onepaste_core::id::new_id().unwrap()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn app(path: &Path) -> App {
    App::new(
        path.to_owned(),
        Arc::new(EncryptedKeyStore::new(path.to_owned())),
        Arc::new(FakeClipboard::new()),
        quiet(),
    )
}
fn probe(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|value| value == needle)
}
fn scan(path: &Path) {
    for entry in std::fs::read_dir(path).unwrap().flatten() {
        if entry.path().is_dir() {
            scan(&entry.path());
        } else {
            let data = std::fs::read(entry.path()).unwrap();
            for marker in [
                b"PRIVATE-CONTENT-PROBE".as_slice(),
                b"PRIVATE-NAME-PROBE".as_slice(),
            ] {
                assert!(
                    !probe(&data, marker),
                    "plaintext reached an application-owned file"
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn real_file_bytes_are_encrypted_locally_and_exported_exactly_without_network() {
    let _serial = SERIAL.lock().await;
    let root = directory();
    let path = root.join("PRIVATE-NAME-PROBE.bin");
    let mut bytes = b"PRIVATE-CONTENT-PROBE".to_vec();
    bytes.extend((0..700_000).map(|i| (i % 256) as u8));
    std::fs::write(&path, &bytes).unwrap();
    let core = app(&root.join("client"));
    core.prepare_local_passphrase("independent file passphrase")
        .unwrap();
    core.choose_local_only().await.unwrap();
    let before = onepaste_client::requests_started();
    let id = core.import_file(&path).await.unwrap();
    let rows = core
        .search("PRIVATE-NAME", Filter::Files, 20, 0)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].size, bytes.len() as i64);
    assert!(core.attachment_info(&id).await.unwrap().cached);
    assert_eq!(core.attachment_bytes(&id).await.unwrap(), bytes);
    let output = root.join("export.bin");
    core.save_attachment_at(&id, &output, core.session_epoch())
        .await
        .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), bytes);
    assert_eq!(onepaste_client::requests_started(), before);
    scan(&root.join("client"));
    assert!(!core.diagnostics().await.contains("PRIVATE-NAME-PROBE"));
    core.lock_session().await.unwrap();
    assert_eq!(
        core.attachment_bytes(&id).await.unwrap_err(),
        AppError::Locked
    );
    core.unlock_local("independent file passphrase")
        .await
        .unwrap();
    assert_eq!(core.attachment_bytes(&id).await.unwrap(), bytes);
    core.shutdown();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread")]
async fn files_upload_and_download_through_real_https_and_remain_available_after_restart_offline() {
    let _serial = SERIAL.lock().await;
    let root = directory();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut cfg = onepaste_server::config::Config::new(root.join("server"));
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
    cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
    let server = onepaste_server::Server::start(cfg).await.unwrap();
    let a = app(&root.join("a"));
    a.prepare_local_passphrase("first file passphrase").unwrap();
    a.create_vault(
        server.first_connect_code.as_deref().unwrap(),
        "file vault passphrase",
        "first",
    )
    .await
    .unwrap();
    a.confirm_kit_saved().await.unwrap();
    let b = app(&root.join("b"));
    b.prepare_local_passphrase("second file passphrase")
        .unwrap();
    b.join(
        &a.new_connection_code().await.unwrap(),
        "file vault passphrase",
        "second",
    )
    .await
    .unwrap();
    let mut bytes = b"PRIVATE-CONTENT-PROBE".to_vec();
    bytes.extend((0..900_000).map(|i| ((i * 31) % 256) as u8));
    let input = root.join("PRIVATE-NAME-PROBE.dat");
    std::fs::write(&input, &bytes).unwrap();
    let id = a.import_file(&input).await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(a.status().await.failed, 0);
    b.sync_now().await.unwrap();
    assert_eq!(
        b.search("PRIVATE-NAME", Filter::Files, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(!b.attachment_info(&id).await.unwrap().cached);
    // Metadata edits work on a receiving client before it has downloaded the bytes.
    b.set_tags(&id, &["PrivateFileTagProbe".into()])
        .await
        .unwrap();
    b.set_flags(&id, Some(true), None, Some("kept note".into()))
        .await
        .unwrap();
    b.sync_now().await.unwrap();
    a.sync_now().await.unwrap();
    assert_eq!(b.attachment_bytes(&id).await.unwrap(), bytes);
    assert!(b.attachment_info(&id).await.unwrap().cached);
    let store = onepaste_server::store::Store::open(&root.join("server")).unwrap();
    let public = store
        .list_items(&a.status().await.device_id, 0, 200)
        .unwrap()
        .to_string();
    for marker in [
        "PRIVATE-NAME-PROBE",
        "PRIVATE-CONTENT-PROBE",
        "PrivateFileTagProbe",
    ] {
        assert!(!public.contains(marker));
    }
    drop(store);
    scan(&root.join("a"));
    scan(&root.join("b"));
    a.shutdown();
    b.shutdown();
    drop(a);
    drop(b);
    server.shutdown().await;
    let before = onepaste_client::requests_started();
    let b = app(&root.join("b"));
    b.unlock_local("second file passphrase").await.unwrap();
    assert_eq!(b.attachment_bytes(&id).await.unwrap(), bytes);
    assert_eq!(onepaste_client::requests_started(), before);
    b.save_attachment_at(&id, &root.join("download.dat"), b.session_epoch())
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.join("download.dat")).unwrap(), bytes);
    let rows = b.search("", Filter::Files, 10, 0).await.unwrap();
    assert_eq!(rows[0].tags, vec!["PrivateFileTagProbe"]);
    assert!(rows[0].favorite);
    b.shutdown();
    drop(b);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_corrupt_cache_and_fresh_start_preserve_the_correct_boundaries() {
    let _serial = SERIAL.lock().await;
    let root = directory();
    let input = root.join("sample.bin");
    std::fs::write(&input, b"original bytes").unwrap();
    let large = root.join("large.bin");
    let sparse = std::fs::File::create(&large).unwrap();
    sparse
        .set_len(onepaste_core::attachment::MAX_FILE_BYTES + 1)
        .unwrap();
    drop(sparse);
    let core = app(&root.join("client"));
    core.prepare_local_passphrase("file integrity passphrase")
        .unwrap();
    core.choose_local_only().await.unwrap();
    assert_eq!(
        core.import_file(&large).await.unwrap_err().code(),
        "attachment_too_large"
    );
    assert!(core
        .search("", Filter::Files, 10, 0)
        .await
        .unwrap()
        .is_empty());
    let id = core.import_file(&input).await.unwrap();
    let blob = std::fs::read_dir(root.join("client/attachments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let chunk = blob.join("0.bin");
    let mut sealed = std::fs::read(&chunk).unwrap();
    sealed[30] ^= 1;
    std::fs::write(&chunk, &sealed).unwrap();
    assert_eq!(
        core.attachment_bytes(&id).await.unwrap_err().code(),
        "attachment_integrity"
    );
    let destination = root.join("must-not-exist.bin");
    assert!(core
        .save_attachment_at(&id, &destination, core.session_epoch())
        .await
        .is_err());
    assert!(!destination.exists());
    core.lock_session().await.unwrap();
    core.restart_local_setup().await.unwrap();
    assert!(!root.join("client/attachments").exists());
    let archive = std::fs::read_dir(root.join("client"))
        .unwrap()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("previous-data-")
        })
        .unwrap()
        .path();
    assert!(archive.join("attachments").exists());
    core.prepare_local_passphrase("new file passphrase")
        .unwrap();
    core.choose_local_only().await.unwrap();
    assert!(core
        .search("", Filter::Files, 10, 0)
        .await
        .unwrap()
        .is_empty());
    core.import_file(&input).await.unwrap();
    core.wipe_local_data().await.unwrap();
    assert!(!root.join("client/attachments").exists());
    core.shutdown();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread")]
async fn locking_during_a_real_large_import_cancels_it_without_publishing_into_a_later_session() {
    let _serial = SERIAL.lock().await;
    let root = directory();
    let path = root.join("large.dat");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(onepaste_core::attachment::MAX_FILE_BYTES)
        .unwrap();
    drop(file);
    let core = app(&root.join("client"));
    core.prepare_local_passphrase("cancel import passphrase")
        .unwrap();
    core.choose_local_only().await.unwrap();
    let importing = core.clone();
    let work = tokio::spawn(async move { importing.import_file(&path).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if root.join("client/attachments").exists() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    core.lock_session().await.unwrap();
    assert_eq!(work.await.unwrap().unwrap_err(), AppError::Locked);
    core.unlock_local("cancel import passphrase").await.unwrap();
    assert!(core
        .search("", Filter::Files, 10, 0)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read_dir(root.join("client/attachments"))
            .unwrap()
            .count(),
        0
    );
    core.shutdown();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_servers_fetches_uncached_attachments_before_leaving_the_original_server() {
    let _serial = SERIAL.lock().await;
    let root = directory();
    async fn server(path: PathBuf) -> onepaste_server::Server {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut cfg = onepaste_server::config::Config::new(path);
        cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
        cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
        onepaste_server::Server::start(cfg).await.unwrap()
    }
    let first = server(root.join("first")).await;
    let replacement = server(root.join("replacement")).await;
    let a = app(&root.join("a"));
    a.prepare_local_passphrase("first migration passphrase")
        .unwrap();
    a.create_vault(
        first.first_connect_code.as_deref().unwrap(),
        "migration vault passphrase",
        "first",
    )
    .await
    .unwrap();
    a.confirm_kit_saved().await.unwrap();
    let b = app(&root.join("b"));
    b.prepare_local_passphrase("second migration passphrase")
        .unwrap();
    b.join(
        &a.new_connection_code().await.unwrap(),
        "migration vault passphrase",
        "second",
    )
    .await
    .unwrap();
    let original: Vec<u8> = (0..800_000).map(|i| ((i * 7) % 251) as u8).collect();
    let input = root.join("migration.dat");
    std::fs::write(&input, &original).unwrap();
    let id = a.import_file(&input).await.unwrap();
    a.sync_now().await.unwrap();
    b.sync_now().await.unwrap();
    assert!(!b.attachment_info(&id).await.unwrap().cached);
    b.change_server(
        replacement.first_connect_code.as_deref().unwrap(),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(b.attachment_info(&id).await.unwrap().cached);
    first.shutdown().await;
    b.sync_now().await.unwrap();
    assert_eq!(b.status().await.failed, 0);
    let c = app(&root.join("c"));
    c.prepare_local_passphrase("third migration passphrase")
        .unwrap();
    c.join(
        &b.new_connection_code().await.unwrap(),
        "migration vault passphrase",
        "third",
    )
    .await
    .unwrap();
    c.sync_now().await.unwrap();
    assert_eq!(c.attachment_bytes(&id).await.unwrap(), original);
    for core in [&a, &b, &c] {
        core.shutdown();
    }
    replacement.shutdown().await;
    drop((a, b, c));
    let _ = std::fs::remove_dir_all(root);
}
