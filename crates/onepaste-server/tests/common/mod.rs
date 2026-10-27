//! Test harness: a real server on a random port, and an HTTPS client that pins the certificate by SPKI fingerprint.
#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD as B64, engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use onepaste_core::crypto::{self, aad, KdfParams, Key};
use onepaste_core::envelope::{seal_item, ItemPayload};
use onepaste_core::keys::{vault_proof, KeyParams};
use onepaste_server::config::Config;
use onepaste_server::Server;
use reqwest::header::HeaderMap;
use reqwest::Method;
use serde_json::{json, Value};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};
use tracing_subscriber::fmt::MakeWriter;

pub const KDF: KdfParams = KdfParams::MIN;

pub use onepaste_client::pin::pinned_client;

static LOGS: Mutex<Vec<u8>> = Mutex::new(Vec::new());

pub struct LogWriter;

impl io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        LOGS.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct LogMake;

impl<'a> MakeWriter<'a> for LogMake {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> LogWriter {
        LogWriter
    }
}

pub fn init_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = tracing_subscriber::fmt().json().with_writer(LogMake).try_init();
    });
}

pub fn captured_logs() -> String {
    String::from_utf8_lossy(&LOGS.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
}

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("onepaste-srv-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&p);
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: HeaderMap,
    pub json: Value,
}

pub struct Env {
    pub dir: TempDir,
    pub cfg: Config,
    pub server: Option<Server>,
    pub http: reqwest::Client,
    pub base: String,
    pub fingerprint: String,
}

impl Env {
    pub async fn new() -> Env {
        Env::with(|_| {}).await
    }

    pub async fn with(tweak: impl FnOnce(&mut Config)) -> Env {
        init_logs();
        let dir = TempDir::new();
        let mut cfg = Config::new(dir.0.join("data"));
        cfg.listen = "127.0.0.1:0".parse().unwrap();
        cfg.public_url = Some("https://localhost:8443".to_string());
        tweak(&mut cfg);
        let server = Server::start(cfg.clone()).await.unwrap();
        let http = pinned_client(&server.fingerprint);
        let base = format!("https://127.0.0.1:{}", server.addr.port());
        let fingerprint = server.fingerprint.clone();
        Env { dir, cfg, server: Some(server), http, base, fingerprint }
    }

    pub fn server(&self) -> &Server {
        self.server.as_ref().unwrap()
    }

    /// Stops the server and starts a new one on the same data directory (the port changes).
    pub async fn restart(&mut self) {
        self.server.take().unwrap().shutdown().await;
        let server = Server::start(self.cfg.clone()).await.unwrap();
        self.base = format!("https://127.0.0.1:{}", server.addr.port());
        self.server = Some(server);
    }

    pub async fn send(&self, method: Method, path: &str, token: Option<&str>, idem: Option<&str>, body: Option<Vec<u8>>, extra: &[(&str, &str)]) -> Resp {
        let mut rb = self.http.request(method.clone(), format!("{}{path}", self.base)).header("OnePaste-Proto", "1");
        if let Some(t) = token {
            rb = rb.header("Authorization", format!("Bearer {t}"));
        }
        if method != Method::GET {
            let key = idem.map(str::to_string).unwrap_or_else(|| onepaste_core::id::new_id().unwrap());
            rb = rb.header("Idempotency-Key", key);
        }
        for (k, v) in extra {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("Content-Type", "application/json").body(b);
        }
        let r = rb.send().await.unwrap();
        let status = r.status().as_u16();
        let headers = r.headers().clone();
        let bytes = r.bytes().await.unwrap_or_default();
        Resp { status, headers, json: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
    }

    pub async fn call(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> Resp {
        self.send(method, path, token, None, body.map(|v| v.to_string().into_bytes()), &[]).await
    }

    pub async fn get(&self, path: &str, token: &str) -> Resp {
        self.call(Method::GET, path, Some(token), None).await
    }
}

/// Extracts the registration credential from a connection code.
pub fn reg_token_of(code: &str) -> String {
    let raw = B64URL.decode(code.strip_prefix("onepaste1:").expect("prefix")).unwrap();
    let v: Value = serde_json::from_slice(&raw).unwrap();
    v["t"].as_str().unwrap().to_string()
}

pub fn proof_b64(mk: &Key) -> String {
    B64URL.encode(vault_proof(mk))
}

pub fn sealed_name(mk: &Key, device_id: &str, name: &str) -> String {
    B64.encode(crypto::seal(mk, &aad::device(device_id), name.as_bytes()).unwrap())
}

pub struct Dev {
    pub id: String,
    pub token: String,
    pub mk: Key,
    pub record: KeyParams,
}

/// Registers the first device (vault still empty) with `init`.
pub async fn first_device(env: &Env) -> Dev {
    let reg = reg_token_of(env.server().first_connect_code.as_deref().expect("first start prints a connection code"));
    let (record, mk) = KeyParams::create("pw", KDF).unwrap();
    let id = onepaste_core::id::new_id().unwrap();
    let r = env
        .call(
            Method::POST,
            "/v1/devices/register",
            None,
            Some(json!({
                "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&mk, &id, "first"),
                "init": { "record": record, "vault_proof": proof_b64(&mk) }
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.json);
    Dev { id, token: r.json["device_token"].as_str().unwrap().to_string(), mk, record }
}

/// Creates a registration credential through `invite` on behalf of `by`.
pub async fn invite(env: &Env, by: &Dev) -> String {
    let r = env.call(Method::POST, "/v1/devices/invite", Some(&by.token), None).await;
    assert_eq!(r.status, 200, "{}", r.json);
    r.json["reg_token"].as_str().unwrap().to_string()
}

/// Registers another device into the existing vault with a vault proof, using the key material of `like`.
pub async fn join_device(env: &Env, reg: &str, like: &Dev) -> Dev {
    let id = onepaste_core::id::new_id().unwrap();
    let r = env
        .call(
            Method::POST,
            "/v1/devices/register",
            None,
            Some(json!({
                "reg_token": reg, "device_id": id, "sealed_name": sealed_name(&like.mk, &id, "joined"),
                "vault_proof": proof_b64(&like.mk)
            })),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.json);
    Dev { id, token: r.json["device_token"].as_str().unwrap().to_string(), mk: Key::from_bytes(*like.mk.as_bytes()), record: like.record.clone() }
}

pub fn envelope_json(mk: &Key, id: &str, text: &str) -> Value {
    let payload = ItemPayload { kind: "text".into(), content: Some(text.into()), ..Default::default() };
    serde_json::to_value(seal_item(mk, id, 1_700_000_000, 1_700_000_000, None, &payload).unwrap()).unwrap()
}

pub async fn put_item(env: &Env, dev: &Dev, id: &str, base_seq: i64, text: &str) -> Resp {
    env.call(Method::PUT, &format!("/v1/items/{id}"), Some(&dev.token), Some(json!({ "base_seq": base_seq, "envelope": envelope_json(&dev.mk, id, text) }))).await
}

pub fn new_id() -> String {
    onepaste_core::id::new_id().unwrap()
}
