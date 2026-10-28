//! Local-only mode. Every test in this file must run without any network use: the request counter of the client is
//! process-wide, so nothing here starts a server or joins a vault.

use onepaste_app::*;
use onepaste_client::{requests_started, SqliteStore};
use onepaste_core::crypto::Key;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!("onepaste-local-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn quiet() -> AppConfig {
    AppConfig { listen_for_changes: true, poll_interval: Duration::from_millis(50), background_sync: true, ..AppConfig::default() }
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

async fn copy(d: &Desktop, text: &str) {
    d.clipboard.user_copies(Content::Text(text.into()));
    tokio::time::sleep(Duration::from_millis(300)).await;
}

// ---- 6 ------------------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn local_only_records_and_searches_with_no_device_no_queue_and_no_network() {
    let before = requests_started();
    let d = desktop("local");
    assert_eq!(d.app.first_run_state().await.unwrap(), Mode::Unset);
    d.app.choose_local_only().await.unwrap();
    assert_eq!(d.app.mode(), Mode::LocalOnly);
    let st = d.app.status().await;
    assert!(st.joined && st.mode == Mode::LocalOnly && st.tray == TrayState::Local, "{st:?}");
    assert_eq!(st.device_id, "", "there is no device");

    copy(&d, "local only needle one").await;
    copy(&d, "local only needle two").await;
    d.clipboard.user_copies(Content::Image { png: vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4], width: 2, height: 2 });
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Everything the normal mode does with an item works
    let found = d.app.search("needle", Filter::All, 10, 0).await.unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(d.app.search("", Filter::Images, 10, 0).await.unwrap().len(), 1);
    let id = found[0].item_id.clone();
    d.app.set_flags(&id, Some(true), Some(true), Some("a note".into())).await.unwrap();
    let favs = d.app.search("", Filter::Favorites, 10, 0).await.unwrap();
    assert_eq!(favs.len(), 1);
    assert!(favs[0].pinned && favs[0].note == "a note");
    d.app.copy(&id).await.unwrap();
    assert!(d.clipboard.writes().last().is_some_and(|(_, marker)| *marker), "the write carries the origin marker");
    d.app.delete(&found[1].item_id).await.unwrap();
    assert_eq!(d.app.search("needle", Filter::All, 10, 0).await.unwrap().len(), 1);
    // Syncing is a no-op, and the tray stays on "local only"
    d.app.sync_now().await.unwrap();
    d.app.trigger_sync();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(d.app.status().await.tray, TrayState::Local);
    d.app.shutdown();

    // The client database has no device identity and nothing queued
    let mut store = SqliteStore::open(&d.dir.join("onepaste.db")).unwrap();
    for key in ["device_id", "u", "fp", "keyparams"] {
        assert!(store.meta_get(key).unwrap().is_none(), "{key} must not exist in local-only mode");
    }
    assert!(store.secret_get("devtoken").unwrap().is_none());
    let mut queued = usize::MAX;
    onepaste_sync::Store::transact(&mut store, &mut |tx| {
        queued = tx.outbox()?.len();
        Ok(())
    })
    .unwrap();
    assert_eq!(queued, 0, "nothing is queued for upload");
    // And no request was started by this process at any point
    assert_eq!(requests_started(), before, "local-only mode must not use the network");
}

#[tokio::test(flavor = "multi_thread")]
async fn local_only_survives_a_restart_with_the_same_key() {
    let d = desktop("restart");
    d.app.choose_local_only().await.unwrap();
    copy(&d, "remember this across restarts").await;
    d.app.shutdown();
    let key: Key = d.secrets.load().unwrap().unwrap();
    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert_eq!(again.first_run_state().await.unwrap(), Mode::LocalOnly);
    assert_eq!(again.search("across restarts", Filter::All, 5, 0).await.unwrap().len(), 1);
    assert_eq!(d.secrets.load().unwrap().unwrap().as_bytes(), key.as_bytes(), "the same key");
    // Without the key nothing opens, and the screens start over
    let lost = App::new(d.dir.clone(), Arc::new(MemorySecretStore::new()), d.clipboard.clone(), quiet());
    assert_eq!(lost.first_run_state().await.unwrap(), Mode::Unset);
    again.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_only_has_no_recovery_kit_and_is_not_set_up_twice() {
    let d = desktop("nokit");
    d.app.choose_local_only().await.unwrap();
    assert!(matches!(d.app.show_kit(), Err(AppError::NoRecoveryKit)));
    assert!(matches!(d.app.choose_local_only().await, Err(AppError::AlreadySetUp)));
    assert!(matches!(d.app.join("x", "y", "z").await, Err(AppError::AlreadySetUp)));
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn local_only_files_hold_neither_content_nor_the_key() {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let d = desktop("leak");
    d.app.choose_local_only().await.unwrap();
    let marker = "LOCAL-LEAK-MARKER-7788";
    copy(&d, &format!("secret text {marker}")).await;
    let log = serde_json::to_string(&d.app.event_log()).unwrap();
    let diag = d.app.diagnostics().await;
    assert!(!log.contains(marker) && !diag.contains(marker));
    let mk: Key = d.secrets.load().unwrap().unwrap();
    let hex: String = mk.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let needles: Vec<Vec<u8>> = vec![marker.as_bytes().to_vec(), mk.as_bytes().to_vec(), hex.into_bytes(), B64.encode(mk.as_bytes()).into_bytes()];
    d.app.shutdown();
    for f in files_under(&d.dir) {
        let data = std::fs::read(&f).unwrap();
        for n in &needles {
            assert!(!data.windows(n.len()).any(|w| w == n.as_slice()), "secret found in {f:?}");
        }
    }
}
