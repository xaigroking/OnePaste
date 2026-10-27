//! OnePaste self-hosted sync server: axum + SQLite, HTTPS only. The server stores ciphertext and never decrypts anything.

pub mod api;
pub mod config;
pub mod error;
pub mod ratelimit;
pub mod store;
pub mod tls;
pub mod util;

use crate::api::{connection_code, AppState};
use crate::config::Config;
use crate::ratelimit::Limiter;
use crate::store::Store;
use crate::util::{random_token, sha256, Clock};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub use crate::error::ApiError;

#[derive(Debug)]
pub struct StartError(pub String);

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StartError {}

fn err<E: std::fmt::Display>(what: &str) -> impl FnOnce(E) -> StartError + '_ {
    move |e| StartError(format!("{what}: {e}"))
}

/// A running server.
pub struct Server {
    pub addr: SocketAddr,
    pub fingerprint: String,
    pub public_url: String,
    /// The connection code created on first start (a vault with no device yet); `None` otherwise.
    pub first_connect_code: Option<String>,
    pub clock: Arc<Clock>,
    state: Arc<AppState>,
    handle: axum_server::Handle<SocketAddr>,
    task: tokio::task::JoinHandle<()>,
    cleanup: tokio::task::JoinHandle<()>,
}

/// The first non-loopback interface address, found by asking the OS which source address it would use.
fn guess_host() -> String {
    let probe = UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.connect("192.0.2.1:9")?;
        s.local_addr()
    });
    match probe {
        Ok(a) if !a.ip().is_loopback() && !a.ip().is_unspecified() => a.ip().to_string(),
        _ => "127.0.0.1".to_string(),
    }
}

fn resolve_public_url(cfg: &Config, port: u16) -> String {
    match &cfg.public_url {
        Some(u) => u.clone(),
        None => {
            tracing::warn!(event = "public_url_guessed", "no public URL configured, guessing one from the network interfaces");
            format!("https://{}:{port}", guess_host())
        }
    }
}

impl Server {
    pub async fn start(cfg: Config) -> Result<Server, StartError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        std::fs::create_dir_all(&cfg.data_dir).map_err(err("create data dir"))?;
        let tls = tls::load_or_create(&cfg.data_dir).map_err(err("tls"))?;
        let store = Store::open(&cfg.data_dir).map_err(|_| StartError("open database".into()))?;

        let listener = std::net::TcpListener::bind(cfg.listen).map_err(err("bind"))?;
        listener.set_nonblocking(true).map_err(err("bind"))?;
        let addr = listener.local_addr().map_err(err("bind"))?;
        let public_url = resolve_public_url(&cfg, addr.port());

        let clock = Arc::new(Clock::default());
        let first_connect_code = if store.device_count().map_err(|_| StartError("read database".into()))? == 0 {
            let reg = random_token();
            let hash = sha256(&util::b64url_decode_32(&reg).ok_or_else(|| StartError("token".into()))?);
            store
                .create_reg_token(&hash, clock.now(), cfg.reg_token_ttl_secs, true)
                .map_err(|_| StartError("write database".into()))?;
            let code = connection_code(&public_url, &tls.fingerprint, &reg);
            println!("{code}");
            Some(code)
        } else {
            None
        };

        let (events, _) = broadcast::channel(256);
        let state = Arc::new(AppState {
            store,
            limiter: Limiter::new(cfg.register_per_min, cfg.auth_fail_per_min),
            clock: clock.clone(),
            cfg: cfg.clone(),
            events,
        });
        let app = api::router(state.clone());
        let rustls_cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert_path, &tls.key_path)
            .await
            .map_err(err("load certificate"))?;
        let handle = axum_server::Handle::new();
        let server = axum_server::from_tcp_rustls(listener, rustls_cfg).map_err(err("serve"))?;
        let task = {
            let handle = handle.clone();
            tokio::spawn(async move {
                let _ = server.handle(handle).serve(app.into_make_service_with_connect_info::<SocketAddr>()).await;
            })
        };
        let cleanup = {
            let state = state.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(24 * 3600));
                tick.tick().await;
                loop {
                    tick.tick().await;
                    run_cleanup(&state);
                }
            })
        };
        Ok(Server { addr, fingerprint: tls.fingerprint, public_url, first_connect_code, clock, state, handle, task, cleanup })
    }

    /// Runs the daily maintenance now.
    pub fn cleanup_now(&self) {
        run_cleanup(&self.state);
    }

    /// Creates a new connection code and invalidates the unused registration credentials, like the `connect-code` subcommand.
    pub fn new_connect_code(&self) -> Result<String, ApiError> {
        new_code(&self.state.store, &self.state.cfg, &self.public_url, &self.fingerprint, self.clock.now())
    }

    pub async fn shutdown(self) {
        self.handle.shutdown();
        self.cleanup.abort();
        let _ = self.task.await;
    }
}

fn run_cleanup(s: &AppState) {
    if s.store.cleanup(s.clock.now(), s.cfg.tombstone_retention_secs, s.cfg.idempotency_ttl_secs).is_err() {
        tracing::error!(kind = "cleanup", "maintenance failed");
    }
}

fn new_code(store: &Store, cfg: &Config, public_url: &str, fingerprint: &str, now: i64) -> Result<String, ApiError> {
    let reg = random_token();
    let bytes = util::b64url_decode_32(&reg).ok_or_else(ApiError::internal)?;
    store.create_reg_token(&sha256(&bytes), now, cfg.reg_token_ttl_secs, true)?;
    Ok(connection_code(public_url, fingerprint, &reg))
}

/// The `connect-code` subcommand: works on the data directory directly, also while the server is running.
pub fn connect_code(cfg: &Config) -> Result<String, StartError> {
    let tls = tls::load_or_create(&cfg.data_dir).map_err(err("tls"))?;
    let store = Store::open(&cfg.data_dir).map_err(|_| StartError("open database".into()))?;
    let public_url = resolve_public_url(cfg, cfg.listen.port());
    new_code(&store, cfg, &public_url, &tls.fingerprint, util::unix_now()).map_err(|_| StartError("write database".into()))
}
