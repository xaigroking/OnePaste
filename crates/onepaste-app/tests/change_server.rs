//! Switching real HTTPS servers preserves the vault and local history.
use onepaste_app::*;
use onepaste_client::{ConnectionCode, SqliteStore};
use onepaste_server::{config::Config, Server};
use onepaste_sync::{ItemRow, OutboxRow, Store, SyncMeta};
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::Duration};

fn dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-switch-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn quiet() -> AppConfig {
    AppConfig { background_sync: false, listen_for_changes: false, poll_interval: Duration::from_secs(3600), ..AppConfig::default() }
}

struct Desktop {
    app: App,
    dir: PathBuf,
    secrets: Arc<MemorySecretStore>,
    clipboard: Arc<FakeClipboard>,
}

impl Desktop {
    fn new(label: &str) -> Self {
        let dir = dir(label);
        let secrets = Arc::new(MemorySecretStore::new());
        let clipboard = Arc::new(FakeClipboard::new());
        Self { app: App::new(dir.clone(), secrets.clone(), clipboard.clone(), quiet()), dir, secrets, clipboard }
    }

    async fn copy(&self, text: &str) {
        self.app.on_clip_event(ClipEvent::Captured(Content::Text(text.into()))).await;
    }
}

async fn server(label: &str) -> (Server, Config) {
    let mut cfg = Config::new(dir(label));
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
    cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
    cfg.register_per_min = 1000;
    cfg.auth_fail_per_min = 1000;
    (Server::start(cfg.clone()).await.unwrap(), cfg)
}

const PASS: &str = "switch test passphrase probe";

async fn first(d: &Desktop, s: &Server) {
    d.app.create_vault(s.first_connect_code.as_deref().unwrap(), PASS, "Test computer").await.unwrap();
    d.app.confirm_kit_saved().await.unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    values: Vec<Option<String>>,
    token: Option<Vec<u8>>,
    items: Vec<ItemRow>,
    queue: Vec<OutboxRow>,
    meta: SyncMeta,
}

fn snapshot(d: &Desktop) -> Snapshot {
    let mut store = SqliteStore::open(&d.dir.join("onepaste.db")).unwrap();
    let values = ["u", "fp", "device_id", "keyparams", "sealed_device_name", "history_seq_offset", "server_initial_sync_done"].iter().map(|k| store.meta_get(k).unwrap()).collect();
    let token = store.secret_get("devtoken").unwrap();
    let mut result = None;
    store.transact(&mut |tx| {
        result = Some((tx.items()?, tx.outbox()?, tx.meta()?));
        Ok(())
    }).unwrap();
    let (items, queue, meta) = result.unwrap();
    Snapshot { values, token, items, queue, meta }
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_server_switch_preserves_key_history_and_uploads_existing_and_new_items() {
    let (a, _) = server("a").await;
    let (b, _) = server("b").await;
    let d = Desktop::new("desktop");
    first(&d, &a).await;
    d.copy("old synced history").await;
    d.app.sync_now().await.unwrap();
    let old = d.app.search("", Filter::All, 20, 0).await.unwrap();
    d.app.set_flags(&old[0].item_id, Some(true), Some(true), Some("preserved note".into())).await.unwrap();
    d.copy("offline queued history").await;
    let old_ids: Vec<_> = d.app.search("", Filter::All, 20, 0).await.unwrap().into_iter().map(|h| h.item_id).collect();
    let key = d.secrets.load().unwrap().unwrap();
    let kit = d.app.show_kit().unwrap();
    let old_device = d.app.status().await.device_id;
    let old_api = onepaste_client::Session::open_with_key(&d.dir, &key).unwrap().api();
    let code = b.first_connect_code.as_deref().unwrap();
    assert_eq!(d.app.probe_server(code).await.unwrap(), ServerVault::Empty);
    d.app.change_server(code, None, None).await.unwrap();
    assert_eq!(d.app.current_server_url().await.as_deref(), Some(b.public_url.as_str()));
    assert_eq!(d.app.devices().await.unwrap().devices.iter().find(|v| v.this_device).unwrap().name.as_deref(), Some("Test computer"));
    assert_eq!(key, d.secrets.load().unwrap().unwrap());
    assert_eq!(kit, d.app.show_kit().unwrap());
    let mut current_ids: Vec<_> = d.app.search("", Filter::All, 20, 0).await.unwrap().into_iter().map(|h| h.item_id).collect();
    current_ids.sort();
    let mut expected = old_ids; expected.sort();
    assert_eq!(current_ids, expected);
    let favorites = d.app.search("", Filter::Favorites, 20, 0).await.unwrap();
    assert_eq!(favorites[0].note, "preserved note");
    assert!(favorites[0].pinned);
    d.app.sync_now().await.unwrap();
    d.copy("new server copy").await;
    d.app.sync_now().await.unwrap();
    let second = Desktop::new("second");
    second.app.join(&d.app.new_connection_code().await.unwrap(), PASS, "Second computer").await.unwrap();
    second.app.sync_now().await.unwrap();
    assert_eq!(second.app.search("", Filter::All, 20, 0).await.unwrap().len(), 3);
    // The old server still has the original device; switching does not revoke it.
    let devices = old_api.call(onepaste_sync::Method::Get, "/v1/devices", None).await.unwrap();
    assert!(devices.as_array().unwrap().iter().any(|v| v["device_id"] == old_device && v["revoked"] == false));
    let a_token = ConnectionCode::parse(a.first_connect_code.as_deref().unwrap()).unwrap();
    assert_ne!(d.app.devices().await.unwrap().server_url, a_token.u);
    assert_ne!(d.app.status().await.device_id, old_device);
    let log = format!("{}{}", serde_json::to_string(&d.app.event_log()).unwrap(), d.app.diagnostics().await);
    for secret in [code, PASS, &kit] { assert!(!log.contains(secret)); }
    let state = snapshot(&d);
    d.app.shutdown();
    let restarted = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert!(restarted.start().await.unwrap());
    assert_eq!(restarted.devices().await.unwrap().server_url, ConnectionCode::parse(code).unwrap().u);
    assert_eq!(snapshot(&d), state);
    assert_eq!(restarted.search("", Filter::All, 20, 0).await.unwrap().len(), 3);
    restarted.shutdown();
    d.app.shutdown(); second.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_credentials_foreign_vault_expired_code_and_bad_pin_leave_every_local_row_unchanged() {
    let (a, _) = server("fail-a").await;
    let (b, _) = server("fail-b").await;
    let d = Desktop::new("failure");
    let foreign = Desktop::new("foreign");
    first(&d, &a).await;
    first(&foreign, &b).await;
    d.copy("history that must survive failure").await;
    d.app.sync_now().await.unwrap();
    d.copy("pending that must survive failure").await;
    let before = snapshot(&d);
    let code = foreign.app.new_connection_code().await.unwrap();
    assert_eq!(d.app.change_server(&code, Some("wrong passphrase probe"), None).await, Err(AppError::WrongPassphrase));
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.app.change_server(&code, None, Some(&d.app.show_kit().unwrap())).await, Err(AppError::WrongRecoveryKit));
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.app.change_server(&code, Some(PASS), None).await, Err(AppError::WrongKey));
    assert_eq!(snapshot(&d), before);
    b.clock.advance(24 * 3600 + 1);
    assert_eq!(d.app.change_server(&code, Some(PASS), None).await, Err(AppError::Server("registration_invalid".into())));
    assert_eq!(snapshot(&d), before);
    let mut bad_pin = ConnectionCode::parse(&foreign.app.new_connection_code().await.unwrap()).unwrap();
    bad_pin.fp = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
    assert_eq!(d.app.change_server(&bad_pin.encode(), Some(PASS), None).await, Err(AppError::Unreachable));
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.app.change_server("bad-code-probe", None, None).await, Err(AppError::InvalidConnectionCode));
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap().len(), 2);
    let output = format!("{}{}", serde_json::to_string(&d.app.event_log()).unwrap(), d.app.diagnostics().await);
    for needle in [&code, PASS, "wrong passphrase probe", "bad-code-probe"] { assert!(!output.contains(needle)); }
    d.app.shutdown(); foreign.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn same_vault_accepts_passphrase_and_kit_without_duplicate_uploads_and_indexes_lower_sequences() {
    let (a, _) = server("same-a").await;
    let (b, _) = server("same-b").await;
    let d = Desktop::new("same");
    first(&d, &a).await;
    for n in 0..25 { d.copy(&format!("original history {n}")).await; }
    d.app.sync_now().await.unwrap();
    let key = d.secrets.load().unwrap().unwrap();
    let a_api = onepaste_client::Session::open_with_key(&d.dir, &key).unwrap().api();
    d.app.change_server(b.first_connect_code.as_deref().unwrap(), None, None).await.unwrap();
    d.app.sync_now().await.unwrap();
    let b_api = onepaste_client::Session::open_with_key(&d.dir, &key).unwrap().api();
    d.app.change_server(&a_api.invite().await.unwrap(), Some(PASS), None).await.unwrap();
    assert_eq!(d.app.status().await.pending, 0, "identical remote items are not queued as duplicate creations");
    d.app.sync_now().await.unwrap();
    let kit = d.app.show_kit().unwrap();
    d.app.change_server(&b_api.invite().await.unwrap(), None, Some(&kit)).await.unwrap();
    assert_eq!(d.app.status().await.pending, 0);
    d.app.sync_now().await.unwrap();
    let other = Desktop::new("remote-writer");
    other.app.join(&d.app.new_connection_code().await.unwrap(), PASS, "Other computer").await.unwrap();
    other.copy("remote lower sequence after replacement").await;
    other.app.sync_now().await.unwrap();
    d.app.sync_now().await.unwrap();
    assert_eq!(d.app.search("remote lower sequence", Filter::All, 10, 0).await.unwrap().len(), 1);
    assert_eq!(d.app.search("", Filter::All, 50, 0).await.unwrap().len(), 26);
    assert_eq!(d.secrets.load().unwrap().unwrap(), key);
    d.app.shutdown(); other.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_only_mode_has_no_server_replacement_path() {
    let d = Desktop::new("local");
    d.app.choose_local_only().await.unwrap();
    assert!(d.app.current_server_url().await.is_none());
    d.copy("local history").await;
    let before = snapshot(&d);
    assert_eq!(d.app.change_server("unused", None, None).await, Err(AppError::NotJoined));
    assert_eq!(snapshot(&d), before);
    d.app.shutdown();
}

async fn interrupted_target(close: bool) -> (String, axum_server::Handle<std::net::SocketAddr>, Arc<tokio::sync::Notify>) {
    use axum::{http::StatusCode, routing::{get, post}, Json, Router};
    let material = onepaste_server::tls::load_or_create(&dir("interruption-tls")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&material.cert_path, &material.key_path).await.unwrap();
    let handle = axum_server::Handle::new();
    let registered = Arc::new(tokio::sync::Notify::new());
    let signal = registered.clone();
    let stop = handle.clone();
    let router = Router::new()
        .route("/v1/vault/keyparams", get(|| async { (StatusCode::NOT_FOUND, Json(serde_json::json!({"code":"vault_empty"}))) }))
        .route("/v1/devices/register", post(move || {
            let (signal, stop) = (signal.clone(), stop.clone());
            async move {
                signal.notify_one();
                if close { stop.shutdown(); }
                std::future::pending::<StatusCode>().await
            }
        }));
    let server = axum_server::from_tcp_rustls(listener, tls).unwrap().handle(handle.clone());
    tokio::spawn(server.serve(router.into_make_service()));
    let code = ConnectionCode { u: format!("https://{address}"), fp: material.fingerprint, t: "A".repeat(43) }.encode();
    (code, handle, registered)
}

#[tokio::test(flavor = "multi_thread")]
async fn network_failure_after_probe_leaves_connection_history_key_and_queue_unchanged() {
    let (a, _) = server("network-a").await;
    let d = Desktop::new("network");
    first(&d, &a).await;
    d.copy("queued before interrupted registration").await;
    let before = snapshot(&d);
    let key = d.secrets.load().unwrap().unwrap();
    let (code, handle, reached) = interrupted_target(true).await;
    let result = tokio::time::timeout(Duration::from_secs(5), d.app.change_server(&code, None, None)).await.unwrap();
    assert_eq!(result, Err(AppError::Unreachable));
    tokio::time::timeout(Duration::from_secs(1), reached.notified()).await.unwrap();
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.secrets.load().unwrap().unwrap(), key);
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap().len(), 1);
    assert!(d.clipboard.writes().is_empty());
    let output = format!("{}{}", serde_json::to_string(&d.app.event_log()).unwrap(), d.app.diagnostics().await);
    assert!(!output.contains(&code) && !output.contains(PASS));
    handle.shutdown(); d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_during_registration_keeps_the_old_connection_usable() {
    let (a, _) = server("cancel-a").await;
    let d = Desktop::new("cancel");
    first(&d, &a).await;
    d.copy("kept after cancellation").await;
    let before = snapshot(&d);
    let (code, handle, reached) = interrupted_target(false).await;
    let app = d.app.clone();
    let task = tokio::spawn(async move { app.change_server(&code, None, None).await });
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(snapshot(&d), before);
    d.app.sync_now().await.unwrap();
    assert_eq!(d.app.status().await.pending, 0);
    handle.shutdown(); d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn rebuilding_the_certificate_at_the_same_address_can_be_repaired_without_wiping_history() {
    let (a, cfg) = server("certificate").await;
    let d = Desktop::new("certificate-client");
    first(&d, &a).await;
    d.copy("survives certificate replacement").await;
    d.app.sync_now().await.unwrap();
    let key = d.secrets.load().unwrap().unwrap();
    a.shutdown().await;
    let (cert, private) = onepaste_server::tls::paths(&cfg.data_dir);
    std::fs::remove_file(cert).unwrap(); std::fs::remove_file(private).unwrap();
    let renewed = Server::start(cfg).await.unwrap();
    assert_eq!(d.app.sync_now().await, Err(AppError::Unreachable));
    d.app.change_server(&renewed.new_connect_code().unwrap(), Some(PASS), None).await.unwrap();
    d.app.sync_now().await.unwrap();
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap().len(), 1);
    assert_eq!(d.secrets.load().unwrap().unwrap(), key);
    assert_eq!(d.app.devices().await.unwrap().devices.len(), 2);
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn background_sync_replaces_the_old_event_subscription_and_receives_new_server_events() {
    let (a, _) = server("background-a").await;
    let (b, _) = server("background-b").await;
    let mut d = Desktop::new("background");
    d.app = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), AppConfig { background_sync: true, listen_for_changes: true, ..quiet() });
    first(&d, &a).await;
    d.clipboard.user_copies(Content::Text("background history".into()));
    wait_until(|| async { d.app.search("background history", Filter::All, 5, 0).await.unwrap().len() == 1 && d.app.status().await.pending == 0 }).await;
    d.app.change_server(b.first_connect_code.as_deref().unwrap(), None, None).await.unwrap();
    wait_until(|| async { d.app.status().await.pending == 0 }).await;
    let other = Desktop::new("background-writer");
    other.app.join(&d.app.new_connection_code().await.unwrap(), PASS, "Writer").await.unwrap();
    other.copy("arrived only through the new server event stream").await;
    other.app.sync_now().await.unwrap();
    wait_until(|| async { d.app.search("arrived only through", Filter::All, 5, 0).await.unwrap().len() == 1 }).await;
    d.app.shutdown(); other.app.shutdown();
}

async fn wait_until<F: std::future::Future<Output = bool>>(mut predicate: impl FnMut() -> F) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !predicate().await { tokio::time::sleep(Duration::from_millis(25)).await; }
    }).await.expect("background operation completed");
}
