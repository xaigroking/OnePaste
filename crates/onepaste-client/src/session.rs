//! An unlocked local vault: the sync engine on top of SQLite, plus the HTTPS transport.

use crate::conn::ConnectionCode;
use crate::error::ClientError;
use crate::sqlite::SqliteStore;
use crate::transport::{event_loop, Transport};
use onepaste_core::crypto::{self, aad, Key};
use onepaste_core::keys::KeyParams;
use onepaste_sync::{Action, Engine, Event, HttpRequest, Method, PauseReason};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DB_FILE: &str = "onepaste.db";

pub fn db_path(dir: &Path) -> PathBuf {
    dir.join(DB_FILE)
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct Info {
    pub url: String,
    pub fingerprint: String,
    pub device_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunEnd {
    /// Everything that could be done has been done.
    Idle,
    /// Stopped because syncing is paused.
    Paused(PauseReason),
    /// A retry is scheduled for later (one-shot runs do not wait for it).
    Waiting,
}

#[derive(Debug)]
pub struct RunReport {
    pub end: RunEnd,
    pub pushed: usize,
    pub pulled: usize,
    pub events: Vec<Event>,
}

pub struct Session {
    pub engine: Engine<SqliteStore>,
    pub transport: Transport,
    pub info: Info,
    mk: Key,
    token: String,
}

impl Session {
    /// Opens the local vault and unlocks it with the passphrase. A wrong passphrase is `WrongPassphrase`.
    pub fn open(dir: &Path, passphrase: &str) -> Result<Session, ClientError> {
        let store = SqliteStore::open(&db_path(dir))?;
        let url = store.meta_get("u")?.ok_or(ClientError::NotJoined)?;
        let fingerprint = store.meta_get("fp")?.ok_or(ClientError::NotJoined)?;
        let device_id = store.meta_get("device_id")?.ok_or(ClientError::NotJoined)?;
        let record: KeyParams = serde_json::from_str(&store.meta_get("keyparams")?.ok_or(ClientError::NotJoined)?)
            .map_err(|_| ClientError::Storage("keyparams".into()))?;
        let mk = record.unlock(passphrase)?;
        let sealed = store.secret_get("devtoken")?.ok_or(ClientError::NotJoined)?;
        let token = String::from_utf8(crypto::open(&mk, aad::DEVTOKEN, &sealed)?).map_err(|_| ClientError::Storage("devtoken".into()))?;
        let transport = Transport::new(&url, &fingerprint);
        let engine = Engine::new(store, Key::from_bytes(*mk.as_bytes()), token.clone());
        Ok(Session { engine, transport, info: Info { url, fingerprint, device_id }, mk, token })
    }

    pub fn master_key(&self) -> &Key {
        &self.mk
    }

    /// Drives the engine until it has nothing left to do. With `retry` it waits out back-off delays; without it,
    /// the first network failure is returned as an error and a scheduled retry ends the run.
    pub async fn run(&mut self, retry: bool) -> Result<RunReport, ClientError> {
        let mut report = RunReport { end: RunEnd::Idle, pushed: 0, pulled: 0, events: Vec::new() };
        loop {
            match self.engine.next_action(now_ms())? {
                Action::Send(req) => match self.transport.send(&req).await {
                    Ok(resp) => {
                        for ev in self.engine.on_response(now_ms(), resp)? {
                            match &ev {
                                Event::Pushed { .. } => report.pushed += 1,
                                Event::Pulled { applied, .. } => report.pulled += applied,
                                _ => {}
                            }
                            report.events.push(ev);
                        }
                    }
                    Err(e) => {
                        self.engine.on_transport_error(now_ms())?;
                        if !retry {
                            return Err(e);
                        }
                    }
                },
                Action::Wait { until_ms } => {
                    if !retry {
                        report.end = RunEnd::Waiting;
                        return Ok(report);
                    }
                    tokio::time::sleep(Duration::from_millis(until_ms.saturating_sub(now_ms()).max(1))).await;
                }
                Action::Idle => return Ok(report),
                Action::Paused(r) => {
                    report.end = RunEnd::Paused(r);
                    return Ok(report);
                }
                Action::InFlight => return Err(ClientError::Other("a request is already outstanding".into())),
            }
        }
    }

    /// Stays running: listens for change notifications and pulls, and uploads whatever is queued. Returns only when
    /// syncing gets paused.
    pub async fn watch(&mut self) -> Result<(), ClientError> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let listener = tokio::spawn(event_loop(self.transport.clone(), self.token.clone(), tx));
        let result = async {
            self.engine.request_pull();
            loop {
                let report = self.run(true).await?;
                if let RunEnd::Paused(r) = report.end {
                    return Err(ClientError::Paused(r));
                }
                tokio::select! {
                    msg = rx.recv() => match msg {
                        Some(Some(seq)) => self.engine.note_event_seq(seq)?,
                        Some(None) => self.engine.request_pull(),
                        None => return Ok(()),
                    },
                    // Safety net: also notice work queued by other commands, and a notification that never arrived
                    _ = tokio::time::sleep(Duration::from_secs(30)) => self.engine.request_pull(),
                }
            }
        }
        .await;
        listener.abort();
        result
    }

    /// A plain API call outside the engine (devices, invitations). Write calls carry a fresh idempotency key.
    pub async fn call(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, ClientError> {
        let mut headers = vec![
            ("OnePaste-Proto".to_string(), "1".to_string()),
            ("Authorization".to_string(), format!("Bearer {}", self.token)),
        ];
        if method != Method::Get {
            headers.push(("Idempotency-Key".to_string(), onepaste_core::id::new_id()?));
        }
        let req = HttpRequest { method, path_and_query: path.to_string(), headers, body: body.map(|b| b.to_string().into_bytes()).unwrap_or_default() };
        let resp = self.transport.send(&req).await?;
        let json: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
        match resp.status {
            200..=299 => Ok(json),
            401 => Err(ClientError::Paused(PauseReason::Revoked)),
            status => Err(ClientError::Server { status, code: json.get("code").and_then(Value::as_str).unwrap_or("unknown").to_string() }),
        }
    }

    /// Asks the server for a new registration credential and builds the connection code for another device.
    pub async fn invite(&self) -> Result<String, ClientError> {
        let v = self.call(Method::Post, "/v1/devices/invite", None).await?;
        let t = v.get("reg_token").and_then(Value::as_str).ok_or_else(|| ClientError::Other("malformed invitation".into()))?;
        Ok(ConnectionCode { u: self.info.url.clone(), fp: self.info.fingerprint.clone(), t: t.to_string() }.encode())
    }
}
