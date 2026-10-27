//! HTTP layer: routing, the middleware shared by every request, and the handlers.

use crate::config::Config;
use crate::error::{ApiError, ErrParts};
use crate::ratelimit::{Kind, Limiter};
use crate::store::{ItemWrite, RegisterIn, Reply, Store};
use crate::util::{b64url_decode_32, b64url_encode, hex, random_token, request_id, sha256, Clock};
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, RawQuery, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::Router;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_core::crypto::{self, Key};
use onepaste_core::envelope::Envelope;
use onepaste_core::keys::{KeyParams, KEYPARAMS_V};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// Largest accepted request body.
pub const MAX_BODY_BYTES: usize = 12 * 1024 * 1024;
/// Largest accepted sealed item (decoded).
pub const MAX_SEALED_BYTES: usize = 8 * 1024 * 1024;
const PROTO: &str = "1";

pub struct AppState {
    pub store: Store,
    pub limiter: Limiter,
    pub clock: Arc<Clock>,
    pub cfg: Config,
    pub events: broadcast::Sender<i64>,
}

type St = State<Arc<AppState>>;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/devices/register", post(register))
        .route("/v1/devices/invite", post(invite))
        .route("/v1/devices", get(list_devices))
        .route("/v1/devices/{id}", delete(revoke_device))
        .route("/v1/vault/keyparams", get(get_keyparams).put(put_keyparams))
        .route("/v1/items/{id}", put(put_item))
        .route("/v1/items", get(list_items))
        .route("/v1/events", get(events))
        .fallback(|| async { ApiError::not_found() })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), common))
        .with_state(state)
}

/// Protocol check, request id, failed-authentication limit, error bodies and the access log.
async fn common(State(s): St, ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    let started = Instant::now();
    let rid = request_id();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let proto_ok = req.headers().get("onepaste-proto").and_then(|v| v.to_str().ok()) == Some(PROTO);

    let mut resp = if !proto_ok {
        ApiError::new(StatusCode::BAD_REQUEST, "unsupported_version").with("min", json!(1)).with("max", json!(1)).into_response()
    } else if let Err(wait) = s.limiter.check(Kind::AuthFail, addr.ip()) {
        ApiError::rate_limited(wait).into_response()
    } else {
        next.run(req).await
    };

    if proto_ok && resp.status() == StatusCode::UNAUTHORIZED {
        s.limiter.record_failure(addr.ip());
    }
    finish_error_body(&mut resp, &rid);
    let h = resp.headers_mut();
    h.insert(HeaderName::from_static("onepaste-proto"), HeaderValue::from_static(PROTO));
    if let Ok(v) = HeaderValue::from_str(&rid) {
        h.insert(HeaderName::from_static("x-request-id"), v);
    }
    tracing::info!(
        method = %method,
        path = %path,
        status = resp.status().as_u16(),
        request_id = %rid,
        latency_ms = started.elapsed().as_millis() as u64,
        "request"
    );
    resp
}

/// Writes the uniform JSON error body. Responses that did not come from `ApiError` (routing and body-limit rejections) are mapped here.
fn finish_error_body(resp: &mut Response, rid: &str) {
    let status = resp.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return;
    }
    let parts = match resp.extensions().get::<ErrParts>() {
        Some(p) => p.clone(),
        None => {
            let code = match status {
                StatusCode::NOT_FOUND => "not_found",
                StatusCode::PAYLOAD_TOO_LARGE => "too_large",
                s if s.is_server_error() => "internal",
                _ => "invalid_argument",
            };
            ErrParts { code, extra: Map::new() }
        }
    };
    let mut body = parts.extra;
    body.insert("code".to_string(), json!(parts.code));
    body.insert("request_id".to_string(), json!(rid));
    let bytes = Value::Object(body).to_string().into_bytes();
    resp.headers_mut().remove(header::CONTENT_LENGTH);
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    *resp.body_mut() = Body::from(bytes);
}

fn json_response(status: u16, body: Vec<u8>) -> Response {
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    resp
}

fn empty_response(status: u16) -> Response {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    resp
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| ApiError::invalid())
}

fn idem_key(h: &HeaderMap) -> Result<String, ApiError> {
    let v = h.get("idempotency-key").and_then(|v| v.to_str().ok()).ok_or_else(ApiError::invalid)?;
    if v.is_empty() || v.len() > 128 || !v.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(ApiError::invalid());
    }
    Ok(v.to_string())
}

fn bearer(h: &HeaderMap) -> Option<[u8; 32]> {
    let v = h.get(header::AUTHORIZATION)?.to_str().ok()?;
    b64url_decode_32(v.strip_prefix("Bearer ")?)
}

/// Authenticates a device token. Every request is looked up in the database, so a revoked device is rejected immediately.
fn auth(s: &AppState, h: &HeaderMap) -> Result<(String, [u8; 32]), ApiError> {
    let tok = bearer(h).ok_or_else(ApiError::unauthorized)?;
    let id = s.store.auth_device(&sha256(&tok), s.clock.now())?;
    Ok((id, tok))
}

fn request_hash(method: &str, path: &str, body: &[u8]) -> [u8; 32] {
    let mut data = Vec::with_capacity(body.len() + 64);
    data.extend_from_slice(method.as_bytes());
    data.push(0);
    data.extend_from_slice(path.as_bytes());
    data.push(0);
    data.extend_from_slice(body);
    sha256(&data)
}

/// Seals a secret response body for the idempotency table with a key only the caller can rebuild from its own request.
fn idem_seal(material: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, ApiError> {
    let key = idem_key_for(material);
    crypto::seal(&key, IDEM_AAD, plain).map_err(|_| ApiError::internal())
}

fn idem_open(material: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, ApiError> {
    crypto::open(&idem_key_for(material), IDEM_AAD, sealed).map_err(|_| ApiError::internal())
}

const IDEM_AAD: &[u8] = b"onepaste/idempotency/v1";

fn idem_key_for(material: &[u8; 32]) -> Key {
    let mut data = IDEM_AAD.to_vec();
    data.extend_from_slice(material);
    Key::from_bytes(sha256(&data))
}

fn finish_write(s: &AppState, reply: Reply) -> Response {
    if let Some(seq) = reply.notify_seq {
        let _ = s.events.send(seq);
    }
    if reply.status == 204 {
        empty_response(204)
    } else {
        json_response(reply.status, reply.body)
    }
}

fn validate_record(rec: &KeyParams) -> Result<(), ApiError> {
    let b64_ok = |s: &str, max: usize| s.len() <= max && B64.decode(s).is_ok();
    let check = rec.mk_check.as_deref().ok_or_else(ApiError::invalid)?;
    if rec.v != KEYPARAMS_V
        || rec.kdf.m_kib < 8192
        || rec.kdf.t < 1
        || rec.kdf.p < 1
        || !b64_ok(&rec.salt, 1024)
        || !b64_ok(&rec.wrapped_mk, 1024)
        || !b64_ok(check, 1024)
    {
        return Err(ApiError::invalid());
    }
    Ok(())
}

fn decode_proof(s: &str) -> Result<[u8; 32], ApiError> {
    b64url_decode_32(s).ok_or_else(ApiError::invalid)
}

#[derive(Deserialize)]
struct InitBody {
    record: KeyParams,
    vault_proof: String,
}

#[derive(Deserialize)]
struct RegisterBody {
    reg_token: String,
    device_id: String,
    sealed_name: String,
    vault_proof: Option<String>,
    init: Option<InitBody>,
}

async fn register(State(s): St, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    s.limiter.take(Kind::Register, addr.ip()).map_err(ApiError::rate_limited)?;
    let key = idem_key(&headers)?;
    let req: RegisterBody = parse(&body)?;
    let reg = b64url_decode_32(&req.reg_token).ok_or_else(ApiError::registration_invalid)?;
    onepaste_core::id::validate_id(&req.device_id).map_err(|_| ApiError::invalid())?;
    if req.sealed_name.len() > 4096 || B64.decode(&req.sealed_name).is_err() {
        return Err(ApiError::invalid());
    }
    let proof = req.vault_proof.as_deref().map(decode_proof).transpose()?;
    let init = match &req.init {
        Some(i) => {
            validate_record(&i.record)?;
            let canonical = serde_json::to_string(&i.record).map_err(|_| ApiError::internal())?;
            Some((canonical, decode_proof(&i.vault_proof)?))
        }
        None => None,
    };
    let reg_hash = sha256(&reg);
    let token = random_token();
    let token_bytes = b64url_decode_32(&token).ok_or_else(ApiError::internal)?;
    let input = RegisterIn {
        reg_hash,
        device_id: req.device_id,
        sealed_name: req.sealed_name,
        token_hash: sha256(&token_bytes),
        proof,
        init,
    };
    let now = s.clock.now();
    let scope = format!("r:{}", hex(&reg_hash));
    let reply = s.store.idempotent(&scope, &key, &request_hash("POST", "/v1/devices/register", &body), now, s.cfg.idempotency_ttl_secs, |tx| {
        Store::register_tx(tx, now, &input)?;
        let plain = json!({ "device_token": token }).to_string().into_bytes();
        let stored = idem_seal(&reg, &plain)?;
        Ok(Reply { status: 200, body: plain, stored: Some(stored), notify_seq: None, replayed: false })
    })?;
    let reply = if reply.replayed { Reply { body: idem_open(&reg, &reply.body)?, ..reply } } else { reply };
    Ok(finish_write(&s, reply))
}

async fn invite(State(s): St, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    let (device, token) = auth(&s, &headers)?;
    s.limiter.take(Kind::Register, addr.ip()).map_err(ApiError::rate_limited)?;
    let key = idem_key(&headers)?;
    let reg = random_token();
    let reg_bytes = b64url_decode_32(&reg).ok_or_else(ApiError::internal)?;
    let now = s.clock.now();
    let (ttl, idem_ttl) = (s.cfg.reg_token_ttl_secs, s.cfg.idempotency_ttl_secs);
    let reply = s.store.idempotent(&format!("d:{device}"), &key, &request_hash("POST", "/v1/devices/invite", &body), now, idem_ttl, |tx| {
        let expires = Store::invite_tx(tx, &sha256(&reg_bytes), now, ttl)?;
        let plain = json!({ "reg_token": reg, "expires_at": expires }).to_string().into_bytes();
        let stored = idem_seal(&token, &plain)?;
        Ok(Reply { status: 200, body: plain, stored: Some(stored), notify_seq: None, replayed: false })
    })?;
    let reply = if reply.replayed { Reply { body: idem_open(&token, &reply.body)?, ..reply } } else { reply };
    Ok(finish_write(&s, reply))
}

async fn list_devices(State(s): St, headers: HeaderMap) -> Result<Response, ApiError> {
    auth(&s, &headers)?;
    let rows: Vec<Value> = s
        .store
        .list_devices()?
        .into_iter()
        .map(|d| json!({ "device_id": d.id, "sealed_name": d.sealed_name, "created_at": d.created_at, "last_seen_at": d.last_seen_at, "revoked": d.revoked }))
        .collect();
    Ok(json_response(200, Value::Array(rows).to_string().into_bytes()))
}

async fn revoke_device(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> Result<Response, ApiError> {
    let (device, _) = auth(&s, &headers)?;
    let key = idem_key(&headers)?;
    onepaste_core::id::validate_id(&id).map_err(|_| ApiError::invalid())?;
    let path = format!("/v1/devices/{id}");
    let reply = s.store.revoke_device(&id, &format!("d:{device}"), &key, &request_hash("DELETE", &path, b""), s.clock.now(), s.cfg.idempotency_ttl_secs)?;
    Ok(finish_write(&s, reply))
}

async fn get_keyparams(State(s): St, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(v) = headers.get("onepaste-reg-token") {
        s.limiter.take(Kind::Register, addr.ip()).map_err(ApiError::rate_limited)?;
        let reg = v.to_str().ok().and_then(b64url_decode_32).ok_or_else(ApiError::registration_invalid)?;
        if !s.store.reg_token_valid(&sha256(&reg), s.clock.now())? {
            return Err(ApiError::registration_invalid());
        }
    } else {
        auth(&s, &headers)?;
    }
    let view = s.store.keyparams_view()?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "vault_empty"))?;
    Ok(json_response(200, view.to_string().into_bytes()))
}

#[derive(Deserialize)]
struct PutKeyParamsBody {
    record: KeyParams,
    base_version: i64,
    vault_proof: String,
}

async fn put_keyparams(State(s): St, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    let (device, _) = auth(&s, &headers)?;
    let key = idem_key(&headers)?;
    let req: PutKeyParamsBody = parse(&body)?;
    if req.base_version < 1 {
        return Err(ApiError::invalid());
    }
    validate_record(&req.record)?;
    let proof = decode_proof(&req.vault_proof)?;
    let record = serde_json::to_string(&req.record).map_err(|_| ApiError::internal())?;
    let reply = s.store.idempotent(&format!("d:{device}"), &key, &request_hash("PUT", "/v1/vault/keyparams", &body), s.clock.now(), s.cfg.idempotency_ttl_secs, |tx| {
        Store::put_keyparams_tx(tx, &record, req.base_version, &proof)
    })?;
    Ok(finish_write(&s, reply))
}

async fn put_item(State(s): St, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Result<Response, ApiError> {
    let (device, _) = auth(&s, &headers)?;
    let key = idem_key(&headers)?;
    onepaste_core::id::validate_id(&id).map_err(|_| ApiError::invalid())?;
    let v: Value = parse(&body)?;
    let obj = v.as_object().ok_or_else(ApiError::invalid)?;
    let base_seq = obj.get("base_seq").and_then(Value::as_i64).filter(|n| *n >= 0).ok_or_else(ApiError::invalid)?;
    let write = if obj.get("deleted") == Some(&Value::Bool(true)) {
        if obj.contains_key("envelope") {
            return Err(ApiError::invalid());
        }
        ItemWrite::Delete
    } else {
        let env_value = obj.get("envelope").ok_or_else(ApiError::invalid)?;
        // An oversized declared size is `too_large`, not a malformed envelope
        if env_value.get("size").and_then(Value::as_u64).is_some_and(|n| n > MAX_SEALED_BYTES as u64) {
            return Err(ApiError::too_large());
        }
        let env: Envelope = serde_json::from_value(env_value.clone()).map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "malformed_envelope"))?;
        env.validate(MAX_SEALED_BYTES).map_err(|e| ApiError::from_core(&e))?;
        if env.id != id {
            return Err(ApiError::invalid());
        }
        ItemWrite::Upsert(env.to_json().map_err(|e| ApiError::from_core(&e))?)
    };
    let now = s.clock.now();
    let path = format!("/v1/items/{id}");
    let reply = s.store.idempotent(&format!("d:{device}"), &key, &request_hash("PUT", &path, &body), now, s.cfg.idempotency_ttl_secs, |tx| {
        Store::put_item_tx(tx, &id, base_seq, &write, now)
    })?;
    // The write and its idempotency record are committed; the answer has not been sent yet
    crate::crash::crash_point("after_commit_before_response");
    Ok(finish_write(&s, reply))
}

async fn list_items(State(s): St, headers: HeaderMap, RawQuery(q): RawQuery) -> Result<Response, ApiError> {
    let (device, _) = auth(&s, &headers)?;
    let (mut after, mut limit) = (0i64, 200i64);
    for pair in q.unwrap_or_default().split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let n: i64 = v.parse().map_err(|_| ApiError::invalid())?;
        match k {
            "after" if n >= 0 => after = n,
            "limit" if (1..=500).contains(&n) => limit = n,
            _ => return Err(ApiError::invalid()),
        }
    }
    let page = s.store.list_items(&device, after, limit)?;
    Ok(json_response(200, page.to_string().into_bytes()))
}

async fn events(State(s): St, headers: HeaderMap) -> Result<Response, ApiError> {
    auth(&s, &headers)?;
    let rx = s.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|m| m.ok()).map(|seq| Ok::<_, Infallible>(Event::default().data(json!({ "seq": seq }).to_string())));
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(25)).text("")).into_response())
}

/// Builds the connection code from the address, the certificate fingerprint and a registration credential.
pub fn connection_code(public_url: &str, fingerprint: &str, reg_token: &str) -> String {
    let body = json!({ "u": public_url, "fp": fingerprint, "t": reg_token }).to_string();
    format!("onepaste1:{}", b64url_encode(body.as_bytes()))
}

