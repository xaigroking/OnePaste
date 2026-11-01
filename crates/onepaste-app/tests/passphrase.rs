//! Passphrase changes against the actual HTTPS API and SQLite store, with controlled stale reads and lost responses.
use onepaste_app::*;
use onepaste_client::{ConnectionCode, Session, SqliteStore};
use onepaste_core::keys::{vault_proof, KeyParams};
use onepaste_server::{api::{self, AppState}, config::Config, ratelimit::Limiter, store::Store as ServerStore, util::{self, Clock}};
use onepaste_sync::{ItemRow, Method, OutboxRow, Store, SyncMeta};
use axum::{body::{self, Body}, extract::Request, middleware::{self, Next}, response::Response};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicUsize, AtomicU8, Ordering}}, time::Duration};

const OLD: &str = "old-passphrase-probe-47d6";
const NEW: &str = "new-passphrase-probe-219a";
fn dir(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("onepaste-passphrase-{label}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap(); path
}
fn quiet() -> AppConfig { AppConfig { background_sync: false, listen_for_changes: false, poll_interval: Duration::from_secs(3600), ..AppConfig::default() } }
struct Desktop { app: App, dir: PathBuf, secrets: Arc<MemorySecretStore> }
impl Desktop {
    fn new(label: &str) -> Self {
        let dir = dir(label); let secrets = Arc::new(MemorySecretStore::new());
        Self { app: App::new(dir.clone(), secrets.clone(), Arc::new(FakeClipboard::new()), quiet()), dir, secrets }
    }
    fn store(&self) -> SqliteStore { SqliteStore::open(&self.dir.join("onepaste.db")).unwrap() }
    fn record(&self) -> String { self.store().meta_get("keyparams").unwrap().unwrap() }
    fn rows(&self) -> (Vec<ItemRow>, Vec<OutboxRow>, SyncMeta) {
        let mut value = None;
        self.store().transact(&mut |tx| { value = Some((tx.items()?, tx.outbox()?, tx.meta()?)); Ok(()) }).unwrap(); value.unwrap()
    }
}
#[derive(Default)]
struct Fault { mode: AtomicU8, puts: AtomicUsize, keys: Mutex<Vec<String>>, reached: tokio::sync::Notify }
struct TestServer { cfg: Config, code: String, state: Arc<AppState>, fault: Arc<Fault>, handle: axum_server::Handle<std::net::SocketAddr>, task: tokio::task::JoinHandle<()> }
impl Drop for TestServer { fn drop(&mut self) { self.handle.shutdown(); self.task.abort(); } }
impl TestServer {
    async fn start(cfg: Option<Config>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut cfg = cfg.unwrap_or_else(|| { let mut cfg = Config::new(dir("server")); cfg.listen = "127.0.0.1:0".parse().unwrap(); cfg });
        cfg.register_per_min = 1000; cfg.auth_fail_per_min = 1000;
        let tls = onepaste_server::tls::load_or_create(&cfg.data_dir).unwrap();
        let store = ServerStore::open(&cfg.data_dir).unwrap();
        let clock = Arc::new(Clock::default()); let token = util::random_token();
        store.create_reg_token(&util::sha256(&util::b64url_decode_32(&token).unwrap()), clock.now(), cfg.reg_token_ttl_secs, store.device_count().unwrap() == 0).unwrap();
        let listener = std::net::TcpListener::bind(cfg.listen).unwrap(); let address = listener.local_addr().unwrap(); listener.set_nonblocking(true).unwrap(); cfg.listen = address;
        let code = ConnectionCode { u: format!("https://{address}"), fp: tls.fingerprint, t: token }.encode();
        let (events, _) = tokio::sync::broadcast::channel(256);
        let state = Arc::new(AppState { store, limiter: Limiter::new(1000, 1000), clock, cfg: cfg.clone(), events });
        let fault = Arc::new(Fault::default()); let handle = axum_server::Handle::new();
        let (inject, stop, database) = (fault.clone(), handle.clone(), state.clone());
        let router = api::router(state.clone()).layer(middleware::from_fn(move |mut request: Request, next: Next| {
            let (inject, stop, database) = (inject.clone(), stop.clone(), database.clone());
            async move {
                let keyparams = request.uri().path() == "/v1/vault/keyparams";
                let get = keyparams && request.method() == axum::http::Method::GET;
                let put = keyparams && request.method() == axum::http::Method::PUT;
                if put { inject.puts.fetch_add(1, Ordering::SeqCst); inject.keys.lock().unwrap().push(request.headers().get("idempotency-key").unwrap().to_str().unwrap().into()); }
                if put && inject.mode.load(Ordering::SeqCst) == 4 {
                    inject.reached.notify_one(); return std::future::pending::<Response>().await;
                }
                if put && inject.mode.load(Ordering::SeqCst) == 3 {
                    let (parts, body) = request.into_parts();
                    let bytes = body::to_bytes(body, 1024 * 1024).await.unwrap();
                    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let proof = util::b64url_decode_32(value["vault_proof"].as_str().unwrap()).unwrap();
                    let current = database.store.keyparams_view().unwrap().unwrap();
                    let record: KeyParams = serde_json::from_value(current.clone()).unwrap();
                    let encoded = serde_json::to_string(&record).unwrap();
                    let race_key = format!("race-{}", inject.puts.load(Ordering::SeqCst));
                    database.store.idempotent("test-race", &race_key, &util::sha256(&bytes), database.clock.now(), 3600,
                        |tx| ServerStore::put_keyparams_tx(tx, &encoded, current["version"].as_i64().unwrap(), &proof)).unwrap();
                    request = Request::from_parts(parts, Body::from(bytes));
                }
                let response = next.run(request).await;
                if get && response.status() == 200 && inject.mode.compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    let (mut parts, body) = response.into_parts();
                    let mut value: serde_json::Value = serde_json::from_slice(&body::to_bytes(body, 1024 * 1024).await.unwrap()).unwrap();
                    value["version"] = (value["version"].as_i64().unwrap() - 1).into(); parts.headers.remove("content-length");
                    return Response::from_parts(parts, Body::from(value.to_string()));
                }
                if put && response.status() == 200 && inject.mode.compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    // The real API transaction committed. Disconnect before delivering its answer, including subsequent reads.
                    stop.shutdown(); return std::future::pending::<Response>().await;
                }
                response
            }
        }));
        let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(tls.cert_path, tls.key_path).await.unwrap();
        let server = axum_server::from_tcp_rustls(listener, config).unwrap().handle(handle.clone());
        let task = tokio::spawn(async move { server.serve(router.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap(); });
        Self { cfg, code, state, fault, handle, task }
    }
    async fn stop(&mut self) { self.handle.shutdown(); (&mut self.task).await.unwrap(); }
    async fn first(&self, d: &Desktop) {
        d.app.create_vault(&self.code, OLD, "Test computer").await.unwrap(); d.app.confirm_kit_saved().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_the_passphrase_keeps_history_queue_master_key_and_recovery_kit_and_changes_new_device_unlocking() {
    let s = TestServer::start(None).await; let d = Desktop::new("first"); s.first(&d).await;
    d.app.on_clip_event(ClipEvent::Captured(Content::Text("kept history".into()))).await;
    let rows = d.rows(); let key = d.secrets.load().unwrap().unwrap(); let token = d.store().secret_get("devtoken").unwrap(); let kit = d.app.show_kit().unwrap();
    d.app.change_passphrase(OLD, NEW).await.unwrap();
    assert_eq!(d.rows(), rows); assert_eq!(d.secrets.load().unwrap().unwrap(), key); assert_eq!(d.store().secret_get("devtoken").unwrap(), token); assert_eq!(d.app.show_kit().unwrap(), kit);
    let rec: KeyParams = serde_json::from_str(&d.record()).unwrap(); assert!(rec.unlock(OLD).is_err()); assert_eq!(rec.unlock(NEW).unwrap(), key);
    assert!(!d.app.passphrase_change_pending().await.unwrap());
    let code = d.app.new_connection_code().await.unwrap(); let other = Desktop::new("second");
    assert_eq!(other.app.join(&code, OLD, "Old password").await, Err(AppError::WrongPassphrase));
    other.app.join(&code, NEW, "New password").await.unwrap();
    let recovered = Desktop::new("kit"); recovered.app.join_with_kit(&d.app.new_connection_code().await.unwrap(), &kit, "Recovered").await.unwrap();
    for app in [&d.app, &other.app, &recovered.app] {
        let log = serde_json::to_string(&app.event_log()).unwrap(); assert!(!log.contains(OLD)); assert!(!log.contains(NEW)); app.shutdown();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_old_passphrase_and_unreachable_server_leave_local_records_and_rows_unchanged() {
    let mut s = TestServer::start(None).await; let d = Desktop::new("failure"); s.first(&d).await;
    let record = d.record(); let rows = d.rows(); let key = d.secrets.load().unwrap().unwrap();
    assert_eq!(d.app.change_passphrase("wrong-old-probe", NEW).await, Err(AppError::WrongPassphrase)); assert_eq!(s.fault.puts.load(Ordering::SeqCst), 0);
    s.stop().await;
    assert_eq!(d.app.change_passphrase(OLD, NEW).await, Err(AppError::Unreachable));
    assert_eq!(d.record(), record); assert_eq!(d.rows(), rows); assert_eq!(d.secrets.load().unwrap().unwrap(), key); assert!(!d.app.passphrase_change_pending().await.unwrap());
    let log = serde_json::to_string(&d.app.event_log()).unwrap(); for probe in [OLD, NEW, "wrong-old-probe"] { assert!(!log.contains(probe)); }
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_real_stale_response_rewraps_the_current_record_once_with_a_fresh_idempotency_key() {
    let s = TestServer::start(None).await; let d = Desktop::new("stale"); s.first(&d).await;
    let key = d.secrets.load().unwrap().unwrap(); let api = Session::open_with_key(&d.dir, &key).unwrap().api();
    let record: KeyParams = serde_json::from_str(&d.record()).unwrap(); let replacement = record.change_passphrase(OLD, OLD).unwrap();
    api.call(Method::Put, "/v1/vault/keyparams", Some(serde_json::json!({"record":replacement,"base_version":1,"vault_proof":B64.encode(vault_proof(&key))}))).await.unwrap();
    s.fault.puts.store(0, Ordering::SeqCst); s.fault.keys.lock().unwrap().clear(); s.fault.mode.store(1, Ordering::SeqCst);
    d.app.change_passphrase(OLD, NEW).await.unwrap();
    assert_eq!(s.fault.puts.load(Ordering::SeqCst), 2); let keys = s.fault.keys.lock().unwrap(); assert_ne!(keys[0], keys[1]);
    let current = s.state.store.keyparams_view().unwrap().unwrap(); assert_eq!(current["version"], 3);
    let record: KeyParams = serde_json::from_value(current).unwrap(); assert_eq!(record.unlock(NEW).unwrap(), key); assert!(record.unlock(OLD).is_err());
    assert!(!d.app.passphrase_change_pending().await.unwrap()); d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_committed_response_survives_restart_and_is_confirmed_without_a_second_write() {
    let mut s = TestServer::start(None).await; let mut d = Desktop::new("lost"); s.first(&d).await;
    let old_record = d.record(); let rows = d.rows(); s.fault.mode.store(2, Ordering::SeqCst);
    assert_eq!(d.app.change_passphrase(OLD, NEW).await, Err(AppError::Server("passphrase_pending".into())));
    assert_eq!(d.record(), old_record); assert_eq!(d.rows(), rows); assert!(d.app.passphrase_change_pending().await.unwrap());
    let pending = d.store().meta_get("pending_passphrase_change").unwrap().unwrap(); assert!(!pending.contains(OLD)); assert!(!pending.contains(NEW));
    assert_eq!(d.app.change_server(&s.code, Some(OLD), None).await, Err(AppError::Server("passphrase_pending".into())));
    d.app.shutdown(); s.stop().await; let cfg = s.cfg.clone(); drop(s);
    let online = TestServer::start(Some(cfg)).await;
    d.app = App::new(d.dir.clone(), d.secrets.clone(), Arc::new(FakeClipboard::new()), quiet()); d.app.start().await.unwrap();
    assert!(d.app.passphrase_change_pending().await.unwrap()); d.app.resume_passphrase_change().await.unwrap();
    assert_eq!(online.fault.puts.load(Ordering::SeqCst), 0); assert!(!d.app.passphrase_change_pending().await.unwrap()); assert_eq!(d.rows(), rows);
    let record: KeyParams = serde_json::from_str(&d.record()).unwrap(); assert!(record.unlock(OLD).is_err()); assert_eq!(record.unlock(NEW).unwrap(), d.secrets.load().unwrap().unwrap());
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_only_mode_has_no_passphrase_operation() {
    let d = Desktop::new("local"); d.app.choose_local_only().await.unwrap();
    assert_eq!(d.app.change_passphrase(OLD, NEW).await, Err(AppError::NotJoined));
    assert_eq!(d.app.resume_passphrase_change().await, Err(AppError::NotJoined)); assert!(!d.app.passphrase_change_pending().await.unwrap()); d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_stale_responses_stop_after_one_retry_and_leave_no_unconfirmed_request() {
    let s = TestServer::start(None).await; let d = Desktop::new("twice-stale"); s.first(&d).await;
    let record = d.record(); s.fault.mode.store(3, Ordering::SeqCst);
    assert_eq!(d.app.change_passphrase(OLD, NEW).await, Err(AppError::Server("passphrase_stale".into())));
    assert_eq!(s.fault.puts.load(Ordering::SeqCst), 2); assert_eq!(d.record(), record); assert!(!d.app.passphrase_change_pending().await.unwrap());
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_uncommitted_request_reuses_its_durable_idempotency_key_on_explicit_retry() {
    let mut s = TestServer::start(None).await; let mut d = Desktop::new("interrupted"); s.first(&d).await;
    s.fault.mode.store(4, Ordering::SeqCst); let app = d.app.clone();
    let task = tokio::spawn(async move { app.change_passphrase(OLD, NEW).await });
    tokio::time::timeout(Duration::from_secs(5), s.fault.reached.notified()).await.unwrap();
    task.abort(); assert!(task.await.unwrap_err().is_cancelled());
    assert!(d.app.passphrase_change_pending().await.unwrap());
    let first_key = s.fault.keys.lock().unwrap()[0].clone(); let record = d.record();
    d.app.shutdown(); s.stop().await; let cfg = s.cfg.clone(); drop(s);
    let online = TestServer::start(Some(cfg)).await;
    d.app = App::new(d.dir.clone(), d.secrets.clone(), Arc::new(FakeClipboard::new()), quiet()); d.app.start().await.unwrap();
    assert_eq!(d.record(), record); d.app.resume_passphrase_change().await.unwrap();
    assert_eq!(online.fault.puts.load(Ordering::SeqCst), 1); assert_eq!(online.fault.keys.lock().unwrap()[0], first_key);
    let current: KeyParams = serde_json::from_str(&d.record()).unwrap(); assert_eq!(current.unlock(NEW).unwrap(), d.secrets.load().unwrap().unwrap());
    assert!(!d.app.passphrase_change_pending().await.unwrap()); d.app.shutdown();
}
