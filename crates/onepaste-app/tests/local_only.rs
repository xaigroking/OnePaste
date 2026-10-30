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

#[tokio::test(flavor = "multi_thread")]
async fn suspected_secrets_are_not_stored_indexed_or_queued_locally() {
    let before = requests_started();
    let d = desktop("filtered");
    d.app.choose_local_only().await.unwrap();
    let samples = [
        format!("-----BEGIN RSA PRIVATE KEY-----\n{}", "LOCAL-PRIVATE-MARKER"),
        format!("{}{}", "github_pat_", "LOCAL_TOKEN_MARKER"),
    ];
    for sample in &samples {
        copy(&d, sample).await;
        assert_eq!(d.app.status().await.items, 0, "filtered text must not create an item");
        assert!(d.app.search("", Filter::All, 10, 0).await.unwrap().is_empty(), "no history index entry");
    }
    let mut store = SqliteStore::open(&d.dir.join("onepaste.db")).unwrap();
    onepaste_sync::Store::transact(&mut store, &mut |tx| {
        assert!(tx.items()?.is_empty(), "no rejected item in the client store");
        assert!(tx.outbox()?.is_empty(), "no rejected text may enter the outbox");
        Ok(())
    }).unwrap();
    assert!(d.clipboard.writes().is_empty(), "filtering must not rewrite the clipboard");
    let events: Vec<_> = d.app.event_log().into_iter().filter(|e| e.kind == "skipped_suspected_secret").collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].detail.as_deref(), Some("private_key_marker"));
    assert_eq!(events[1].detail.as_deref(), Some("github_token_prefix"));
    let output = format!("{}{}", serde_json::to_string(&d.app.event_log()).unwrap(), d.app.diagnostics().await);
    for needle in samples.iter().map(String::as_str).chain(["LOCAL-PRIVATE-MARKER", "LOCAL_TOKEN_MARKER", "-----BEGIN", "github_pat_"]) {
        assert!(!output.contains(needle), "logs and diagnostics must contain only fixed reason codes");
    }
    assert_eq!(requests_started(), before);
    d.app.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn suspected_secrets_switch_is_immediate_persistent_and_does_not_override_other_guards() {
    let before = requests_started();
    let d = desktop("filter-switch");
    d.app.choose_local_only().await.unwrap();
    let sample = format!("{}{}", "ghr_", "LOCAL_SWITCH_SYNTHETIC");
    d.app.on_clip_event(ClipEvent::Captured(Content::Text(sample.clone()))).await;
    let mut settings = d.app.settings();
    settings.skip_suspected_secrets = false;
    settings.auto_apply_remote = false;
    settings.text_preview_height = Some(333);
    d.app.set_settings(settings.clone()).unwrap();
    assert!(d.app.search("", Filter::All, 10, 0).await.unwrap().is_empty(), "disabling must not backfill");
    d.app.on_clip_event(ClipEvent::Captured(Content::Text(sample.clone()))).await;
    let history = d.app.search("", Filter::All, 10, 0).await.unwrap();
    assert_eq!(history.len(), 1, "a rejected copy did not advance the captured fingerprint");
    assert_eq!(d.app.item_text(&history[0].item_id).await.unwrap(), sample);
    d.app.on_clip_event(ClipEvent::SkippedSensitive).await;
    d.app.on_clip_event(ClipEvent::SkippedSelf).await;
    assert_eq!(d.app.search("", Filter::All, 10, 0).await.unwrap().len(), 1);
    d.app.shutdown();

    let again = App::new(d.dir.clone(), d.secrets.clone(), d.clipboard.clone(), quiet());
    assert_eq!(again.settings(), settings, "all settings survive reconstruction");
    assert!(again.start().await.unwrap());
    settings.skip_suspected_secrets = true;
    again.set_settings(settings.clone()).unwrap();
    again.on_clip_event(ClipEvent::Captured(Content::Text(format!("{}{}", "ghs_", "RE_ENABLED_SYNTHETIC")))).await;
    assert_eq!(again.search("", Filter::All, 10, 0).await.unwrap().len(), 1, "enabling does not remove history");
    again.on_clip_event(ClipEvent::Captured(Content::Text("ordinary text".into()))).await;
    // Images are not scanned, even if their bytes happen to include a text marker.
    again.on_clip_event(ClipEvent::Captured(Content::Image { png: b"-----BEGIN PRIVATE KEY-----".to_vec(), width: 1, height: 1 })).await;
    assert_eq!(again.search("", Filter::Images, 10, 0).await.unwrap().len(), 1);
    let logs_before = again.event_log().len();
    settings.record_clipboard = false;
    again.set_settings(settings).unwrap();
    again.on_clip_event(ClipEvent::Captured(Content::Text(format!("{}{}", "ghu_", "LISTENER_OFF_SYNTHETIC")))).await;
    again.on_clip_event(ClipEvent::Captured(Content::Text("listener off".into()))).await;
    assert_eq!(again.event_log().len(), logs_before, "listening off short-circuits detection and capture");
    assert_eq!(again.search("", Filter::All, 10, 0).await.unwrap().len(), 3);
    again.shutdown();
    assert_eq!(requests_started(), before, "local-only filtering and switches use no network");
}

#[test]
fn suspected_secrets_old_settings_default_on_and_failed_save_preserves_the_previous_choice() {
    let dir = temp_dir("old-settings");
    std::fs::write(dir.join("app-settings.json"), r#"{"recordClipboard":false,"autoApplyRemote":false,"textPreviewHeight":310}"#).unwrap();
    let app = App::new(dir.clone(), Arc::new(MemorySecretStore::new()), Arc::new(FakeClipboard::new()), quiet());
    let old = app.settings();
    assert!(old.skip_suspected_secrets && Settings::default().skip_suspected_secrets);
    assert!(!old.record_clipboard && !old.auto_apply_remote);
    assert_eq!(old.text_preview_height, Some(310));
    let mut next = old.clone();
    next.skip_suspected_secrets = false;
    app.set_settings(next.clone()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("app-settings.json")).unwrap()).unwrap();
    assert_eq!(json["skipSuspectedSecrets"], false);
    assert_eq!(json["recordClipboard"], false);
    assert_eq!(json["textPreviewHeight"], 310);
    std::fs::remove_file(dir.join("app-settings.json")).unwrap();
    std::fs::create_dir(dir.join("app-settings.json")).unwrap();
    assert!(app.set_settings(old).is_err());
    assert_eq!(app.settings(), next, "a save failure must not change the live choice");
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
