//! The sync engine: a state machine with no IO. The driver sends the requests it asks for and feeds the responses back.

use crate::error::Error;
use crate::intent::{open_blob, open_envelope, parse_envelope, reseal, seal_blob, secs, Edit, Intent, OpBlob};
use crate::store::{Store, StoreError, Tx};
use crate::types::*;
use onepaste_core::crypto::Key;
use onepaste_core::envelope::{seal_item, ItemPayload};
use onepaste_core::id::new_id_at;
use serde_json::{json, Value};
use std::collections::HashMap;
use zeroize::Zeroizing;

/// What the driver should do next.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Send this request and hand the response (or a transport error) back.
    Send(HttpRequest),
    /// Nothing to do until this time (milliseconds on the caller's clock).
    Wait { until_ms: u64 },
    /// Nothing to do.
    Idle,
    /// Syncing is stopped until the user resolves the reason.
    Paused(PauseReason),
    /// A request is already outstanding.
    InFlight,
}

/// Facts the driver may want to show.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Pushed { op_id: u64, item_id: String, seq: i64 },
    /// The operation was dropped because the item was deleted elsewhere (deleting wins).
    OpDropped { op_id: u64, item_id: String },
    Conflict { op_id: u64, item_id: String },
    OpFailed { op_id: u64, item_id: String, code: String },
    Pulled { applied: usize, skipped: usize, has_more: bool },
    SkippedRecord { id: String, code: &'static str },
    Refetching,
    Paused(PauseReason),
    PullFailed { code: String },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub cursor: i64,
    pub max_seen_seq: i64,
    pub paused: Option<PauseReason>,
    pub refetching: bool,
    pub pending: usize,
    pub conflict: usize,
    pub failed: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ItemSummary {
    pub id: String,
    pub seq: i64,
    pub deleted: bool,
    pub decrypt_failed: bool,
    /// The first line of the content, at most 60 characters.
    pub first_line: String,
    pub favorite: bool,
    pub pinned: bool,
    pub note: Option<String>,
    pub tags: Vec<String>,
    /// The item still has changes waiting to be uploaded.
    pub queued: bool,
}

const DEFAULT_PAGE_SIZE: u32 = 200;

pub(crate) enum InFlight {
    Push { op_id: u64 },
    Pull,
}

pub struct Engine<S: Store> {
    pub(crate) store: S,
    pub(crate) mk: Key,
    token: Zeroizing<String>,
    pub(crate) inflight: Option<InFlight>,
    pub(crate) pull_wanted: bool,
    pub(crate) push_retry_at: u64,
    pub(crate) pull_retry_at: u64,
    pub(crate) pull_attempts: u32,
    /// How many times in a row a full pull was restarted because the server kept answering `cursor_expired`.
    pub(crate) refetch_restarts: u32,
    /// Pulling is switched off for this run (the server kept refusing the full pull) until the next `request_pull`.
    pub(crate) pull_suspended: bool,
    pub(crate) stale_streak: HashMap<u64, u32>,
    page_size: u32,
}

/// Runs `f` as one storage transaction. If `f` fails, nothing it did is kept.
pub(crate) fn run_tx<S: Store, R>(store: &mut S, f: impl FnOnce(&mut dyn Tx) -> Result<R, Error>) -> Result<R, Error> {
    let mut f = Some(f);
    let mut out: Option<Result<R, Error>> = None;
    let res = store.transact(&mut |tx| {
        let f = f.take().ok_or(StoreError::Aborted)?;
        match f(tx) {
            Ok(r) => {
                out = Some(Ok(r));
                Ok(())
            }
            Err(e) => {
                out = Some(Err(e));
                Err(StoreError::Aborted)
            }
        }
    });
    match out {
        Some(Err(e)) => Err(e),
        Some(Ok(r)) => {
            res?;
            Ok(r)
        }
        None => {
            res?;
            Err(Error::Store(StoreError::Aborted))
        }
    }
}

pub(crate) fn backoff_ms(attempts: u32) -> u64 {
    (1u64 << attempts.saturating_sub(1).min(6)).min(60) * 1000
}

impl<S: Store> Engine<S> {
    /// `mk` is the already unlocked master key; the engine never sees the passphrase.
    pub fn new(store: S, mk: Key, device_token: String) -> Engine<S> {
        Engine {
            store,
            mk,
            token: Zeroizing::new(device_token),
            inflight: None,
            pull_wanted: false,
            push_retry_at: 0,
            pull_retry_at: 0,
            pull_attempts: 0,
            refetch_restarts: 0,
            pull_suspended: false,
            stale_streak: HashMap::new(),
            page_size: DEFAULT_PAGE_SIZE,
        }
    }

    /// Items requested per pull page (the default is 200).
    pub fn set_page_size(&mut self, n: u32) {
        self.page_size = n.clamp(1, 500);
    }

    pub fn into_store(self) -> S {
        self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub(crate) fn read<R>(&mut self, f: impl FnOnce(&mut dyn Tx) -> Result<R, Error>) -> Result<R, Error> {
        run_tx(&mut self.store, f)
    }

    pub(crate) fn headers(&self, idem: Option<&str>) -> Vec<(String, String)> {
        let mut h = vec![
            ("OnePaste-Proto".to_string(), "1".to_string()),
            ("Authorization".to_string(), format!("Bearer {}", self.token.as_str())),
        ];
        if let Some(k) = idem {
            h.push(("Idempotency-Key".to_string(), k.to_string()));
            h.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        h
    }

    // ---- local changes -------------------------------------------------------------------------------------------

    /// Queues a new item and shows it locally at once. Returns its id.
    pub fn enqueue_create(&mut self, now_ms: u64, payload: &ItemPayload) -> Result<String, Error> {
        let id = new_id_at(now_ms)?;
        let created = secs(now_ms);
        let env_json = seal_item(&self.mk, &id, created, created, None, payload)?.to_json()?;
        let idem = new_id_at(now_ms)?;
        let mk = &self.mk;
        run_tx(&mut self.store, |tx| {
            let mut meta = tx.meta()?;
            let op_id = take_op_id(&mut meta);
            let blob = OpBlob { intent: Intent::Create, envelope: Some(env_json.clone()) };
            tx.put_outbox(&new_op(op_id, &id, seal_blob(mk, op_id, &blob)?, &idem))?;
            tx.put_item(&ItemRow { id: id.clone(), seq: 0, deleted: false, envelope_json: Some(env_json.clone()), mark: meta.epoch })?;
            tx.set_meta(&meta)?;
            Ok(())
        })?;
        Ok(id)
    }

    /// Queues a change to one field of an item.
    pub fn enqueue_edit(&mut self, now_ms: u64, id: &str, edit: Edit) -> Result<(), Error> {
        if matches!(edit, Edit::Kind(_) | Edit::Content(_)) {
            return Err(Error::BodyImmutable);
        }
        let idem = new_id_at(now_ms)?;
        let mk = &self.mk;
        run_tx(&mut self.store, |tx| {
            let mut row = tx.item(id)?.ok_or(Error::NotFound)?;
            let json = match (&row.envelope_json, row.deleted) {
                (Some(j), false) => j.clone(),
                _ => return Err(Error::Deleted),
            };
            let env = parse_envelope(&json)?;
            let mut payload = open_envelope(mk, &env)?;
            edit.apply(&mut payload)?;
            let new_json = reseal(mk, &env, secs(now_ms), &payload)?;
            let mut meta = tx.meta()?;
            let op_id = take_op_id(&mut meta);
            let blob = OpBlob { intent: Intent::Edit { edit: edit.clone() }, envelope: Some(new_json.clone()) };
            tx.put_outbox(&new_op(op_id, id, seal_blob(mk, op_id, &blob)?, &idem))?;
            row.envelope_json = Some(new_json);
            tx.put_item(&row)?;
            tx.set_meta(&meta)?;
            Ok(())
        })
    }

    /// Queues a delete. An item that never reached the server is simply forgotten.
    pub fn enqueue_delete(&mut self, now_ms: u64, id: &str) -> Result<(), Error> {
        let idem = new_id_at(now_ms)?;
        let inflight_op = match &self.inflight {
            Some(InFlight::Push { op_id }) => Some(*op_id),
            _ => None,
        };
        let mk = &self.mk;
        run_tx(&mut self.store, |tx| {
            let mut row = tx.item(id)?.ok_or(Error::NotFound)?;
            if row.deleted {
                return Err(Error::Deleted);
            }
            let ops: Vec<OutboxRow> = tx.outbox()?.into_iter().filter(|o| o.item_id == id).collect();
            let never_sent = row.seq == 0 && ops.iter().all(|o| o.attempts == 0 && Some(o.op_id) != inflight_op);
            if never_sent {
                for o in &ops {
                    tx.remove_outbox(o.op_id)?;
                }
                tx.remove_item(id)?;
                return Ok(());
            }
            let mut meta = tx.meta()?;
            let op_id = take_op_id(&mut meta);
            let blob = OpBlob { intent: Intent::Delete, envelope: None };
            tx.put_outbox(&new_op(op_id, id, seal_blob(mk, op_id, &blob)?, &idem))?;
            row.deleted = true;
            row.envelope_json = None;
            tx.put_item(&row)?;
            tx.set_meta(&meta)?;
            Ok(())
        })
    }

    // ---- reading -------------------------------------------------------------------------------------------------

    pub fn get_item(&mut self, id: &str) -> Result<ItemPayload, Error> {
        let mk = &self.mk;
        run_tx(&mut self.store, |tx| {
            let row = tx.item(id)?.ok_or(Error::NotFound)?;
            match (&row.envelope_json, row.deleted) {
                (Some(j), false) => open_envelope(mk, &parse_envelope(j)?),
                _ => Err(Error::Deleted),
            }
        })
    }

    pub fn list_items(&mut self) -> Result<Vec<ItemSummary>, Error> {
        let mk = &self.mk;
        run_tx(&mut self.store, |tx| {
            let queued: std::collections::HashSet<String> = tx.outbox()?.into_iter().map(|o| o.item_id).collect();
            let mut out = Vec::new();
            for row in tx.items()? {
                let mut s = ItemSummary {
                    id: row.id.clone(),
                    seq: row.seq,
                    deleted: row.deleted,
                    decrypt_failed: false,
                    first_line: String::new(),
                    favorite: false,
                    pinned: false,
                    note: None,
                    tags: Vec::new(),
                    queued: queued.contains(&row.id),
                };
                if let (Some(j), false) = (&row.envelope_json, row.deleted) {
                    match parse_envelope(j).and_then(|e| open_envelope(mk, &e)) {
                        Ok(p) => {
                            s.first_line = p.content.as_deref().unwrap_or("").lines().next().unwrap_or("").chars().take(60).collect();
                            s.favorite = p.favorite;
                            s.pinned = p.pinned;
                            s.note = p.note;
                            s.tags = p.tags;
                        }
                        Err(_) => s.decrypt_failed = true,
                    }
                }
                out.push(s);
            }
            Ok(out)
        })
    }

    pub fn status(&mut self) -> Result<Status, Error> {
        self.read(|tx| {
            let m = tx.meta()?;
            let ob = tx.outbox()?;
            let count = |s: OpState| ob.iter().filter(|o| o.state == s).count();
            Ok(Status {
                cursor: m.cursor,
                max_seen_seq: m.max_seen_seq,
                paused: m.paused,
                refetching: m.refetching,
                pending: count(OpState::Pending),
                conflict: count(OpState::Conflict),
                failed: count(OpState::Failed),
            })
        })
    }

    pub fn outbox(&mut self) -> Result<Vec<OutboxRow>, Error> {
        self.read(|tx| Ok(tx.outbox()?))
    }

    // ---- driving -------------------------------------------------------------------------------------------------

    /// Asks for a pull on the next `next_action`.
    pub fn request_pull(&mut self) {
        self.pull_wanted = true;
        self.pull_suspended = false;
        self.pull_retry_at = 0;
        self.pull_attempts = 0;
    }

    /// A change notification arrived: remember the sequence number it announced and pull.
    pub fn note_event_seq(&mut self, seq: i64) -> Result<(), Error> {
        self.read(|tx| {
            let mut m = tx.meta()?;
            if seq > m.max_seen_seq {
                m.max_seen_seq = seq;
                tx.set_meta(&m)?;
            }
            Ok(())
        })?;
        self.request_pull();
        Ok(())
    }

    /// Manual recovery after a pause: forgets the local items, the cursor and the highest sequence number seen,
    /// keeps the outbox and pulls everything again. Items that exist only locally are not uploaded again.
    pub fn resync(&mut self) -> Result<(), Error> {
        self.read(|tx| {
            let mut m = tx.meta()?;
            tx.clear_items()?;
            m.cursor = 0;
            m.max_seen_seq = 0;
            m.paused = None;
            m.refetching = false;
            tx.set_meta(&m)?;
            Ok(())
        })?;
        self.inflight = None;
        self.request_pull();
        Ok(())
    }

    pub fn next_action(&mut self, now_ms: u64) -> Result<Action, Error> {
        if self.inflight.is_some() {
            return Ok(Action::InFlight);
        }
        loop {
            let (meta, ops) = self.read(|tx| Ok((tx.meta()?, tx.outbox()?)))?;
            if let Some(reason) = meta.paused {
                return Ok(Action::Paused(reason));
            }
            if let Some(op) = ops.iter().find(|o| o.state == OpState::Pending) {
                if now_ms < self.push_retry_at {
                    return Ok(Action::Wait { until_ms: self.push_retry_at });
                }
                match self.build_push(op) {
                    Ok(req) => {
                        self.inflight = Some(InFlight::Push { op_id: op.op_id });
                        return Ok(Action::Send(req));
                    }
                    Err(e) => {
                        // An operation that cannot be opened or built is parked; the rest of the queue goes on.
                        self.fail_op(op.op_id, e.code())?;
                        continue;
                    }
                }
            }
            if (self.pull_wanted || meta.refetching) && !self.pull_suspended {
                if now_ms < self.pull_retry_at {
                    return Ok(Action::Wait { until_ms: self.pull_retry_at });
                }
                self.inflight = Some(InFlight::Pull);
                return Ok(Action::Send(HttpRequest {
                    method: Method::Get,
                    path_and_query: format!("/v1/items?after={}&limit={}", meta.cursor, self.page_size),
                    headers: self.headers(None),
                    body: Vec::new(),
                }));
            }
            return Ok(Action::Idle);
        }
    }

    fn build_push(&mut self, op: &OutboxRow) -> Result<HttpRequest, Error> {
        let blob = open_blob(&self.mk, op.op_id, &op.sealed_intent)?;
        let base_seq = self.read(|tx| Ok(tx.item(&op.item_id)?.map(|r| r.seq).unwrap_or(0)))?;
        let body = match (&blob.intent, &blob.envelope) {
            (Intent::Delete, _) => json!({ "base_seq": base_seq, "deleted": true }),
            (_, Some(env)) => {
                let env: Value = serde_json::from_str(env).map_err(|_| Error::MalformedResponse)?;
                json!({ "base_seq": base_seq, "envelope": env })
            }
            _ => return Err(Error::MalformedResponse),
        };
        Ok(HttpRequest {
            method: Method::Put,
            path_and_query: format!("/v1/items/{}", op.item_id),
            headers: self.headers(Some(&op.idem_key)),
            body: body.to_string().into_bytes(),
        })
    }

    /// Parks an operation as failed with a stable code.
    pub(crate) fn fail_op(&mut self, op_id: u64, code: &str) -> Result<(), Error> {
        self.read(|tx| {
            if let Some(mut o) = tx.outbox_row(op_id)? {
                o.state = OpState::Failed;
                o.last_error = Some(code.to_string());
                tx.put_outbox(&o)?;
            }
            Ok(())
        })
    }

    /// The request got no usable answer (could not connect, timed out, TLS failure). The operation is retried with
    /// the same idempotency key after an exponential delay.
    pub fn on_transport_error(&mut self, now_ms: u64) -> Result<(), Error> {
        match self.inflight.take() {
            Some(InFlight::Push { op_id }) => self.bump_push_attempt(now_ms, op_id, None),
            Some(InFlight::Pull) => {
                self.pull_attempts += 1;
                self.pull_retry_at = now_ms + backoff_ms(self.pull_attempts);
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub(crate) fn bump_push_attempt(&mut self, now_ms: u64, op_id: u64, retry_after_secs: Option<u64>) -> Result<(), Error> {
        let attempts = self.read(|tx| {
            let Some(mut o) = tx.outbox_row(op_id)? else { return Ok(1) };
            o.attempts += 1;
            tx.put_outbox(&o)?;
            Ok(o.attempts)
        })?;
        let wait = retry_after_secs.map(|s| s.clamp(1, 3600) * 1000).unwrap_or_else(|| backoff_ms(attempts));
        self.push_retry_at = now_ms + wait;
        Ok(())
    }

    pub fn on_response(&mut self, now_ms: u64, resp: HttpResponse) -> Result<Vec<Event>, Error> {
        match self.inflight.take() {
            Some(InFlight::Push { op_id }) => self.on_push_response(now_ms, op_id, resp),
            Some(InFlight::Pull) => self.on_pull_response(now_ms, resp),
            None => Ok(Vec::new()),
        }
    }
}

pub(crate) fn take_op_id(meta: &mut SyncMeta) -> u64 {
    let id = meta.next_op_id;
    meta.next_op_id += 1;
    id
}

pub(crate) fn new_op(op_id: u64, item_id: &str, sealed: Vec<u8>, idem: &str) -> OutboxRow {
    OutboxRow {
        op_id,
        item_id: item_id.to_string(),
        sealed_intent: sealed,
        idem_key: idem.to_string(),
        attempts: 0,
        state: OpState::Pending,
        last_error: None,
    }
}
