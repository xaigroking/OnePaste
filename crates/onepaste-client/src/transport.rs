//! Sends the engine's requests over HTTPS and listens for change notifications.

use crate::error::ClientError;
use crate::pin::pinned_client;
use onepaste_sync::{HttpRequest, HttpResponse, Method};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[cfg(feature = "fault-injection")]
static FAULT_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many requests this process has started so far (to any server). Local-only mode must leave it unchanged.
pub fn requests_started() -> u64 {
    REQUESTS.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Transport {
    http: reqwest::Client,
    base: String,
}

impl Transport {
    pub fn new(base_url: &str, fingerprint: &str) -> Transport {
        Transport { http: pinned_client(fingerprint), base: base_url.trim_end_matches('/').to_string() }
    }

    async fn send_inner(&self, req: &HttpRequest) -> Result<HttpResponse, ClientError> {
        REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let method = match req.method {
            Method::Get => reqwest::Method::GET,
            Method::Put => reqwest::Method::PUT,
            Method::Post => reqwest::Method::POST,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut rb = self.http.request(method, format!("{}{}", self.base, req.path_and_query));
        for (k, v) in &req.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        if !req.body.is_empty() {
            rb = rb.body(req.body.clone());
        }
        let resp = rb.send().await.map_err(transport_error)?;
        let status = resp.status().as_u16();
        let headers = resp.headers().iter().filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string()))).collect();
        let body = resp.bytes().await.map_err(transport_error)?.to_vec();
        Ok(HttpResponse { status, headers, body })
    }

    /// Sends one request. Any failure to get a complete answer is a `Transport` error.
    pub async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, ClientError> {
        #[cfg(feature = "fault-injection")]
        {
            use std::sync::atomic::Ordering;
            let armed = std::env::var("ONEPASTE_FAULT").as_deref() == Ok("drop_response_once");
            if armed && req.method == Method::Put && req.path_and_query.starts_with("/v1/items/") && !FAULT_FIRED.swap(true, Ordering::SeqCst) {
                // The server handles the request, but the answer is thrown away
                let _ = self.send_inner(req).await;
                return Err(ClientError::Transport("injected fault".into()));
            }
        }
        self.send_inner(req).await
    }

    pub async fn open_events(&self, token: &str) -> Result<reqwest::Response, ClientError> {
        REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resp = self
            .http
            .get(format!("{}/v1/events", self.base))
            .timeout(Duration::from_secs(24 * 3600))
            .header("OnePaste-Proto", "1")
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(transport_error)?;
        if resp.status().as_u16() != 200 {
            return Err(ClientError::Server { status: resp.status().as_u16(), code: "events_refused".into() });
        }
        Ok(resp)
    }
}

/// Only a short category is kept: reqwest errors can include URLs.
fn transport_error(e: reqwest::Error) -> ClientError {
    let kind = if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else {
        "request"
    };
    ClientError::Transport(kind.to_string())
}

/// Pulls the sequence numbers out of the event stream text.
pub fn parse_event_seqs(buf: &mut String) -> Vec<i64> {
    let mut out = Vec::new();
    while let Some(end) = buf.find('\n') {
        let line: String = buf.drain(..=end).collect();
        if let Some(data) = line.trim_end().strip_prefix("data:") {
            if let Some(seq) = serde_json::from_str::<serde_json::Value>(data.trim()).ok().and_then(|v| v.get("seq")?.as_i64()) {
                out.push(seq);
            }
        }
    }
    out
}

/// Keeps a notification stream open for as long as the receiver lives, reconnecting with a growing delay (at most 60
/// seconds). `Some(seq)` is a change notification; `None` means the stream was re-established. A notification is only
/// a hint to pull.
pub async fn event_loop(transport: Transport, token: String, tx: UnboundedSender<Option<i64>>) {
    let mut delay = 1u64;
    loop {
        if let Ok(mut resp) = transport.open_events(&token).await {
            delay = 1;
            if tx.send(None).is_err() {
                return;
            }
            let mut buf = String::new();
            while let Ok(Some(chunk)) = resp.chunk().await {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                for seq in parse_event_seqs(&mut buf) {
                    if tx.send(Some(seq)).is_err() {
                        return;
                    }
                }
            }
        }
        if tx.is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(60);
    }
}
