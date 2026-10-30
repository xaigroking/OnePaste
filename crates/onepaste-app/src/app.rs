//! The application object: one place that owns the vault session, the history and the background work.

use crate::error::AppError;
use crate::ports::{ClipEvent, ClipboardPort, Content, MonitorGuard, SecretStore};
use crate::settings::{Settings, StateFile, STATE_FILE_NAME};
use crate::update::{self, Channel, HttpsSource, Source, UpdateInfo};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use onepaste_client::{db_path, join, now_ms, prepare_new_vault, probe, JoinOptions, NewVault, Session, SqliteStore, VaultState, DB_FILE};
use onepaste_core::crypto::Key;
use onepaste_core::recovery;
use onepaste_core::envelope::{open_item, Envelope, ItemPayload};
use onepaste_history::{Filter, HistoryStore, ItemSource};
use onepaste_sync::{Action, Edit, Engine as SyncEngine, Event, PauseReason};
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayState {
    Synced,
    Syncing,
    Offline,
    Error,
    /// Local-only mode: nothing is synced, and that is on purpose. Not the same as offline.
    Local,
}

/// How this computer uses OnePaste.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Not set up yet: the first-run screens are next.
    Unset,
    /// The history stays on this computer.
    LocalOnly,
    /// Synced through the user's own server.
    Synced,
}

/// What the server behind a connection code holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerVault {
    Existing,
    Empty,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub joined: bool,
    pub mode: Mode,
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

/// One device of the vault, as the devices page shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub device_id: String,
    /// The name the user gave it, opened with the vault key. `None` if it cannot be read.
    pub name: Option<String>,
    /// Seconds since the epoch.
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked: bool,
    pub this_device: bool,
}

/// The devices of the vault and the server they sync through.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    pub devices: Vec<DeviceView>,
    pub server_url: String,
}

/// The picture of an image item, for the detail pane, the thumbnail and the big view.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    /// The PNG file, base64 encoded.
    pub data: String,
    pub width: u32,
    pub height: u32,
    /// Size of the PNG in bytes.
    pub size: usize,
    pub format: String,
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
    /// Look for updates in the background: once when `start_update_checks` is called, then every six hours.
    pub update_checks: bool,
    /// The version this program is, as the update check compares it: a release name without its `v`.
    pub update_version: String,
    /// When this build was made, seconds since the epoch. It orders pre-releases that share their numbers.
    pub built_at: u64,
    /// Where updates come from and which key signs them. `None` is the real thing: HTTPS to GitHub and the key that
    /// is part of this program. There is no environment variable or file that changes it; only code that builds an
    /// `AppConfig` (tests do) can.
    pub update_source: Option<UpdateSetup>,
}

/// A replacement for the real update source and key, for tests.
#[derive(Clone)]
pub struct UpdateSetup {
    pub source: Arc<dyn Source>,
    pub releases_url: String,
    pub public_key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig { listen_for_changes: true, poll_interval: Duration::from_secs(30), background_sync: true, update_checks: false, update_version: "0.0.0".into(), built_at: 0, update_source: None }
    }
}

type Listener = Arc<dyn Fn(&Status) + Send + Sync>;

struct Shared {
    settings: Settings,
    tray: TrayState,
    paused: Option<PauseReason>,
    /// The server answered that it does not speak this program's protocol version: it, or this program, needs upgrading.
    incompatible: bool,
    log: VecDeque<LogEntry>,
    /// Fingerprint of the last content captured or applied, so the same content is not queued twice in a row.
    last_fp: Option<[u8; 32]>,
    device_id: String,
    joined: bool,
    mode: Mode,
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
    /// The engine of a local-only setup: it keeps items but has no device, no token and no queue.
    local: Mutex<Option<SyncEngine<SqliteStore>>>,
    /// A new vault whose recovery kit is on screen. Nothing of it is on the server or on disk until it is confirmed.
    pending: StdMutex<Option<NewVault>>,
    history: StdMutex<Option<HistoryStore>>,
    shared: StdMutex<Shared>,
    wake: Notify,
    /// What an update check found last, and the listener that is told about a new version.
    update_found: StdMutex<Option<UpdateInfo>>,
    update_listener: StdMutex<Option<Arc<dyn Fn(&UpdateInfo) + Send + Sync>>>,
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
            local: Mutex::new(None),
            pending: StdMutex::new(None),
            history: StdMutex::new(None),
            shared: StdMutex::new(Shared {
                settings,
                tray: TrayState::Synced,
                paused: None,
                incompatible: false,
                log: VecDeque::new(),
                last_fp: None,
                device_id: String::new(),
                joined: false,
                mode: Mode::Unset,
                started: false,
                initial_sync_done: false,
                listener: None,
            }),
            wake: Notify::new(),
            update_found: StdMutex::new(None),
            update_listener: StdMutex::new(None),
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
            mode: g.mode,
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
        lock(&self.0.shared).settings.clone()
    }

    pub fn set_settings(&self, s: Settings) -> Result<(), AppError> {
        s.save(&self.0.dir)?;
        lock(&self.0.shared).settings = s;
        Ok(())
    }

    // ---- first run, joining and starting ---------------------------------------------------------------------------

    /// A passphrase for the user who has none in mind: 100 bits, in groups that are easy to copy.
    pub fn generate_passphrase() -> Result<String, AppError> {
        onepaste_core::passphrase::generate().map_err(|e| AppError::Other(e.code().to_string()))
    }

    /// Whether the program is set up (in either mode) on this computer.
    pub fn is_joined(&self) -> bool {
        lock(&self.0.shared).joined
    }

    pub fn mode(&self) -> Mode {
        lock(&self.0.shared).mode
    }

    /// Opens what is already set up and says where the first-run screens should begin. Harmless to call again.
    pub async fn first_run_state(&self) -> Result<Mode, AppError> {
        self.start().await?;
        Ok(self.mode())
    }

    /// Fails if this computer is already set up. Otherwise clears what an interrupted setup may have left behind: files
    /// that no key in the secure store opens are of no use to anyone.
    async fn require_unset(&self) -> Result<(), AppError> {
        if self.start().await? {
            return Err(AppError::AlreadySetUp);
        }
        self.remove_local_files();
        Ok(())
    }

    fn remove_local_files(&self) {
        for name in [DB_FILE, "history.db", crate::settings::FILE_NAME, STATE_FILE_NAME] {
            for suffix in ["", "-wal", "-shm", "-journal", ".tmp"] {
                let _ = std::fs::remove_file(self.0.dir.join(format!("{name}{suffix}")));
            }
        }
    }

    /// Asks the server behind the code whether its vault exists. Registers nothing and stores nothing.
    pub async fn probe_server(&self, code: &str) -> Result<ServerVault, AppError> {
        Ok(match probe(code).await? {
            VaultState::Existing => ServerVault::Existing,
            VaultState::Empty => ServerVault::Empty,
        })
    }

    /// Joins an existing vault with the connection code and the passphrase. The passphrase is used once and dropped;
    /// the master key goes into the secure store. A wrong passphrase registers nothing.
    pub async fn join(&self, code: &str, passphrase: &str, device_name: &str) -> Result<(), AppError> {
        self.require_unset().await?;
        let result = join(
            &self.0.dir,
            JoinOptions {
                code: code.to_string(),
                device_name: device_name.to_string(),
                passphrase: Some(passphrase.to_string()),
                recovery_kit_out: None,
                existing_only: true,
                recovery_kit: None,
            },
        )
        .await
        .map_err(AppError::from)?;
        self.0.secrets.save(&result.master_key)?;
        self.log("joined", None);
        self.start().await?;
        Ok(())
    }

    /// Joins an existing vault with the recovery kit instead of the passphrase. A kit that is malformed or belongs to
    /// another vault registers nothing and stores no key.
    pub async fn join_with_kit(&self, code: &str, kit: &str, device_name: &str) -> Result<(), AppError> {
        self.require_unset().await?;
        let result = join(
            &self.0.dir,
            JoinOptions {
                code: code.to_string(),
                device_name: device_name.to_string(),
                passphrase: None,
                recovery_kit_out: None,
                existing_only: true,
                recovery_kit: Some(kit.to_string()),
            },
        )
        .await
        .map_err(AppError::from)?;
        self.0.secrets.save(&result.master_key)?;
        self.log("joined_with_recovery_kit", None);
        self.start().await?;
        Ok(())
    }

    /// Makes a new vault for an empty server and returns its recovery kit for the screen. Nothing is registered and
    /// nothing is written: the vault exists only in memory until [`App::confirm_kit_saved`]. Leaving now changes nothing.
    pub async fn create_vault(&self, code: &str, passphrase: &str, device_name: &str) -> Result<String, AppError> {
        if self.start().await? {
            return Err(AppError::AlreadySetUp);
        }
        let vault = prepare_new_vault(code, passphrase, device_name).await?;
        let kit = vault.recovery_kit().to_string();
        *lock(&self.0.pending) = Some(vault);
        Ok(kit)
    }

    /// The user has saved the recovery kit: only now is the vault registered on the server.
    pub async fn confirm_kit_saved(&self) -> Result<(), AppError> {
        self.require_unset().await?;
        let result = {
            let vault = lock(&self.0.pending).take().ok_or(AppError::NoPendingSetup)?;
            let r = vault.register(&self.0.dir).await;
            if r.is_err() {
                // Nothing was saved, so the same vault can be tried again
                *lock(&self.0.pending) = Some(vault);
            }
            r.map_err(AppError::from)?
        };
        self.0.secrets.save(&result.master_key)?;
        self.log("vault_created", None);
        self.start().await?;
        Ok(())
    }

    /// Drops a vault that was prepared but not confirmed (the user went back).
    pub fn cancel_setup(&self) {
        *lock(&self.0.pending) = None;
    }

    /// Keeps the history on this computer only: a random key goes into the secure store, no device is created and the
    /// network is never used.
    pub async fn choose_local_only(&self) -> Result<(), AppError> {
        self.require_unset().await?;
        let key = Key::random().map_err(|e| AppError::Other(e.code().to_string()))?;
        self.0.secrets.save(&key)?;
        StateFile { local_only: true, ..Default::default() }.save(&self.0.dir)?;
        self.log("local_only_chosen", None);
        self.start().await?;
        Ok(())
    }

    /// The recovery kit of the joined vault, computed from the key in the secure store. Local-only mode has none.
    pub fn show_kit(&self) -> Result<String, AppError> {
        if self.mode() != Mode::Synced {
            return Err(AppError::NoRecoveryKit);
        }
        let key = self.0.secrets.load()?.ok_or(AppError::NotJoined)?;
        Ok(recovery::encode(&key))
    }

    /// Deletes everything this program keeps on this computer: both databases, the two small files and the key in the
    /// secure store. The server is not touched. Afterwards the program is as on the first run.
    pub async fn wipe_local_data(&self) -> Result<(), AppError> {
        self.shutdown();
        *self.0.session.lock().await = None;
        *self.0.local.lock().await = None;
        *lock(&self.0.history) = None;
        *lock(&self.0.pending) = None;
        self.0.secrets.delete()?;
        self.remove_local_files();
        {
            let mut g = lock(&self.0.shared);
            g.settings = Settings::default();
            g.joined = false;
            g.started = false;
            g.mode = Mode::Unset;
            g.device_id.clear();
            g.last_fp = None;
            g.paused = None;
            g.tray = TrayState::Synced;
            g.initial_sync_done = false;
        }
        self.log("local_data_wiped", None);
        self.notify_status();
        Ok(())
    }

    /// Opens the vault with the key from the secure store (no passphrase) and starts the background work. Does nothing
    /// if no key is stored yet. Calling it again is harmless. Returns whether the program is set up.
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
        let state = StateFile::load(&self.0.dir);
        if state.local_only {
            let engine = SyncEngine::new_local(SqliteStore::open(&db_path(&self.0.dir)).map_err(AppError::from)?, Key::from_bytes(*mk.as_bytes()));
            let history = HistoryStore::open(&self.0.dir, &mk).map_err(AppError::from)?;
            *self.0.local.lock().await = Some(engine);
            *lock(&self.0.history) = Some(history);
            {
                let mut g = lock(&self.0.shared);
                g.joined = true;
                g.started = true;
                g.mode = Mode::LocalOnly;
                g.tray = TrayState::Local;
            }
            self.start_monitor();
            self.notify_status();
            return Ok(true);
        }
        // A key without a joined vault is what an interrupted setup leaves; the screens start over
        let joined_here = SqliteStore::open(&db_path(&self.0.dir)).ok().and_then(|s| s.meta_get("device_id").ok().flatten()).is_some();
        if !joined_here {
            return Ok(false);
        }
        let session = Session::open_with_key(&self.0.dir, &mk).map_err(AppError::from)?;
        let device_id = session.info.device_id.clone();
        let history = HistoryStore::open(&self.0.dir, &mk).map_err(AppError::from)?;
        *self.0.session.lock().await = Some(session);
        *lock(&self.0.history) = Some(history);
        {
            let mut g = lock(&self.0.shared);
            g.joined = true;
            g.started = true;
            g.mode = Mode::Synced;
            g.device_id = device_id;
            g.initial_sync_done = state.initial_sync_done;
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

    /// Runs `f` on the engine of whichever mode is active.
    async fn with_engine<R>(&self, f: impl FnOnce(&mut SyncEngine<SqliteStore>) -> R) -> Result<R, AppError> {
        if let Some(s) = self.0.session.lock().await.as_mut() {
            return Ok(f(&mut s.engine));
        }
        if let Some(e) = self.0.local.lock().await.as_mut() {
            return Ok(f(e));
        }
        Err(AppError::NotJoined)
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

    /// The monitor always runs; whether a copy is recorded is decided per event, so switching listening back on needs
    /// no restart.
    fn start_monitor(&self) {
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
        if self.settings().skip_suspected_secrets {
            if let Content::Text(text) = &content {
                if let Some(reason) = crate::secrets::detect(text) {
                    self.log("skipped_suspected_secret", Some(reason.code().into()));
                    return Ok(());
                }
            }
        }
        let fp = fingerprint(&content);
        if lock(&self.0.shared).last_fp == Some(fp) {
            self.log("skipped_duplicate", None);
            return Ok(());
        }
        let device = Some(lock(&self.0.shared).device_id.clone()).filter(|d| !d.is_empty());
        let (payload, kind, size) = match &content {
            Content::Text(t) => (ItemPayload { kind: "text".into(), content: Some(t.clone()), device, ..Default::default() }, "text", t.len()),
            Content::Image { png, width, height } => {
                let encoded = B64.encode(png);
                // The ciphertext is about the size of the encoded content; the server refuses anything above its limit
                if encoded.len() + 1024 > MAX_ITEM_BYTES {
                    self.log("image_too_large", Some(format!("{} bytes", png.len())));
                    return Err(AppError::ImageTooLarge);
                }
                let mut p = ItemPayload { kind: "image".into(), content: Some(encoded), device, ..Default::default() };
                p.extra.insert("width".into(), (*width).into());
                p.extra.insert("height".into(), (*height).into());
                p.extra.insert("mime".into(), "image/png".into());
                p.extra.insert("size".into(), png.len().into());
                (p, "image", png.len())
            }
        };
        self.with_engine(|e| e.enqueue_create(now_ms(), &payload)).await?.map_err(AppError::from)?;
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
        if self.mode() == Mode::LocalOnly {
            return Ok(());
        }
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
        lock(&self.0.shared).incompatible = false;
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
            if (StateFile { initial_sync_done: true, local_only: false }).save(&self.0.dir).is_ok() {
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
            _ if lock(&self.0.shared).incompatible => self.set_tray(TrayState::Error, None),
            RoundEnd::Offline => self.set_tray(TrayState::Offline, None),
            _ => self.set_tray(TrayState::Synced, None),
        }
        end
    }

    /// A server that refuses this program's protocol version must be visible, not a silent failure.
    fn note_incompatible(&self, code: &str) {
        if code == "unsupported_version" {
            let first = !std::mem::replace(&mut lock(&self.0.shared).incompatible, true);
            if first {
                self.log("server_incompatible", None);
            }
        }
    }

    fn note_event(&self, ev: &Event) {
        match ev {
            Event::Paused(r) => {
                lock(&self.0.shared).paused = Some(*r);
            }
            Event::Pushed { .. } => self.log("uploaded", None),
            Event::Pulled { applied, .. } if *applied > 0 => self.log("downloaded", Some(format!("{applied} items"))),
            Event::Conflict { .. } => self.log("conflict", None),
            Event::OpFailed { code, .. } => {
                self.note_incompatible(code);
                self.log("upload_failed", Some(code.clone()))
            }
            Event::PullFailed { code } => {
                self.note_incompatible(code);
                self.log("download_failed", Some(code.clone()))
            }
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
        let history = &self.0.history;
        let r = self
            .with_engine(|e| {
                let mk = e.master_key().clone();
                let mut h = lock(history);
                match h.as_mut() {
                    Some(h) => h.refresh(e.store_mut(), &mk).map(|_| ()),
                    None => Ok(()),
                }
            })
            .await;
        match r {
            Ok(r) => Ok(r?),
            Err(AppError::NotJoined) => Ok(()),
            Err(e) => Err(e),
        }
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
        if lock(&self.0.shared).incompatible {
            st.paused = Some("server_incompatible".to_string());
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
                if let Ok(Ok(p)) = self.with_engine(|e| e.get_item(&hit.item_id)).await {
                    width = p.extra.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    height = p.extra.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
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

    /// The picture of an image item. It is read from the history store and passed on: it is never logged, never put in the
    /// diagnostics and never written anywhere. A text item is `NotAnImage`.
    pub async fn item_image(&self, item_id: &str) -> Result<ImageData, AppError> {
        let payload = self.with_engine(|e| e.get_item(item_id)).await?.map_err(|_| AppError::NotFound)?;
        if payload.kind != "image" {
            return Err(AppError::NotAnImage);
        }
        let data = payload.content.clone().ok_or(AppError::NotFound)?;
        let dim = |k: &str| payload.extra.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let size = payload.extra.get("size").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or_else(|| B64.decode(&data).map(|b| b.len()).unwrap_or(0));
        Ok(ImageData { data, width: dim("width"), height: dim("height"), size, format: "png".into() })
    }

    /// The whole text of a text item (the list shows only its first line). Never logged. An image is `NotText`.
    pub async fn item_text(&self, item_id: &str) -> Result<String, AppError> {
        let payload = self.with_engine(|e| e.get_item(item_id)).await?.map_err(|_| AppError::NotFound)?;
        if payload.kind == "image" {
            return Err(AppError::NotText);
        }
        payload.content.ok_or(AppError::NotFound)
    }

    /// Puts an item back on the clipboard. The write carries the origin marker, so it does not become a new item.
    pub async fn copy(&self, item_id: &str) -> Result<(), AppError> {
        let payload = self.with_engine(|e| e.get_item(item_id)).await?.map_err(|_| AppError::NotFound)?;
        let content = content_of(&payload).ok_or(AppError::NotFound)?;
        self.0.clipboard.write(&content, true)
    }

    pub async fn set_flags(&self, item_id: &str, favorite: Option<bool>, pinned: Option<bool>, note: Option<String>) -> Result<(), AppError> {
        self.with_engine(|e| -> Result<(), onepaste_sync::Error> {
            if let Some(v) = favorite {
                e.enqueue_edit(now_ms(), item_id, Edit::Favorite(v))?;
            }
            if let Some(v) = pinned {
                e.enqueue_edit(now_ms(), item_id, Edit::Pinned(v))?;
            }
            if let Some(n) = note {
                e.enqueue_edit(now_ms(), item_id, Edit::Note(if n.is_empty() { None } else { Some(n) }))?;
            }
            Ok(())
        })
        .await?
        .map_err(AppError::from)?;
        self.0.wake.notify_one();
        Ok(())
    }

    pub async fn delete(&self, item_id: &str) -> Result<(), AppError> {
        self.with_engine(|e| e.enqueue_delete(now_ms(), item_id)).await?.map_err(AppError::from)?;
        self.0.wake.notify_one();
        Ok(())
    }

    /// Text for the "copy diagnostics" button: versions, state, counts and recent event kinds. No content and no keys.
    pub async fn diagnostics(&self) -> String {
        let st = self.status().await;
        let log = self.event_log();
        let mut out = String::new();
        out.push_str(&format!("joined: {}\nmode: {:?}\n", st.joined, st.mode));
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

    /// A handle for plain server calls, taken while the session lock is held and used after it is released.
    async fn api(&self) -> Result<(onepaste_client::ApiClient, Key, String), AppError> {
        let g = self.0.session.lock().await;
        let s = g.as_ref().ok_or(AppError::NotJoined)?;
        Ok((s.api(), Key::from_bytes(*s.master_key().as_bytes()), s.info.device_id.clone()))
    }

    /// The devices of the vault, with their names opened. Needs the server.
    pub async fn devices(&self) -> Result<DeviceList, AppError> {
        use base64::engine::general_purpose::STANDARD as B64;
        use onepaste_core::crypto::{self, aad};
        let (api, mk, me) = self.api().await?;
        let rows = api.call(onepaste_sync::Method::Get, "/v1/devices", None).await.map_err(AppError::from)?;
        let mut devices = Vec::new();
        for d in rows.as_array().cloned().unwrap_or_default() {
            let id = d["device_id"].as_str().unwrap_or("").to_string();
            let name = d["sealed_name"]
                .as_str()
                .and_then(|n| B64.decode(n).ok())
                .and_then(|raw| crypto::open(&mk, &aad::device(&id), &raw).ok())
                .and_then(|b| String::from_utf8(b).ok());
            devices.push(DeviceView {
                this_device: id == me,
                device_id: id,
                name,
                created_at: d["created_at"].as_i64().unwrap_or(0),
                last_seen_at: d["last_seen_at"].as_i64(),
                revoked: d["revoked"].as_bool().unwrap_or(false),
            });
        }
        let server_url = self.0.session.lock().await.as_ref().map(|s| s.info.url.clone()).unwrap_or_default();
        Ok(DeviceList { devices, server_url })
    }

    /// Removes another device from the vault: it can no longer sync. This device cannot revoke itself here.
    pub async fn revoke_device(&self, device_id: &str) -> Result<(), AppError> {
        let (api, _, me) = self.api().await?;
        if device_id == me {
            return Err(AppError::CannotRevokeThisDevice);
        }
        api.call(onepaste_sync::Method::Delete, &format!("/v1/devices/{device_id}"), None).await.map_err(AppError::from)?;
        self.log("device_revoked", None);
        Ok(())
    }

    /// A new connection code for adding another device. It replaces the unused ones.
    pub async fn new_connection_code(&self) -> Result<String, AppError> {
        let (api, _, _) = self.api().await?;
        let code = api.invite().await.map_err(AppError::from)?;
        self.log("connection_code_made", None);
        Ok(code)
    }

    // ---- updates ---------------------------------------------------------------------------------------------------

    /// Registers a function that is told when a newer version is found by a background check.
    pub fn on_update_available(&self, f: impl Fn(&UpdateInfo) + Send + Sync + 'static) {
        *lock(&self.0.update_listener) = Some(Arc::new(f));
    }

    /// The newer version the last check found, if there is one that has not been skipped.
    pub fn available_update(&self) -> Option<UpdateInfo> {
        lock(&self.0.update_found).clone()
    }

    /// The channel in use: the user's choice, or the default for this version.
    pub fn update_channel(&self) -> Channel {
        self.settings().update_channel.unwrap_or_else(|| update::default_channel(&self.0.cfg.update_version))
    }

    /// Looks for a newer version now. A background check (`manual` false) that fails says nothing: it is noted in the
    /// event log and the answer is "nothing new". A check the user asked for returns the reason. A version the user
    /// skipped is not reported by a background check; a manual check reports it again.
    pub async fn check_for_update(&self, manual: bool) -> Result<Option<UpdateInfo>, AppError> {
        let cfg = &self.0.cfg;
        let result = match &cfg.update_source {
            Some(setup) => update::check(setup.source.as_ref(), &setup.releases_url, &setup.public_key, &cfg.update_version, cfg.built_at, self.update_channel(), update::platform_key()).await,
            None => match HttpsSource::new(&cfg.update_version) {
                Ok(src) => update::check(&src, update::RELEASES_URL, update::PUBLIC_KEY, &cfg.update_version, cfg.built_at, self.update_channel(), update::platform_key()).await,
                Err(e) => Err(e),
            },
        };
        match result {
            Ok(found) => {
                let found = found.filter(|i| manual || self.settings().skipped_update.as_deref() != Some(i.version.as_str()));
                *lock(&self.0.update_found) = found.clone();
                if let (Some(info), false) = (&found, manual) {
                    let listener = lock(&self.0.update_listener).clone();
                    if let Some(l) = listener {
                        l(info);
                    }
                }
                self.log("update_checked", None);
                Ok(found)
            }
            Err(e) => {
                self.log("update_check_failed", Some(e.code().to_string()));
                if manual {
                    Err(e.into())
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// The user chose "skip this version": it is not offered again, a newer one is.
    pub fn skip_update(&self, version: &str) -> Result<(), AppError> {
        let mut s = self.settings();
        s.skipped_update = Some(version.to_string());
        self.set_settings(s)?;
        *lock(&self.0.update_found) = None;
        Ok(())
    }

    /// Starts the background checks: one now, then one every six hours, as long as the setting is on.
    pub fn start_update_checks(&self) {
        if !self.0.cfg.update_checks {
            return;
        }
        let app = self.clone();
        lock(&self.0.tasks).push(tokio::spawn(async move {
            loop {
                if app.settings().check_updates {
                    let _ = app.check_for_update(false).await;
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        }));
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

