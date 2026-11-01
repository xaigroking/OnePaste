use super::*;

#[tokio::test]
async fn shutdown_waits_for_pending_tls_connections_to_release_the_store() {
    let dir = std::env::temp_dir().join(format!("onepaste-shutdown-{}", onepaste_core::id::new_id().unwrap()));
    let mut cfg = Config::new(dir.clone());
    cfg.listen = "127.0.0.1:0".parse().unwrap();
    cfg.public_url = Some("https://127.0.0.1".into());
    let server = Server::start(cfg).await.unwrap();
    let store_owner = Arc::downgrade(&server.state);
    let handle = server.handle.clone();
    // A TCP peer that has not sent a TLS hello keeps the accepted service and its database alive.
    let socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while handle.connection_count() == 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    server.shutdown().await;
    assert_eq!(handle.connection_count(), 0, "shutdown returned with an accepted connection still running");
    assert!(store_owner.upgrade().is_none(), "the database owner must be released before shutdown returns");
    drop(socket);
    std::fs::remove_dir_all(dir).unwrap();
}
