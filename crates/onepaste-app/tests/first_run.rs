//! The first-run flow against a real server (in this process): choosing how to use the program, creating a new vault,
//! the recovery kit and joining with it. The secure store is in memory and the clipboard is a fake.

use onepaste_app::*;
use onepaste_client::{join, now_ms, JoinOptions, Session};
use onepaste_core::crypto::Key;
use onepaste_core::recovery;
use onepaste_server::config::Config;
use onepaste_server::store::Store;
use onepaste_server::Server;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const NEW_PASS: &str = "PASSMARK-first-run-passphrase";

fn temp_dir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-first-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn quiet() -> AppConfig {
    AppConfig { listen_for_changes: false, poll_interval: Duration::from_secs(3600), background_sync: false, ..AppConfig::default() }
}

/// A server that has no vault yet.
struct EmptyServer {
    dir: PathBuf,
    port: u16,
    server: Option<Server>,
}

fn config(dir: &Path, port: u16) -> Config {
    let mut cfg = Config::new(dir.join("srv"));
    cfg.listen = format!("127.0.0.1:{port}").parse().unwrap();
    cfg.public_url = Some(format!("https://127.0.0.1:{port}"));
    cfg
}

impl EmptyServer {
    async fn new() -> EmptyServer {
        let dir = temp_dir("srv");
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let server = Server::start(config(&dir, port)).await.unwrap();
        EmptyServer { dir, port, server: Some(server) }
    }

    async fn stop(&mut self) {
        self.server.take().unwrap().shutdown().await;
    }

    async fn start_again(&mut self) {
        self.server = Some(Server::start(config(&self.dir, self.port)).await.unwrap());
    }

    fn code(&self) -> String {
        self.server.as_ref().unwrap().first_connect_code.clone().unwrap()
    }

    fn new_code(&self) -> String {
        self.server.as_ref().unwrap().new_connect_code().unwrap()
    }

    /// Devices the server knows, read from its own database.
    fn devices(&self) -> i64 {
        Store::open(&self.dir.join("srv")).unwrap().device_count().unwrap()
    }
}

struct Desktop {
    app: App,
    secrets: Arc<MemorySecretStore>,
    clipboard: Arc<FakeClipboard>,
    dir: PathBuf,
}

fn desktop(name: &str) -> Desktop {
    let dir = temp_dir(name);
    let secrets = Arc::new(MemorySecretStore::new());
    let clipboard = Arc::new(FakeClipboard::new());
    Desktop { app: App::new(dir.clone(), secrets.clone(), clipboard.clone(), quiet()), secrets, clipboard, dir }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files_under(&p))
        } else {
            out.push(p)
        }
    }
    out
}

/// Creates a vault the way the screens do and confirms it. Returns the kit.
async fn create_and_confirm(d: &Desktop, s: &EmptyServer) -> String {
    let kit = d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    d.app.confirm_kit_saved().await.unwrap();
    kit
}

// ---- 1 and 2 and 3: a new vault -----------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn probing_an_empty_server_says_new_and_registers_no_device() {
    let s = EmptyServer::new().await;
    let d = desktop("probe");
    assert_eq!(d.app.first_run_state().await.unwrap(), Mode::Unset);
    assert_eq!(d.app.probe_server(&s.code()).await.unwrap(), ServerVault::Empty);
    assert_eq!(s.devices(), 0, "probing registers nothing");
    assert!(d.secrets.load().unwrap().is_none());
    assert!(!d.app.is_joined());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_vault_is_registered_only_after_the_kit_is_confirmed() {
    let s = EmptyServer::new().await;
    let d = desktop("create");
    let kit = d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    // The kit is on screen; nothing exists anywhere yet
    assert_eq!(s.devices(), 0, "no device before the confirmation");
    assert!(d.secrets.load().unwrap().is_none(), "no key before the confirmation");
    assert!(!d.dir.join("onepaste.db").exists() || !d.app.is_joined());
    assert_eq!(d.app.mode(), Mode::Unset);
    d.app.confirm_kit_saved().await.unwrap();
    assert_eq!(s.devices(), 1, "the device exists after the confirmation");
    assert_eq!(d.app.mode(), Mode::Synced);
    let stored: Key = d.secrets.load().unwrap().expect("the key is in the secure store");
    let from_kit = recovery::decode(&kit).unwrap();
    assert_eq!(stored.as_bytes(), from_kit.as_bytes(), "the kit holds the key that the secure store holds");
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_before_the_confirmation_leaves_nothing_behind() {
    let s = EmptyServer::new().await;
    let d = desktop("leave");
    d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    // The window is closed: a new application object over the same directory and secure store
    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert_eq!(again.first_run_state().await.unwrap(), Mode::Unset);
    assert_eq!(s.devices(), 0, "the server still has no vault");
    assert!(d.secrets.load().unwrap().is_none(), "no key was stored");
    assert!(matches!(again.confirm_kit_saved().await, Err(AppError::NoPendingSetup)), "there is nothing to confirm after a restart");
    // Going back also drops it
    d.app.cancel_setup();
    assert!(matches!(d.app.confirm_kit_saved().await, Err(AppError::NoPendingSetup)));
    assert_eq!(s.devices(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_registration_keeps_the_pending_vault_and_stores_nothing() {
    let mut s = EmptyServer::new().await;
    let d = desktop("retry");
    d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    s.stop().await;
    assert!(matches!(d.app.confirm_kit_saved().await, Err(AppError::Unreachable)));
    // Still pending: trying again is a network failure again, not "nothing to confirm"
    assert!(matches!(d.app.confirm_kit_saved().await, Err(AppError::Unreachable)));
    assert!(d.secrets.load().unwrap().is_none(), "nothing is stored after a failure");
    assert_eq!(d.app.mode(), Mode::Unset);
    // A server that starts without any device makes a new connection code and drops the old one: the old code is refused,
    // and still nothing is stored
    s.start_again().await;
    assert!(matches!(d.app.confirm_kit_saved().await, Err(AppError::Server(c)) if c == "registration_invalid"));
    assert_eq!(s.devices(), 0);
    assert!(d.secrets.load().unwrap().is_none());
    // Starting over with the new code works
    d.app.cancel_setup();
    let kit = d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    d.app.confirm_kit_saved().await.unwrap();
    assert_eq!(s.devices(), 1);
    assert_eq!(d.app.show_kit().unwrap(), kit);
    d.app.shutdown();
}

// ---- 4: an existing vault -----------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_existing_vault_is_reported_and_a_wrong_passphrase_registers_nothing() {
    let s = EmptyServer::new().await;
    let first = desktop("first");
    create_and_confirm(&first, &s).await;
    assert_eq!(s.devices(), 1);
    let second = desktop("second");
    let code = s.new_code();
    assert_eq!(second.app.probe_server(&code).await.unwrap(), ServerVault::Existing);
    // Creating a vault over an existing one is refused
    assert!(matches!(second.app.create_vault(&code, "x y z", "second").await, Err(AppError::Server(c)) if c == "vault_exists"));
    assert!(matches!(second.app.join(&code, "not the passphrase", "second").await, Err(AppError::WrongPassphrase)));
    assert_eq!(s.devices(), 1, "a wrong passphrase registers nothing");
    assert!(second.secrets.load().unwrap().is_none(), "and stores no key");
    second.app.join(&code, NEW_PASS, "second").await.unwrap();
    assert_eq!(s.devices(), 2);
    assert_eq!(second.app.mode(), Mode::Synced);
    first.app.shutdown();
    second.app.shutdown();
}

// ---- 5: joining with the recovery kit -----------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_recovery_kit_joins_the_vault_and_a_wrong_kit_registers_nothing() {
    let s = EmptyServer::new().await;
    let first = desktop("first");
    let kit = create_and_confirm(&first, &s).await;
    // An item from the first device, so the second can show it is in the same vault
    first.clipboard.user_copies(Content::Text("hello from the first computer".into()));
    tokio::time::sleep(Duration::from_millis(400)).await;
    first.app.sync_now().await.unwrap();

    let second = desktop("second");
    // A kit with a typo (checksum fails), one that is not a kit at all, and the kit of another vault
    let mut broken = kit.clone().into_bytes();
    broken[0] = if broken[0] == b'0' { b'1' } else { b'0' };
    let other_vault = recovery::encode(&Key::random().unwrap());
    for bad in [String::from_utf8(broken).unwrap(), "not a kit".to_string(), other_vault, String::new()] {
        let r = second.app.join_with_kit(&s.new_code(), &bad, "second").await;
        assert!(matches!(r, Err(AppError::WrongRecoveryKit)), "a bad kit is refused: {r:?}");
        assert_eq!(s.devices(), 1, "nothing is registered");
        assert!(second.secrets.load().unwrap().is_none(), "no key is stored");
    }
    second.app.join_with_kit(&s.new_code(), &kit, "second").await.unwrap();
    assert_eq!(s.devices(), 2);
    second.app.sync_now().await.unwrap();
    let found = second.app.search("first computer", Filter::All, 5, 0).await.unwrap();
    assert_eq!(found.len(), 1, "the second device sees the first one's item");
    assert_eq!(second.secrets.load().unwrap().unwrap().as_bytes(), first.secrets.load().unwrap().unwrap().as_bytes());
    first.app.shutdown();
    second.app.shutdown();
}

// ---- 7: showing the kit again -------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn show_kit_gives_the_text_that_was_shown_at_creation() {
    let s = EmptyServer::new().await;
    let d = desktop("show");
    let kit = create_and_confirm(&d, &s).await;
    assert_eq!(d.app.show_kit().unwrap(), kit);
    // After a restart as well: it is computed from the key in the secure store, not kept anywhere
    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert!(again.start().await.unwrap());
    assert_eq!(again.show_kit().unwrap(), kit);
    d.app.shutdown();
    again.shutdown();
}

// ---- 8: wiping the local data -------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn wiping_local_data_returns_to_the_first_run_and_leaves_the_server_alone() {
    let s = EmptyServer::new().await;
    let d = desktop("wipe");
    create_and_confirm(&d, &s).await;
    d.clipboard.user_copies(Content::Text("something to forget".into()));
    tokio::time::sleep(Duration::from_millis(400)).await;
    d.app.sync_now().await.unwrap();
    d.app.set_settings(Settings { auto_apply_remote: false, ..Settings::default() }).unwrap();
    assert!(d.dir.join("onepaste.db").exists() && d.dir.join("history.db").exists());

    d.app.wipe_local_data().await.unwrap();
    for name in ["onepaste.db", "history.db", "app-settings.json", "app-state.json"] {
        assert!(!d.dir.join(name).exists(), "{name} is gone");
    }
    assert!(d.secrets.load().unwrap().is_none(), "the key is gone from the secure store");
    assert_eq!(d.app.mode(), Mode::Unset);
    assert_eq!(s.devices(), 1, "the server's record of the device is untouched");
    // Starting again leads to the first run
    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert_eq!(again.first_run_state().await.unwrap(), Mode::Unset);
    assert!(!again.settings().auto_apply_remote == true || again.settings() == Settings::default());
    // And a new setup works from there
    again.choose_local_only().await.unwrap();
    assert_eq!(again.mode(), Mode::LocalOnly);
    again.shutdown();
}

// ---- 9: nothing secret in logs, diagnostics or files --------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn neither_the_passphrase_nor_the_kit_reaches_logs_diagnostics_or_files() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let s = EmptyServer::new().await;
    let d = desktop("leak");
    let kit = d.app.create_vault(&s.code(), NEW_PASS, "first").await.unwrap();
    d.app.confirm_kit_saved().await.unwrap();
    d.clipboard.user_copies(Content::Text(format!("copied item {}", now_ms())));
    tokio::time::sleep(Duration::from_millis(400)).await;
    d.app.sync_now().await.unwrap();
    // A failed attempt, so error paths are in the log as well
    let _ = d.app.join_with_kit(&s.new_code(), &kit, "again").await;
    let _ = d.app.create_vault("not a code", NEW_PASS, "x").await;

    let log = serde_json::to_string(&d.app.event_log()).unwrap();
    let diag = d.app.diagnostics().await;
    let mk: Key = d.secrets.load().unwrap().unwrap();
    let hex: String = mk.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let compact_kit: String = kit.chars().filter(|c| *c != '-').collect();
    let needles: Vec<(&str, Vec<u8>)> = vec![
        ("passphrase", NEW_PASS.as_bytes().to_vec()),
        ("recovery kit", kit.clone().into_bytes()),
        ("recovery kit without dashes", compact_kit.into_bytes()),
        ("key bytes", mk.as_bytes().to_vec()),
        ("key hex", hex.into_bytes()),
        ("key base64", B64.encode(mk.as_bytes()).into_bytes()),
    ];
    for (name, text) in [("event log", &log), ("diagnostics", &diag)] {
        for (what, n) in &needles {
            assert!(!text.as_bytes().windows(n.len()).any(|w| w == n.as_slice()), "{what} found in the {name}");
        }
    }
    assert!(log.contains("vault_created") && diag.contains("recent events"), "they are not empty");
    let mut scanned = 0;
    for f in files_under(&d.dir) {
        let data = std::fs::read(&f).unwrap();
        scanned += 1;
        for (what, n) in &needles {
            assert!(!data.windows(n.len()).any(|w| w == n.as_slice()), "{what} found in {f:?}");
        }
    }
    assert!(scanned >= 3, "client database, history database and settings were scanned ({scanned} files)");
    d.app.shutdown();
}

// ---- a vault made on screen can be used from the command line client -----------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_vault_made_by_the_screens_can_be_joined_by_the_command_line_client() {
    let s = EmptyServer::new().await;
    let d = desktop("cli");
    create_and_confirm(&d, &s).await;
    let cli_dir = temp_dir("cli-dir");
    join(
        &cli_dir,
        JoinOptions { code: s.new_code(), device_name: "cli".into(), passphrase: Some(NEW_PASS.into()), recovery_kit_out: None, existing_only: false, recovery_kit: None },
    )
    .await
    .unwrap();
    assert!(Session::open(&cli_dir, NEW_PASS).is_ok());
    d.app.shutdown();
}
