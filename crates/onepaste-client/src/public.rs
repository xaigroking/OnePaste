//! Plain HTTPS requests to public servers (the update check): the system's trusted roots, no cookies, no identifiers.
//! A test can pin one certificate instead, to talk to a local server.

use crate::error::ClientError;
use crate::pin::pinned_client;
use std::time::Duration;

/// The largest answer that is read. Update files are small; anything bigger is refused.
const MAX_BYTES: usize = 2 * 1024 * 1024;

/// A client for public HTTPS servers. With a fingerprint it trusts exactly that certificate (for tests).
pub fn public_client(pinned_fingerprint: Option<&str>) -> Result<reqwest::Client, ClientError> {
    if let Some(fp) = pinned_fingerprint {
        return Ok(pinned_client(fp));
    }
    let mut roots = rustls::RootCertStore::empty();
    let loaded = rustls_native_certs::load_native_certs();
    for cert in loaded.certs {
        let _ = roots.add(cert);
    }
    if roots.is_empty() {
        return Err(ClientError::Other("no trusted root certificates are available".into()));
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| ClientError::Other("tls".into()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(cfg)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| ClientError::Other("client".into()))
}

/// One GET. The only header added is the user agent the caller gives; nothing else identifies the device or the user.
/// Returns the status and the body (at most 2 MiB).
pub async fn get(client: &reqwest::Client, url: &str, user_agent: &str, accept: &str) -> Result<(u16, Vec<u8>), ClientError> {
    let mut resp = client
        .get(url)
        .header("User-Agent", user_agent)
        .header("Accept", accept)
        .send()
        .await
        .map_err(|e| ClientError::Transport(if e.is_timeout() { "timeout" } else if e.is_connect() { "connect" } else { "request" }.to_string()))?;
    let status = resp.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| ClientError::Transport("request".into()))? {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(ClientError::Other("the answer is too large".into()));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}
