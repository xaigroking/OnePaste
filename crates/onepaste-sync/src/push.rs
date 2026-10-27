//! Handling of the responses to uploads.

use crate::engine::{run_tx, Engine, Event};
use crate::error::Error;
use crate::intent::{open_blob, open_envelope, reseal, seal_blob, secs, Intent};
use crate::record::{parse_record, Record};
use crate::store::Store;
use crate::types::*;
use onepaste_core::id::new_id_at;
use serde_json::Value;

/// How many `stale` answers in a row an operation gets before it is parked as a conflict.
const MAX_STALE: u32 = 3;

pub(crate) fn json_body(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

/// The stable code from an error body, or `http_<status>` when there is none.
pub(crate) fn error_code(resp: &HttpResponse) -> String {
    json_body(&resp.body)
        .and_then(|v| v.get("code").and_then(Value::as_str).map(str::to_string))
        .filter(|c| !c.is_empty() && c.len() <= 64 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .unwrap_or_else(|| format!("http_{}", resp.status))
}

pub(crate) fn retry_after(resp: &HttpResponse) -> Option<u64> {
    resp.header("retry-after").and_then(|v| v.trim().parse().ok())
}

enum Rebased {
    /// The operation is ready to be sent again (with a new key).
    Retry,
    /// The item was deleted elsewhere; these operations were dropped.
    Dropped(Vec<u64>),
}

impl<S: Store> Engine<S> {
    pub(crate) fn pause(&mut self, reason: PauseReason) -> Result<Vec<Event>, Error> {
        self.read(|tx| {
            let mut m = tx.meta()?;
            m.paused = Some(reason);
            tx.set_meta(&m)?;
            Ok(())
        })?;
        self.pull_wanted = false;
        Ok(vec![Event::Paused(reason)])
    }

    pub(crate) fn on_push_response(&mut self, now_ms: u64, op_id: u64, resp: HttpResponse) -> Result<Vec<Event>, Error> {
        let Some(op) = self.read(|tx| Ok(tx.outbox_row(op_id)?))? else { return Ok(Vec::new()) };
        match resp.status {
            200 => {
                let seq = json_body(&resp.body).and_then(|v| v.get("seq").and_then(Value::as_i64)).filter(|n| *n >= 1);
                let Some(seq) = seq else {
                    // A success without a usable body is treated like a lost answer: retry with the same key
                    self.bump_push_attempt(now_ms, op_id, None)?;
                    return Ok(Vec::new());
                };
                let item_id = op.item_id.clone();
                self.read(|tx| {
                    let mut m = tx.meta()?;
                    m.max_seen_seq = m.max_seen_seq.max(seq);
                    tx.set_meta(&m)?;
                    if let Some(mut row) = tx.item(&item_id)? {
                        row.seq = seq;
                        tx.put_item(&row)?;
                    }
                    tx.remove_outbox(op_id)?;
                    Ok(())
                })?;
                self.stale_streak.remove(&op_id);
                self.push_retry_at = 0;
                Ok(vec![Event::Pushed { op_id, item_id: op.item_id, seq }])
            }
            409 => self.on_stale(now_ms, op, &resp),
            401 => self.pause(PauseReason::Revoked),
            429 | 500..=599 => {
                self.bump_push_attempt(now_ms, op_id, retry_after(&resp))?;
                Ok(Vec::new())
            }
            _ => {
                let code = error_code(&resp);
                self.fail_op(op_id, &code)?;
                self.stale_streak.remove(&op_id);
                self.push_retry_at = 0;
                Ok(vec![Event::OpFailed { op_id, item_id: op.item_id, code }])
            }
        }
    }

    fn park(&mut self, op: &OutboxRow, state: OpState, code: &str) -> Result<Vec<Event>, Error> {
        let op_id = op.op_id;
        self.read(|tx| {
            if let Some(mut o) = tx.outbox_row(op_id)? {
                o.state = state;
                o.last_error = Some(code.to_string());
                tx.put_outbox(&o)?;
            }
            Ok(())
        })?;
        self.stale_streak.remove(&op_id);
        self.push_retry_at = 0;
        let item_id = op.item_id.clone();
        Ok(vec![if state == OpState::Conflict {
            Event::Conflict { op_id, item_id }
        } else {
            Event::OpFailed { op_id, item_id, code: code.to_string() }
        }])
    }

    fn on_stale(&mut self, now_ms: u64, op: OutboxRow, resp: &HttpResponse) -> Result<Vec<Event>, Error> {
        let streak = {
            let c = self.stale_streak.entry(op.op_id).or_insert(0);
            *c += 1;
            *c
        };
        let blob = match open_blob(&self.mk, op.op_id, &op.sealed_intent) {
            Ok(b) => b,
            Err(e) => return self.park(&op, OpState::Failed, e.code()),
        };
        let current = json_body(&resp.body).and_then(|v| v.get("current_item").cloned());
        let Some(current) = current else { return self.park(&op, OpState::Conflict, "stale") };
        // A new item whose id already exists, or the third stale answer in a row, is left to the user
        if matches!(blob.intent, Intent::Create) || streak >= MAX_STALE {
            return self.park(&op, OpState::Conflict, "stale");
        }
        let rec = match parse_record(&current) {
            Ok(r) if r.id == op.item_id => r,
            Ok(_) => return self.park(&op, OpState::Failed, "id_mismatch"),
            Err((_, code)) => return self.park(&op, OpState::Failed, code),
        };
        match self.rebase(now_ms, &op, rec) {
            Ok(Rebased::Retry) => {
                self.push_retry_at = 0;
                Ok(Vec::new())
            }
            Ok(Rebased::Dropped(ids)) => {
                self.push_retry_at = 0;
                for id in &ids {
                    self.stale_streak.remove(id);
                }
                Ok(ids.into_iter().map(|op_id| Event::OpDropped { op_id, item_id: op.item_id.clone() }).collect())
            }
            Err(Error::Core(e)) => self.park(&op, OpState::Failed, e.code()),
            Err(e) => Err(e),
        }
    }

    /// Re-applies this operation, and the queued ones after it for the same item, on top of the server's latest version.
    /// The body changes, so every operation touched gets a new idempotency key before it is sent.
    fn rebase(&mut self, now_ms: u64, op: &OutboxRow, rec: Record) -> Result<Rebased, Error> {
        let mk = &self.mk;
        let item_id = op.item_id.clone();
        run_tx(&mut self.store, |tx| {
            let epoch = tx.meta()?.epoch;
            let local_seq = tx.item(&item_id)?.map(|r| r.seq).unwrap_or(0);
            let ops: Vec<OutboxRow> = tx
                .outbox()?
                .into_iter()
                .filter(|o| o.item_id == item_id && o.state == OpState::Pending && o.op_id >= op.op_id)
                .collect();
            if rec.seq <= local_seq {
                // Nothing newer to merge in; try again with a fresh key
                let mut first = op.clone();
                first.idem_key = new_id_at(now_ms)?;
                tx.put_outbox(&first)?;
                return Ok(Rebased::Retry);
            }
            if rec.deleted {
                // Deleting wins over any edit
                tx.put_item(&ItemRow { id: item_id.clone(), seq: rec.seq, deleted: true, envelope_json: None, mark: epoch })?;
                let wants_delete = matches!(open_blob(mk, op.op_id, &op.sealed_intent)?.intent, Intent::Delete);
                if wants_delete {
                    let mut first = op.clone();
                    first.idem_key = new_id_at(now_ms)?;
                    tx.put_outbox(&first)?;
                    return Ok(Rebased::Retry);
                }
                let mut dropped = Vec::new();
                for o in &ops {
                    tx.remove_outbox(o.op_id)?;
                    dropped.push(o.op_id);
                }
                return Ok(Rebased::Dropped(dropped));
            }
            let server_env = rec.envelope.ok_or(Error::MalformedResponse)?;
            let mut payload = open_envelope(mk, &server_env)?;
            let mut json = server_env.to_json()?;
            let mut deleted = false;
            for o in &ops {
                let mut blob = open_blob(mk, o.op_id, &o.sealed_intent)?;
                match &blob.intent {
                    Intent::Edit { edit } => {
                        edit.apply(&mut payload)?;
                        json = reseal(mk, &server_env, secs(now_ms), &payload)?;
                        blob.envelope = Some(json.clone());
                    }
                    Intent::Delete => {
                        deleted = true;
                        blob.envelope = None;
                    }
                    Intent::Create => {}
                }
                let mut row = o.clone();
                row.sealed_intent = seal_blob(mk, o.op_id, &blob)?;
                row.idem_key = new_id_at(now_ms)?;
                tx.put_outbox(&row)?;
            }
            let row = if deleted {
                ItemRow { id: item_id.clone(), seq: rec.seq, deleted: true, envelope_json: None, mark: epoch }
            } else {
                ItemRow { id: item_id.clone(), seq: rec.seq, deleted: false, envelope_json: Some(json), mark: epoch }
            };
            tx.put_item(&row)?;
            Ok(Rebased::Retry)
        })
    }
}
