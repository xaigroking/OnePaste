//! The devices page: the list with readable names, removing a device, and making a connection code for a new one.

use onepaste_app::*;
use onepaste_server::config::Config;
use onepaste_server::Server;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PASS: &str = "device page test passphrase";

fn temp_dir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-devices-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn quiet() -> AppConfig {
    AppConfig { listen_for_changes: false, poll_interval: Duration::from_secs(3600), background_sync: false }
}

fn desktop(name: &str) -> App {
    App::new(temp_dir(name), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), quiet())
}

async fn server_and_first() -> (Server, App) {
    let dir = temp_dir("srv");
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut cfg = Config::new(dir.join("srv"));
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
    cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
    let server = Server::start(cfg).await.unwrap();
    let first = desktop("first");
    first.create_vault(&server.first_connect_code.clone().unwrap(), PASS, "Kitchen laptop").await.unwrap();
    first.confirm_kit_saved().await.unwrap();
    (server, first)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_device_list_shows_every_device_with_its_name_and_marks_this_one() {
    let (server, first) = server_and_first().await;
    let second = desktop("second");
    second.join(&first.new_connection_code().await.unwrap(), PASS, "Office desktop").await.unwrap();
    let list = first.devices().await.unwrap();
    assert!(list.server_url.starts_with("https://127.0.0.1:"), "{}", list.server_url);
    assert_eq!(list.devices.len(), 2);
    let mine = list.devices.iter().find(|d| d.this_device).expect("this device is marked");
    assert_eq!(mine.name.as_deref(), Some("Kitchen laptop"));
    let other = list.devices.iter().find(|d| !d.this_device).unwrap();
    assert_eq!(other.name.as_deref(), Some("Office desktop"));
    assert!(!other.revoked && other.created_at > 0);
    // The other device's view marks itself, not the first
    let again = second.devices().await.unwrap();
    assert_eq!(again.devices.iter().find(|d| d.this_device).unwrap().name.as_deref(), Some("Office desktop"));
    first.shutdown();
    second.shutdown();
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_device_stops_it_and_this_device_cannot_remove_itself() {
    let (server, first) = server_and_first().await;
    let second = desktop("second");
    second.join(&first.new_connection_code().await.unwrap(), PASS, "Office desktop").await.unwrap();
    let list = first.devices().await.unwrap();
    let me = list.devices.iter().find(|d| d.this_device).unwrap().device_id.clone();
    let other = list.devices.iter().find(|d| !d.this_device).unwrap().device_id.clone();
    assert!(matches!(first.revoke_device(&me).await, Err(AppError::CannotRevokeThisDevice)));
    first.revoke_device(&other).await.unwrap();
    let after = first.devices().await.unwrap();
    assert!(after.devices.iter().find(|d| d.device_id == other).unwrap().revoked);
    assert!(!after.devices.iter().find(|d| d.device_id == me).unwrap().revoked);
    // The removed device finds out the next time it syncs
    assert!(matches!(second.sync_now().await, Err(AppError::Revoked)));
    first.shutdown();
    second.shutdown();
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_connection_code_adds_one_more_device() {
    let (server, first) = server_and_first().await;
    let code = first.new_connection_code().await.unwrap();
    assert!(code.starts_with("onepaste1:"));
    let third = desktop("third");
    third.join(&code, PASS, "Phone").await.unwrap();
    assert_eq!(first.devices().await.unwrap().devices.len(), 2);
    first.shutdown();
    third.shutdown();
    drop(server);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_local_only_computer_has_no_devices() {
    let local = desktop("local");
    local.choose_local_only().await.unwrap();
    assert!(matches!(local.devices().await, Err(AppError::NotJoined)));
    assert!(matches!(local.new_connection_code().await, Err(AppError::NotJoined)));
    assert!(matches!(local.revoke_device("x").await, Err(AppError::NotJoined)));
    local.shutdown();
}
