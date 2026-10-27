//! The application object: one place that owns the vault session, the history and the background work.

use crate::error::AppError;
use crate::ports::{ClipEvent, ClipboardPort, Content, MonitorGuard, SecretStore};
use crate::settings::Settings;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_client::{join, now_ms, JoinOptions, Session};
use onepaste_core::envelope::{open_item, Envelope, ItemPayload};
use onepaste_history::{Filter, HistoryStore, ItemSource};
use onepaste_sync::{Action, Edit, Event, PauseReason};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio::task::JoinHandle;

/// The server refuses items whose ciphertext is larger than this.
const MAX_ITEM_BYTES: usize = 8 * 1024 * 1024;
const MAX_LOG: usize = 200;
const PREVIEW_CHARS: usize = 120;
/// Presence of this file in the data directory means the first sync after joining is done.
const STATE_FILE: &str = "app-state.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayState {
    Synced,
    Syncing,
    Offline,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub joined: bool,
    pub tray: TrayState,
    /// `revoked` or `server_rollback` when syncing is stopped.
    pub paused: Option<String>,
    pub pending: usize,
    pub conflict: usize,
    pub failed: usize,
    /// Number of items in the history.
    pub items: i64,
    pub device_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub item_id: String,
    pub kind: String,
    /// First line of a text item.
    pub preview: String,
    pub favorite: bool,
    pub pinned: bool,
    pub note: String,
    pub width: u32,
    pub height: u32,
    /// Size in bytes of an image.
    pub size: i64,
}

/// An entry of the event log. It never holds clipboard content: only kinds, sizes and error codes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts_ms: u64,
    pub kind: String,
    pub detail: Option<String>,
}

#[derive(Clone)]
pub struct AppConfig {
    /// Listen for change notifications from the server (turned off in some tests).
    pub listen_for_changes: bool,
    /// How often to look for changes when nothing else triggers a sync.
    pub poll_interval: Duration,
    /// Run the sync loop in the background (turned off in tests that drive each round by hand with `sync_now`).
    pub background_sync: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig { listen_for_changes: true, poll_interval: Duration::from_secs(30), background_sync: true }
    }
}

type Listener = Arc<dyn Fn(&Status) + Send + Sync>;

struct Shared {
    settings: Settings,
    tray: TrayState,
    paused: Option<PauseReason>,
    log: VecDeque<LogEntry>,
    /// Fingerprint of the last content captured or applied, so the same content is not queued twice in a row.
    last_fp: Option<[u8; 32]>,
    device_id: String,
    joined: bool,
    started: bool,
    /// The first sync after joining has finished. Until then an item that arrives is history, not news.
    initial_sync_done: bool,
    listener: Option<Listener>,
}

struct Inner {
    dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    clipboard: Arc<dyn ClipboardPort>,
    cfg: AppConfig,
    session: Mutex<Option<Session>>,
    history: StdMutex<Option<HistoryStore>>,
    shared: StdMutex<Shared>,
    wake: Notify,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
    monitor: StdMutex<Option<MonitorGuard>>,
}

#[derive(Clone)]
pub struct App(Arc<Inner>);

enum RoundEnd {
    Idle,
    Offline,
    Waiting(u64),
    Paused,
}

fn fingerprint(c: &Content) -> [u8; 32] {
    let mut h = Sha256::new();
    match c {
        Content::Text(t) => {
            h.update([0u8]);
            h.update(t.as_bytes());
        }
        Content::Image { png, .. } => {
            h.update([1u8]);
            h.update(png);
        }
    }
    h.finalize().into()
}

fn lock<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn paused_code(r: PauseReason) -> &'static str {
    r.as_str()
}

impl App {
    pub fn new(dir: PathBuf, secrets: Arc<dyn SecretStore>, clipboard: Arc<dyn ClipboardPort>, cfg: AppConfig) -> App {
        let settings = Settings::load(&dir);
        App(Arc::new(Inner {
            dir,
            secrets,
            clipboard,
            cfg,
            session: Mutex::new(None),
            history: StdMutex::new(None),
            shared: StdMutex::new(Shared {
                settings,
                tray: TrayState::Synced,
                paused: None,
                log: VecDeque::new(),
                last_fp: None,
                device_id: String::new(),
                joined: false,
                started: false,
                initial_sync_done: false,
                listener: None,
            }),
            wake: Notify::new(),
            tasks: StdMutex::new(Vec::new()),
            monitor: StdMutex::new(None),
        }))
    }

    // ---- state and log ---------------------------------------------------------------------------------------------

    /// Registers a function that is called whenever the status changes (the shell updates the tray from it).
    pub fn on_status_change(&self, f: impl Fn(&Status) + Send + Sync + 'static) {
        lock(&self.0.shared).listener = Some(Arc::new(f));
    }

    fn log(&self, kind: &str, detail: Option<String>) {
        let mut g = lock(&self.0.shared);
        g.log.push_back(LogEntry { ts_ms: now_ms(), kind: kind.to_string(), detail });
        while g.log.len() > MAX_LOG {
            g.log.pop_front();
        }
    }

    pub fn event_log(&self) -> Vec<LogEntry> {
        lock(&self.0.shared).log.iter().cloned().collect()
    }

    fn set_tray(&self, tray: TrayState, paused: Option<PauseReason>) {
        let (listener, status) = {
            let mut g = lock(&self.0.shared);
            let changed = g.tray != tray || g.paused != paused;
            g.tray = tray;
            g.paused = paused;
            if !changed {
                return;
            }
            (g.listener.clone(), self.quick_status(&g))
        };
        if let Some(l) = listener {
            l(&status);
        }
    }

    fn quick_status(&self, g: &Shared) -> Status {
        Status {
            joined: g.joined,
            tray: g.tray,
            paused: g.paused.map(|p| paused_code(p).to_string()),
            pending: 0,
            conflict: 0,
            failed: 0,
            items: 0,
            device_id: g.device_id.clone(),
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.0.shared).settings
    }

    pub fn set_settings(&self, s: Settings) -> Result<(), AppError> {
        s.save(&self.0.dir)?;
        lock(&self.0.shared).settings = s;
        Ok(())
    }

    // ---- joining and starting --------------------------------------------------------------------------------------

    /// Whether a master key is in the secure store and a vault was joined on this computer.
    pub fn is_joined(&self) -> bool {
        lock(&self.0.shared).joined
    }

    /// Joins an existing vault with the connection code and the passphrase. The passphrase is used once and dropped;
    /// the master key goes into the secure store. A wrong passphrase registers nothing.
    pub async fn join(&self, code: &str, passphrase: &str, device_name: &str) -> Result<(), AppError> {
        if self.is_joined() {
            return Err(AppError::Other("already_joined".into()));
        }
        let result = join(
            &self.0.dir,
            JoinOptions {
                code: code.to_string(),
                device_name: device_name.to_string(),
                passphrase: Some(passphrase.to_string()),
                recovery_kit_out: None,
                existing_only: true,
            },
        )
        .await
        .map_err(AppError::from)?;
        self.0.secrets.save(&result.master_key)?;
        self.log("joined", None);
        self.start().await?;
        Ok(())
    }

    /// Opens the vault with the key from the secure store (no passphrase) and starts the background work. Does nothing
    /// if no key is stored yet. Calling it again is harmless.
    pub async fn start(&self) -> Result<bool, AppError> {
        {
            let g = lock(&self.0.shared);
            if g.started {
                return Ok(g.joined);
            }
        }
        let Some(mk) = self.0.secrets.load()? else {
            return Ok(false);
        };
        let session = Session::open_with_key(&self.0.dir, &mk).map_err(AppError::from)?;
        let device_id = session.info.device_id.clone();
        let history = HistoryStore::open(&self.0.dir, &mk).map_err(AppError::from)?;
        *self.0.session.lock().await = Some(session);
        *lock(&self.0.history) = Some(history);
        {
            let mut g = lock(&self.0.shared);
            g.joined = true;
            g.started = true;
            g.device_id = device_id;
            g.initial_sync_done = self.0.dir.join(STATE_FILE).exists();
        }
        self.start_monitor();
        if self.0.cfg.background_sync {
            let app = self.clone();
            lock(&self.0.tasks).push(tokio::spawn(async move { app.drive().await }));
            self.0.wake.notify_one();
        }
        self.notify_status();
        Ok(true)
    }

    fn notify_status(&self) {
        let (listener, status) = {
            let g = lock(&self.0.shared);
            (g.listener.clone(), self.quick_status(&g))
        };
        if let Some(l) = listener {
            l(&status);
        }
    }

    fn start_monitor(&self) {
        if !lock(&self.0.shared).settings.record_clipboard {
            return;
        }
        let app = self.clone();
        let handle = tokio::runtime::Handle::current();
        let started = self.0.clipboard.start_monitor(Box::new(move |ev| {
            let app = app.clone();
            handle.spawn(async move { app.on_clip_event(ev).await });
        }));
        match started {
            Ok(guard) => *lock(&self.0.monitor) = Some(guard),
            Err(e) => self.log("clipboard_monitor_failed", Some(e.code())),
        }
    }

    // ---- capturing -------------------------------------------------------------------------------------------------

    /// Handles one clipboard event.
    pub async fn on_clip_event(&self, ev: ClipEvent) {
        match ev {
            ClipEvent::SkippedSensitive => self.log("skipped_sensitive", None),
            ClipEvent::SkippedSelf => self.log("skipped_self", None),
            ClipEvent::Captured(content) => {
                if !self.settings().record_clipboard {
                    return;
                }
                if let Err(e) = self.capture(content).await {
                    self.log("capture_failed", Some(e.code()));
                }
            }
        }
    }

    async fn capture(&self, content: Content) -> Result<(), AppError> {
        let fp = fingerprint(&content);
        if lock(&self.0.shared).last_fp == Some(fp) {
            self.log("skipped_duplicate", None);
            return Ok(());
        }
        let device = lock(&self.0.shared).device_id.clone();
        let (payload, kind, size) = match &content {
            Content::Text(t) => (ItemPayload { kind: "text".into(), content: Some(t.clone()), device: Some(device), ..Default::default() }, "text", t.len()),
            Content::Image { png, width, height } => {
                let encoded = B64.encode(png);
                // The ciphertext is about the size of the encoded content; the server refuses anything above its limit
                if encoded.len() + 1024 > MAX_ITEM_BYTES {
                    self.log("image_too_large", Some(format!("{} bytes", png.len())));
                    return Err(AppError::ImageTooLarge);
                }
                let mut p = ItemPayload { kind: "image".into(), content: Some(encoded), device: Some(device), ..Default::default() };
                p.extra.insert("width".into(), (*width).into());
                p.extra.insert("height".into(), (*height).into());
                p.extra.insert("mime".into(), "image/png".into());
                p.extra.insert("size".into(), png.len().into());
                (p, "image", png.len())
            }
        };
        {
            let mut g = self.0.session.lock().await;
            let s = g.as_mut().ok_or(AppError::NotJoined)?;
            s.engine.enqueue_create(now_ms(), &payload).map_err(AppError::from)?;
        }
        lock(&self.0.shared).last_fp = Some(fp);
        self.log("captured", Some(format!("{kind}, {size} bytes")));
        self.0.wake.notify_one();
        Ok(())
    }

    // ---- syncing ---------------------------------------------------------------------------------------------------

    /// Asks for a sync as soon as possible (a wake from sleep, the network coming back, a local change).
    pub fn trigger_sync(&self) {
        self.0.wake.notify_one();
    }

    pub fn notify_wake(&self) {
        self.log("woke_from_sleep", None);
        self.trigger_sync();
    }

    pub fn notify_network(&self, online: bool) {
        self.log(if online { "network_online" } else { "network_offline" }, None);
        if online {
            self.trigger_sync();
        }
    }

    /// Runs one sync round now and returns when it is over. The vault session is locked only while the engine is
    /// consulted, never while a request is in flight, so capturing and searching stay responsive.
    pub async fn sync_now(&self) -> Result<(), AppError> {
        match self.sync_round().await {
            RoundEnd::Idle => Ok(()),
            RoundEnd::Offline => Err(AppError::Unreachable),
            RoundEnd::Waiting(_) => Ok(()),
            RoundEnd::Paused => Err(match lock(&self.0.shared).paused {
                Some(PauseReason::Revoked) => AppError::Revoked,
                _ => AppError::ServerRollback,
            }),
        }
    }

    async fn sync_round(&self) -> RoundEnd {
        let before = {
            let mut g = self.0.session.lock().await;
            let Some(s) = g.as_mut() else { return RoundEnd::Idle };
            s.engine.request_pull();
            s.engine.status().map(|st| st.max_seen_seq).unwrap_or(0)
        };
        let (paused_before, first_sync) = {
            let g = lock(&self.0.shared);
            (g.paused, !g.initial_sync_done)
        };
        self.set_tray(TrayState::Syncing, paused_before);
        let mut end = RoundEnd::Idle;
        loop {
            let (action, transport) = {
                let mut g = self.0.session.lock().await;
                let Some(s) = g.as_mut() else { return RoundEnd::Idle };
                (s.engine.next_action(now_ms()), s.transport.clone())
            };
            match action {
                Err(e) => {
                    self.log("sync_engine_error", Some(e.code().to_string()));
                    end = RoundEnd::Offline;
                    break;
                }
                Ok(Action::Send(req)) => {
                    let answer = transport.send(&req).await;
                    let mut g = self.0.session.lock().await;
                    let Some(s) = g.as_mut() else { return RoundEnd::Idle };
                    match answer {
                        Ok(resp) => match s.engine.on_response(now_ms(), resp) {
                            Ok(events) => {
                                for ev in events {
                                    self.note_event(&ev);
                                }
                            }
                            Err(e) => {
                                self.log("sync_engine_error", Some(e.code().to_string()));
                                end = RoundEnd::Offline;
                                break;
                            }
                        },
                        Err(_) => {
                            let _ = s.engine.on_transport_error(now_ms());
                            end = RoundEnd::Offline;
                            break;
                        }
                    }
                }
                Ok(Action::Wait { until_ms }) => {
                    end = RoundEnd::Waiting(until_ms);
                    break;
                }
                Ok(Action::Idle) | Ok(Action::InFlight) => break,
                Ok(Action::Paused(r)) => {
                    lock(&self.0.shared).paused = Some(r);
                    end = RoundEnd::Paused;
                    break;
                }
            }
        }
        if !matches!(end, RoundEnd::Paused) && !first_sync {
            self.apply_remote(before).await;
        }
        if first_sync && matches!(end, RoundEnd::Idle) {
            // From the next round on, what arrives is news
            if std::fs::write(self.0.dir.join(STATE_FILE), b"{\"initialSyncDone\":true}").is_ok() {
                lock(&self.0.shared).initial_sync_done = true;
            }
        }
        if let Err(e) = self.refresh_history().await {
            self.log("history_refresh_failed", Some(e.code()));
        }
        match end {
            RoundEnd::Paused => {
                let reason = lock(&self.0.shared).paused;
                self.log("sync_paused", reason.map(|r| paused_code(r).to_string()));
                self.set_tray(TrayState::Error, reason);
            }
            RoundEnd::Offline => self.set_tray(TrayState::Offline, None),
            _ => self.set_tray(TrayState::Synced, None),
        }
        end
    }

    fn note_event(&self, ev: &Event) {
        match ev {
            Event::Paused(r) => {
                lock(&self.0.shared).paused = Some(*r);
            }
            Event::Pushed { .. } => self.log("uploaded", None),
            Event::Pulled { applied, .. } if *applied > 0 => self.log("downloaded", Some(format!("{applied} items"))),
            Event::Conflict { .. } => self.log("conflict", None),
            Event::OpFailed { code, .. } => self.log("upload_failed", Some(code.clone())),
            Event::PullFailed { code } => self.log("download_failed", Some(code.clone())),
            _ => {}
        }
    }

    /// The background loop: sync when asked, when the server announces a change, and from time to time.
    async fn drive(self) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let listener = if self.0.cfg.listen_for_changes {
            self.0.session.lock().await.as_ref().map(|s| s.listen_for_changes(tx.clone()))
        } else {
            None
        };
        let _keep_open = tx;
        let mut wait = Duration::ZERO;
        let mut backoff = 1u64;
        loop {
            tokio::select! {
                _ = self.0.wake.notified() => {}
                msg = rx.recv() => {
                    if let Some(Some(seq)) = msg {
                        if let Some(s) = self.0.session.lock().await.as_mut() {
                            let _ = s.engine.note_event_seq(seq);
                        }
                    }
                }
                _ = tokio::time::sleep(wait) => {}
            }
            let end = self.sync_round().await;
            wait = match end {
                RoundEnd::Idle => {
                    backoff = 1;
                    self.0.cfg.poll_interval
                }
                RoundEnd::Offline => {
                    let w = backoff;
                    backoff = (backoff * 2).min(60);
                    Duration::from_secs(w)
                }
                RoundEnd::Waiting(until) => Duration::from_millis(until.saturating_sub(now_ms()).max(500)),
                RoundEnd::Paused => Duration::from_secs(3600),
            };
            if let Some(l) = &listener {
                if l.is_finished() {
                    break;
                }
            }
        }
    }

    /// Writes the newest item that arrived from another device into the clipboard, if all the conditions hold.
    async fn apply_remote(&self, before: i64) {
        if !self.settings().auto_apply_remote {
            return;
        }
        let newest: Option<(i64, Content)> = {
            let mut g = self.0.session.lock().await;
            let Some(s) = g.as_mut() else { return };
            let own = s.info.device_id.clone();
            let queued: HashSet<String> = s.engine.outbox().map(|o| o.into_iter().map(|r| r.item_id).collect()).unwrap_or_default();
            let mk = onepaste_core::crypto::Key::from_bytes(*s.master_key().as_bytes());
            let Ok(changes) = s.engine.store_mut().changed_since(before) else { return };
            let mut best: Option<(i64, Content)> = None;
            for row in changes.rows {
                if row.seq <= before || row.deleted || queued.contains(&row.id) {
                    continue;
                }
                let Some(json) = row.envelope_json.as_deref() else { continue };
                let Ok(env) = serde_json::from_str::<Envelope>(json) else { continue };
                let Ok(payload) = open_item(&mk, &env) else { continue };
                if payload.device.as_deref() == Some(own.as_str()) {
                    continue;
                }
                if best.as_ref().is_some_and(|(seq, _)| *seq >= row.seq) {
                    continue;
                }
                if let Some(c) = content_of(&payload) {
                    best = Some((row.seq, c));
                }
            }
            best
        };
        if let Some((_, content)) = newest {
            // The write carries the origin marker, which is what keeps it from coming back as a new item
            match self.0.clipboard.write(&content, true) {
                Ok(()) => self.log("applied_remote", Some(kind_and_size(&content))),
                Err(e) => self.log("apply_failed", Some(e.code())),
            }
        }
    }

    async fn refresh_history(&self) -> Result<(), AppError> {
        let mut g = self.0.session.lock().await;
        let Some(s) = g.as_mut() else { return Ok(()) };
        let mk = onepaste_core::crypto::Key::from_bytes(*s.master_key().as_bytes());
        let mut h = lock(&self.0.history);
        let Some(h) = h.as_mut() else { return Ok(()) };
        h.refresh(s.engine.store_mut(), &mk)?;
        Ok(())
    }

    // ---- commands --------------------------------------------------------------------------------------------------

    pub async fn status(&self) -> Status {
        let mut st = {
            let g = lock(&self.0.shared);
            self.quick_status(&g)
        };
        if let Some(s) = self.0.session.lock().await.as_mut() {
            if let Ok(x) = s.engine.status() {
                st.pending = x.pending;
                st.conflict = x.conflict;
                st.failed = x.failed;
                st.paused = x.paused.map(|p| paused_code(p).to_string()).or(st.paused);
            }
        }
        st.items = lock(&self.0.history).as_ref().and_then(|h| h.count().ok()).unwrap_or(0);
        st
    }

    /// Searches the local history; an empty query lists the most recent items. Brings the history up to date first, so
    /// something copied a moment ago is found at once.
    pub async fn search(&self, query: &str, filter: Filter, limit: usize, offset: usize) -> Result<Vec<ItemView>, AppError> {
        self.refresh_history().await?;
        let hits = {
            let h = lock(&self.0.history);
            h.as_ref().ok_or(AppError::NotJoined)?.search(query, filter, limit, offset)?
        };
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let (mut width, mut height) = (0, 0);
            if hit.kind == "image" {
                if let Some(s) = self.0.session.lock().await.as_mut() {
                    if let Ok(p) = s.engine.get_item(&hit.item_id) {
                        width = p.extra.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        height = p.extra.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                }
            }
            out.push(ItemView {
                item_id: hit.item_id,
                kind: hit.kind,
                preview: hit.text.lines().next().unwrap_or("").chars().take(PREVIEW_CHARS).collect(),
                favorite: hit.favorite,
                pinned: hit.pinned,
                note: hit.note,
                width,
                height,
                size: hit.size,
            });
        }
        Ok(out)
    }

    /// Puts an item back on the clipboard. The write carries the origin marker, so it does not become a new item.
    pub async fn copy(&self, item_id: &str) -> Result<(), AppError> {
        let content = {
            let mut g = self.0.session.lock().await;
            let s = g.as_mut().ok_or(AppError::NotJoined)?;
            let payload = s.engine.get_item(item_id).map_err(|_| AppError::NotFound)?;
            content_of(&payload).ok_or(AppError::NotFound)?
        };
        self.0.clipboard.write(&content, true)
    }

    pub async fn set_flags(&self, item_id: &str, favorite: Option<bool>, pinned: Option<bool>, note: Option<String>) -> Result<(), AppError> {
        {
            let mut g = self.0.session.lock().await;
            let s = g.as_mut().ok_or(AppError::NotJoined)?;
            if let Some(v) = favorite {
                s.engine.enqueue_edit(now_ms(), item_id, Edit::Favorite(v)).map_err(AppError::from)?;
            }
            if let Some(v) = pinned {
                s.engine.enqueue_edit(now_ms(), item_id, Edit::Pinned(v)).map_err(AppError::from)?;
            }
            if let Some(n) = note {
                s.engine.enqueue_edit(now_ms(), item_id, Edit::Note(if n.is_empty() { None } else { Some(n) })).map_err(AppError::from)?;
            }
        }
        self.0.wake.notify_one();
        Ok(())
    }

    pub async fn delete(&self, item_id: &str) -> Result<(), AppError> {
        {
            let mut g = self.0.session.lock().await;
            let s = g.as_mut().ok_or(AppError::NotJoined)?;
            s.engine.enqueue_delete(now_ms(), item_id).map_err(AppError::from)?;
        }
        self.0.wake.notify_one();
        Ok(())
    }

    /// Text for the "copy diagnostics" button: versions, state, counts and recent event kinds. No content and no keys.
    pub async fn diagnostics(&self) -> String {
        let st = self.status().await;
        let log = self.event_log();
        let mut out = String::new();
        out.push_str(&format!("joined: {}\n", st.joined));
        out.push_str(&format!("tray: {:?}\n", st.tray));
        out.push_str(&format!("paused: {}\n", st.paused.as_deref().unwrap_or("no")));
        out.push_str(&format!("queued changes: {} pending, {} conflict, {} failed\n", st.pending, st.conflict, st.failed));
        out.push_str(&format!("items in the history: {}\n", st.items));
        let settings = self.settings();
        out.push_str(&format!("auto apply remote: {}, record clipboard: {}\n", settings.auto_apply_remote, settings.record_clipboard));
        out.push_str("recent events:\n");
        for e in log.iter().rev().take(20).rev() {
            out.push_str(&format!("  {} {}{}\n", e.ts_ms, e.kind, e.detail.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()));
        }
        out
    }

    /// Stops the background work.
    pub fn shutdown(&self) {
        for t in lock(&self.0.tasks).drain(..) {
            t.abort();
        }
        *lock(&self.0.monitor) = None;
    }
}

fn kind_and_size(c: &Content) -> String {
    match c {
        Content::Text(t) => format!("text, {} bytes", t.len()),
        Content::Image { png, .. } => format!("image, {} bytes", png.len()),
    }
}

/// The clipboard content of an item, or `None` for something this version cannot place on the clipboard.
fn content_of(p: &ItemPayload) -> Option<Content> {
    match p.kind.as_str() {
        "text" => Some(Content::Text(p.content.clone()?)),
        "image" => {
            let png = B64.decode(p.content.as_deref()?).ok()?;
            let dim = |k: &str| p.extra.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            Some(Content::Image { png, width: dim("width"), height: dim("height") })
        }
        _ => None,
    }
}

