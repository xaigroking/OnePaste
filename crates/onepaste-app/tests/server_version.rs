//! A server that speaks another protocol version must be reported, not ignored.

use onepaste_app::*;
use onepaste_client::ConnectionCode;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn a_server_with_another_protocol_version_is_reported_when_connecting() {
    let dir = std::env::temp_dir().join(format!("onepaste-protocol-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let material = onepaste_server::tls::load_or_create(&dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    // What the real server answers to a request whose protocol version it does not support
    let app = axum::Router::new().fallback(|| async {
        (
            axum::http::StatusCode::BAD_REQUEST,
            [("content-type", "application/json"), ("onepaste-proto", "2")],
            r#"{"code":"unsupported_version","min":2,"max":2}"#,
        )
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&material.cert_path, &material.key_path).await.unwrap();
    tokio::spawn(axum_server::bind_rustls(format!("127.0.0.1:{port}").parse::<std::net::SocketAddr>().unwrap(), tls).serve(app.into_make_service()));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let code = ConnectionCode { u: format!("https://127.0.0.1:{port}"), fp: material.fingerprint.clone(), t: "A".repeat(43) }.encode();
    let desktop = App::new(dir.join("app"), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), AppConfig::default());
    let err = desktop.probe_server(&code).await.unwrap_err();
    assert_eq!(err.code(), "unsupported_version", "the screens get a code they can explain: {err:?}");
    let err = desktop.join(&code, "a passphrase", "x").await.unwrap_err();
    assert_eq!(err.code(), "unsupported_version");
}
