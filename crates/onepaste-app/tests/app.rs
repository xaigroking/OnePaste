//! Tests of the application logic against a real server (in this process), real client and history databases, an
//! in-memory secret store and a fake clipboard. No window is involved.

use onepaste_app::*;
use onepaste_client::{join, now_ms, JoinOptions, Session};
use onepaste_core::crypto::Key;
use onepaste_core::envelope::ItemPayload;
use onepaste_server::config::Config;
use onepaste_server::Server;
use onepaste_sync::Method;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PASS: &str = "a long test passphrase";
const ZH2: &str = "\u{4e2d}\u{6587}";

fn temp_dir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-app-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn server_config(dir: &Path, port: u16) -> Config {
    let mut cfg = Config::new(dir.join("srv"));
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
    cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
    cfg
}

/// A server, a first device standing in for the command line client, and helpers to build desktop apps.
struct World {
    dir: PathBuf,
    port: u16,
    server: Option<Server>,
    cli: Session,
}

impl World {
    /// A server with a vault whose first device is `cli`.
    async fn new() -> World {
        let dir = temp_dir("world");
        let port = free_port();
        let server = Server::start(server_config(&dir, port)).await.unwrap();
        let code = server.first_connect_code.clone().unwrap();
        let first = join(
            &dir.join("cli"),
            JoinOptions { code, device_name: "cli".into(), passphrase: Some(PASS.into()), recovery_kit_out: None, existing_only: false },
        )
        .await
        .unwrap();
        assert!(first.new_vault);
        let cli = Session::open(&dir.join("cli"), PASS).unwrap();
        World { dir, port, server: Some(server), cli }
    }

    async fn invite(&self) -> String {
        self.cli.invite().await.unwrap()
    }

    async fn app(&mut self, name: &str, cfg: AppConfig) -> Desktop {
        let dir = self.dir.join(name);
        let secrets = Arc::new(MemorySecretStore::new());
        let clipboard = Arc::new(FakeClipboard::new());
        let app = App::new(dir.clone(), secrets.clone(), clipboard.clone(), cfg);
        let code = self.invite().await;
        app.join(&code, PASS, name).await.unwrap();
        Desktop { app, secrets, clipboard, dir }
    }

    /// The command line client adds a text item and uploads it.
    async fn cli_adds(&mut self, text: &str) -> String {
        let p = ItemPayload { kind: "text".into(), content: Some(text.into()), ..Default::default() };
        let id = self.cli.engine.enqueue_create(now_ms(), &p).unwrap();
        self.cli_syncs().await;
        id
    }

    async fn cli_syncs(&mut self) {
        self.cli.engine.request_pull();
        self.cli.run(false).await.unwrap();
    }

    fn cli_texts(&mut self) -> Vec<String> {
        self.cli.engine.list_items().unwrap().into_iter().filter(|i| !i.deleted).map(|i| i.first_line).collect()
    }

    fn cli_live_count(&mut self) -> usize {
        self.cli.engine.list_items().unwrap().into_iter().filter(|i| !i.deleted).count()
    }
}

struct Desktop {
    app: App,
    secrets: Arc<MemorySecretStore>,
    clipboard: Arc<FakeClipboard>,
    dir: PathBuf,
}

fn quiet() -> AppConfig {
    AppConfig { listen_for_changes: false, poll_interval: Duration::from_secs(3600), background_sync: false }
}

fn text(s: &str) -> Content {
    Content::Text(s.into())
}

fn png(n: usize) -> Content {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend(std::iter::repeat(7u8).take(n));
    Content::Image { png: bytes, width: 4, height: 3 }
}

async fn wait_for<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..150 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}

// ---- 1. joining ---------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn joining_stores_the_key_and_a_restart_needs_no_passphrase() {
    let mut w = World::new().await;
    let d = w.app("desktop", quiet()).await;
    assert!(d.secrets.load().unwrap().is_some(), "the master key is in the secure store");
    assert!(d.app.is_joined());
    d.app.shutdown();
    // A new application object with the same data directory and secure store opens the vault with no passphrase
    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert!(again.start().await.unwrap());
    assert!(again.status().await.joined);
    // Without a key in the secure store nothing opens
    let empty = App::new(d.dir.clone(), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), quiet());
    assert!(!empty.start().await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_passphrase_registers_nothing_and_stores_no_key() {
    let w = World::new().await;
    let devices_before = w.cli.call(Method::Get, "/v1/devices", None).await.unwrap().as_array().unwrap().len();
    let secrets = Arc::new(MemorySecretStore::new());
    let app = App::new(w.dir.join("wrong"), secrets.clone(), Arc::new(FakeClipboard::new()), quiet());
    let code = w.invite().await;
    let err = app.join(&code, "not the passphrase", "desktop").await.unwrap_err();
    assert_eq!(err.code(), "wrong_passphrase");
    assert!(secrets.load().unwrap().is_none());
    assert!(!app.is_joined());
    assert_eq!(w.cli.call(Method::Get, "/v1/devices", None).await.unwrap().as_array().unwrap().len(), devices_before);
    // The same credential still works with the right passphrase
    app.join(&code, PASS, "desktop").await.unwrap();
    assert!(secrets.load().unwrap().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn joining_never_creates_a_vault() {
    let dir = temp_dir("empty");
    let port = free_port();
    let server = Server::start(server_config(&dir, port)).await.unwrap();
    let secrets = Arc::new(MemorySecretStore::new());
    let app = App::new(dir.join("desktop"), secrets.clone(), Arc::new(FakeClipboard::new()), quiet());
    let err = app.join(server.first_connect_code.as_deref().unwrap(), PASS, "desktop").await.unwrap_err();
    assert_eq!(err.code(), "vault_empty");
    assert!(secrets.load().unwrap().is_none());
}

// ---- 2. capturing -------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_copied_text_is_queued_and_reaches_the_other_device() {
    let mut w = World::new().await;
    let d = w.app("desktop", quiet()).await;
    d.app.on_clip_event(ClipEvent::Captured(text("copied on the desktop"))).await;
    assert_eq!(d.app.status().await.pending, 1);
    d.app.sync_now().await.unwrap();
    assert_eq!(d.app.status().await.pending, 0);
    w.cli_syncs().await;
    assert_eq!(w.cli_texts(), vec!["copied on the desktop"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copied_image_is_queued_with_its_metadata_and_a_huge_one_is_not() {
    let mut w = World::new().await;
    let d = w.app("desktop", quiet()).await;
    d.app.on_clip_event(ClipEvent::Captured(png(2000))).await;
    d.app.sync_now().await.unwrap();
    w.cli_syncs().await;
    let items = w.cli.engine.list_items().unwrap();
    assert_eq!(items.len(), 1);
    let p = w.cli.engine.get_item(&items[0].id).unwrap();
    assert_eq!(p.kind, "image");
    let get = |k: &str| p.extra.get(k).cloned().unwrap();
    assert_eq!((get("width"), get("height"), get("mime"), get("size")), (4.into(), 3.into(), "image/png".into(), 2008.into()));
    // Above what the server accepts: not queued, and the event log says so
    d.app.on_clip_event(ClipEvent::Captured(png(7 * 1024 * 1024))).await;
    assert_eq!(d.app.status().await.pending, 0);
    assert!(d.app.event_log().iter().any(|e| e.kind == "image_too_large"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_same_content_twice_in_a_row_is_queued_once_and_sensitive_or_own_writes_not_at_all() {
    let mut w = World::new().await;
    let d = w.app("desktop", quiet()).await;
    d.app.on_clip_event(ClipEvent::Captured(text("same"))).await;
    d.app.on_clip_event(ClipEvent::Captured(text("same"))).await;
    assert_eq!(d.app.status().await.pending, 1, "a repeat of the latest content is not queued again");
    d.app.on_clip_event(ClipEvent::Captured(text("different"))).await;
    d.app.on_clip_event(ClipEvent::Captured(text("same"))).await;
    assert_eq!(d.app.status().await.pending, 3, "the same text after something else is a new copy");
    d.app.on_clip_event(ClipEvent::SkippedSensitive).await;
    d.app.on_clip_event(ClipEvent::SkippedSelf).await;
    assert_eq!(d.app.status().await.pending, 3);
    let kinds: Vec<String> = d.app.event_log().into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"skipped_sensitive".to_string()) && kinds.contains(&"skipped_self".to_string()));
    // Recording can be switched off
    let mut s = d.app.settings();
    s.record_clipboard = false;
    d.app.set_settings(s).unwrap();
    d.app.on_clip_event(ClipEvent::Captured(text("not recorded"))).await;
    assert_eq!(d.app.status().await.pending, 3);
    drop(w);
}

// ---- 3. applying what arrives from other devices ------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn only_news_is_written_to_the_clipboard_and_only_the_newest_one_marked_as_our_own() {
    let mut w = World::new().await;
    w.cli_adds("old item one").await;
    w.cli_adds("old item two").await;
    let d = w.app("desktop", quiet()).await;
    // The first pull of a new device brings history, not news
    d.app.sync_now().await.unwrap();
    assert!(d.clipboard.writes().is_empty(), "the first full pull must not touch the clipboard");
    assert_eq!(d.app.status().await.items, 2);
    // Several new items in one pull: only the newest is written
    w.cli_adds("news one").await;
    w.cli_adds("news two").await;
    w.cli_adds("news three").await;
    d.app.sync_now().await.unwrap();
    let writes = d.clipboard.writes();
    assert_eq!(writes, vec![(text("news three"), true)], "one write, the newest, with the origin marker");
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_written_when_the_setting_is_off_for_own_items_or_for_deletions() {
    let mut w = World::new().await;
    w.cli_adds("seed").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    // The setting is off
    let mut s = d.app.settings();
    s.auto_apply_remote = false;
    d.app.set_settings(s).unwrap();
    w.cli_adds("while the setting is off").await;
    d.app.sync_now().await.unwrap();
    assert!(d.clipboard.writes().is_empty());
    s.auto_apply_remote = true;
    d.app.set_settings(s).unwrap();
    // This device's own item comes back from the server: not written
    d.app.on_clip_event(ClipEvent::Captured(text("my own copy"))).await;
    d.app.sync_now().await.unwrap();
    d.app.sync_now().await.unwrap();
    assert!(d.clipboard.writes().is_empty(), "an item made on this device is never applied back");
    // A deletion made elsewhere is not written
    let id = w.cli_adds("to be deleted").await;
    d.app.sync_now().await.unwrap();
    let before = d.clipboard.writes().len();
    w.cli.engine.enqueue_delete(now_ms(), &id).unwrap();
    w.cli_syncs().await;
    d.app.sync_now().await.unwrap();
    assert_eq!(d.clipboard.writes().len(), before, "a tombstone is not applied");
}

// ---- 4 and 5. no feedback loop, and copying from the history ------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_applied_item_does_not_come_back_as_a_new_one() {
    let mut w = World::new().await;
    w.cli_adds("seed").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    w.cli_adds("travels to the desktop").await;
    d.app.sync_now().await.unwrap();
    assert_eq!(d.clipboard.writes().len(), 1);
    // The clipboard monitor reports the write as the application's own; give it time to be handled
    wait_for("the own write to be reported", || async { d.app.event_log().iter().any(|e| e.kind == "skipped_self") }).await;
    assert_eq!(d.app.status().await.pending, 0, "nothing was queued from the application's own write");
    d.app.sync_now().await.unwrap();
    w.cli_syncs().await;
    assert_eq!(w.cli_live_count(), 2, "the server holds exactly the two items that were ever created");
}

#[tokio::test(flavor = "multi_thread")]
async fn clicking_a_history_item_puts_it_back_on_the_clipboard_without_a_new_item() {
    let mut w = World::new().await;
    let id = w.cli_adds("click me").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    d.app.copy(&id).await.unwrap();
    assert_eq!(d.clipboard.current(), Some(text("click me")));
    assert_eq!(d.clipboard.writes().last().map(|w| w.1), Some(true), "with the origin marker");
    wait_for("the own write to be reported", || async { d.app.event_log().iter().any(|e| e.kind == "skipped_self") }).await;
    assert_eq!(d.app.status().await.pending, 0);
    assert_eq!(d.app.status().await.items, 1);
}

// ---- 6. history and search ----------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_history_follows_every_sync_and_finds_both_synced_and_just_copied_items() {
    let mut w = World::new().await;
    w.cli_adds(&format!("a note about {ZH2} today")).await;
    w.cli_adds("quarterly invoice").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    let found = |hits: &Vec<ItemView>| hits.iter().map(|h| h.preview.clone()).collect::<Vec<_>>();
    assert_eq!(found(&d.app.search(ZH2, Filter::All, 10, 0).await.unwrap()), vec![format!("a note about {ZH2} today")], "a two-character Chinese word");
    assert_eq!(found(&d.app.search("invoice", Filter::All, 10, 0).await.unwrap()), vec!["quarterly invoice"]);
    // Copied here a moment ago and not synced yet: found at once
    d.app.on_clip_event(ClipEvent::Captured(text("copied right now"))).await;
    assert_eq!(found(&d.app.search("right now", Filter::All, 10, 0).await.unwrap()), vec!["copied right now"]);
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap().len(), 3);
    assert_eq!(d.app.search("", Filter::Images, 10, 0).await.unwrap().len(), 0);
}

// ---- 7. changes travel back to the other device -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn flags_and_deletions_made_on_the_desktop_reach_the_other_device() {
    let mut w = World::new().await;
    let keep = w.cli_adds("keep this").await;
    let drop_id = w.cli_adds("remove this").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    d.app.set_flags(&keep, Some(true), Some(true), Some("remember".into())).await.unwrap();
    d.app.delete(&drop_id).await.unwrap();
    d.app.sync_now().await.unwrap();
    w.cli_syncs().await;
    let items = w.cli.engine.list_items().unwrap();
    let kept = items.iter().find(|i| i.id == keep).unwrap();
    assert!(kept.favorite && kept.pinned);
    assert_eq!(kept.note.as_deref(), Some("remember"));
    assert!(items.iter().find(|i| i.id == drop_id).unwrap().deleted);
    assert!(d.app.search("remove this", Filter::All, 10, 0).await.unwrap().is_empty(), "gone from the desktop's search too");
}

// ---- 8. wake and network ------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn waking_from_sleep_and_the_network_coming_back_sync_at_once() {
    let mut w = World::new().await;
    let cfg = AppConfig { listen_for_changes: false, poll_interval: Duration::from_secs(3600), background_sync: true };
    let d = w.app("desktop", cfg).await;
    // Let the first round finish so that later rounds are only started by the events below
    wait_for("the first round", || async { d.app.status().await.items == 0 && d.app.event_log().iter().all(|e| e.kind != "sync_paused") && d.app.status().await.joined }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    w.cli_adds("written while the desktop slept").await;
    d.app.notify_wake();
    wait_for("the item after a wake", || async { d.app.status().await.items == 1 }).await;
    w.cli_adds("written while the network was down").await;
    d.app.notify_network(false);
    d.app.notify_network(true);
    wait_for("the item after the network returned", || async { d.app.status().await.items == 2 }).await;
    let kinds: Vec<String> = d.app.event_log().into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"woke_from_sleep".to_string()) && kinds.contains(&"network_online".to_string()));
    d.app.shutdown();
}

// ---- 9. revoked and rolled back -----------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_revoked_device_stops_with_a_clear_state_and_fetches_nothing_more() {
    let mut w = World::new().await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    let devices = w.cli.call(Method::Get, "/v1/devices", None).await.unwrap();
    let mine = devices.as_array().unwrap().iter().find(|x| x["device_id"].as_str() != Some(&w.cli.info.device_id)).unwrap()["device_id"].as_str().unwrap().to_string();
    w.cli.call(Method::Delete, &format!("/v1/devices/{mine}"), None).await.unwrap();
    w.cli_adds("added after the revocation").await;
    assert_eq!(d.app.sync_now().await.unwrap_err().code(), "revoked");
    let st = d.app.status().await;
    assert_eq!((st.tray, st.paused.as_deref()), (TrayState::Error, Some("revoked")));
    // Paused for good: a further attempt fails the same way and the new item is not fetched
    assert_eq!(d.app.sync_now().await.unwrap_err().code(), "revoked");
    assert_eq!(d.app.status().await.items, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_went_back_in_time_stops_the_sync_with_a_rollback_state() {
    let mut w = World::new().await;
    w.cli_adds("first").await;
    let d = w.app("desktop", quiet()).await;
    d.app.sync_now().await.unwrap();
    // Take a copy of the server's data now, then let more happen
    w.server.take().unwrap().shutdown().await;
    let backup = w.dir.join("srv-backup");
    copy_dir(&w.dir.join("srv"), &backup);
    w.server = Some(Server::start(server_config(&w.dir, w.port)).await.unwrap());
    w.cli_adds("second").await;
    w.cli_adds("third").await;
    d.app.sync_now().await.unwrap();
    assert_eq!(d.app.status().await.items, 3);
    // The server comes back with the old data
    w.server.take().unwrap().shutdown().await;
    std::fs::remove_dir_all(w.dir.join("srv")).unwrap();
    copy_dir(&backup, &w.dir.join("srv"));
    w.server = Some(Server::start(server_config(&w.dir, w.port)).await.unwrap());
    assert_eq!(d.app.sync_now().await.unwrap_err().code(), "server_rollback");
    let st = d.app.status().await;
    assert_eq!((st.tray, st.paused.as_deref()), (TrayState::Error, Some("server_rollback")));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

// ---- 10. nothing sensitive in logs, diagnostics or files ----------------------------------------------------------

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(files_under(&p)) } else { out.push(p) }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_diagnostics_and_files_hold_neither_content_nor_the_master_key() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let mut w = World::new().await;
    let marker = format!("LEAK-APP-{}", now_ms());
    let d = w.app("desktop", quiet()).await;
    d.app.on_clip_event(ClipEvent::Captured(text(&format!("copied {marker}")))).await;
    d.app.on_clip_event(ClipEvent::Captured(png(300))).await;
    d.app.sync_now().await.unwrap();
    let first = d.app.search(&marker, Filter::All, 5, 0).await.unwrap();
    d.app.set_flags(&first[0].item_id, None, None, Some(format!("note {marker}"))).await.unwrap();
    w.cli_adds(&format!("from the other device {marker}")).await;
    d.app.sync_now().await.unwrap();
    // The event log and the diagnostics
    let log = serde_json::to_string(&d.app.event_log()).unwrap();
    let diag = d.app.diagnostics().await;
    for (name, text) in [("event log", &log), ("diagnostics", &diag)] {
        assert!(!text.contains(&marker), "{name} holds content");
    }
    assert!(log.contains("captured") && diag.contains("recent events"), "they are not empty");
    // The data directory: no plaintext, and the master key in no form at all
    let mk: Key = d.secrets.load().unwrap().unwrap();
    let hex: String = mk.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let needles: Vec<(String, Vec<u8>)> = vec![
        ("marker".into(), marker.clone().into_bytes()),
        ("key bytes".into(), mk.as_bytes().to_vec()),
        ("key hex".into(), hex.clone().into_bytes()),
        ("key base64".into(), B64.encode(mk.as_bytes()).into_bytes()),
        ("passphrase".into(), PASS.as_bytes().to_vec()),
    ];
    let mut scanned = 0;
    for f in files_under(&d.dir) {
        let data = std::fs::read(&f).unwrap();
        scanned += 1;
        for (what, n) in &needles {
            assert!(!data.windows(n.len()).any(|win| win == n.as_slice()), "{what} found in {f:?}");
        }
    }
    assert!(scanned >= 3, "client database, history database and settings were scanned ({scanned} files)");
}

// ---- the desktop shell runs these on its own task runtime ----------------------------------------------------------

/// The shell cannot be built on every machine that runs these tests, so what it needs from this crate is checked here at
/// compile time: every command future can be moved to another thread, and the application object is shared freely.
#[test]
fn the_futures_of_the_commands_can_run_on_any_thread() {
    fn send<T: Send>(_: &T) {}
    fn shared<T: Send + Sync + Clone + 'static>() {}
    shared::<App>();
    let app = App::new(temp_dir("send"), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), quiet());
    send(&app.start());
    send(&app.join("", "", ""));
    send(&app.status());
    send(&app.search("", Filter::All, 1, 0));
    send(&app.copy(""));
    send(&app.set_flags("", None, None, None));
    send(&app.delete(""));
    send(&app.diagnostics());
    send(&app.sync_now());
    send(&app.on_clip_event(ClipEvent::SkippedSelf));
}
