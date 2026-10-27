//! Handling of the responses to pulls.

use crate::engine::{backoff_ms, Engine, Event};
use crate::error::Error;
use crate::push::{error_code, json_body, retry_after};
use crate::record::parse_record;
use crate::store::Store;
use crate::types::*;
use serde_json::Value;
use std::collections::HashSet;

struct Page {
    items: Vec<Value>,
    next_after: i64,
    has_more: bool,
    head_seq: i64,
}

fn parse_page(body: &[u8]) -> Option<Page> {
    let v = json_body(body)?;
    Some(Page {
        items: v.get("items")?.as_array()?.clone(),
        next_after: v.get("next_after")?.as_i64()?,
        has_more: v.get("has_more")?.as_bool()?,
        head_seq: v.get("head_seq")?.as_i64()?,
    })
}

struct Applied {
    applied: usize,
    skipped: Vec<(String, &'static str)>,
    rolled_back: bool,
}

impl<S: Store> Engine<S> {
    pub(crate) fn on_pull_response(&mut self, now_ms: u64, resp: HttpResponse) -> Result<Vec<Event>, Error> {
        match resp.status {
            200 => {}
            410 => {
                // The cursor is older than the server keeps tombstones: pull everything again from the start
                self.read(|tx| {
                    let mut m = tx.meta()?;
                    m.cursor = 0;
                    m.refetching = true;
                    m.epoch += 1;
                    tx.set_meta(&m)?;
                    Ok(())
                })?;
                self.pull_wanted = true;
                return Ok(vec![Event::Refetching]);
            }
            401 => return self.pause(PauseReason::Revoked),
            429 | 500..=599 => {
                self.pull_attempts += 1;
                let wait = retry_after(&resp).map(|s| s.clamp(1, 3600) * 1000).unwrap_or_else(|| backoff_ms(self.pull_attempts));
                self.pull_retry_at = now_ms + wait;
                return Ok(Vec::new());
            }
            _ => {
                self.pull_wanted = false;
                return Ok(vec![Event::PullFailed { code: error_code(&resp) }]);
            }
        }
        let Some(page) = parse_page(&resp.body) else {
            self.pull_wanted = false;
            return Ok(vec![Event::PullFailed { code: "malformed_response".into() }]);
        };
        self.pull_attempts = 0;
        self.pull_retry_at = 0;
        let has_more = page.has_more;
        let out = self.apply_page(page)?;
        if out.rolled_back {
            self.pull_wanted = false;
            return Ok(vec![Event::Paused(PauseReason::ServerRollback)]);
        }
        self.pull_wanted = has_more;
        let mut events: Vec<Event> =
            out.skipped.iter().map(|(id, code)| Event::SkippedRecord { id: id.clone(), code }).collect();
        events.push(Event::Pulled { applied: out.applied, skipped: out.skipped.len(), has_more });
        Ok(events)
    }

    /// Applies one page and advances the cursor in a single transaction.
    fn apply_page(&mut self, page: Page) -> Result<Applied, Error> {
        self.read(|tx| {
            let mut meta = tx.meta()?;
            if page.head_seq < meta.max_seen_seq {
                // The server reports a lower sequence number than one this client has already seen
                meta.paused = Some(PauseReason::ServerRollback);
                tx.set_meta(&meta)?;
                return Ok(Applied { applied: 0, skipped: Vec::new(), rolled_back: true });
            }
            let queued: HashSet<String> = tx.outbox()?.into_iter().map(|o| o.item_id).collect();
            let mut applied = 0;
            let mut skipped = Vec::new();
            let mut max_seen = meta.max_seen_seq.max(page.head_seq);
            for v in &page.items {
                if let Some(n) = v.get("seq").and_then(Value::as_i64) {
                    max_seen = max_seen.max(n);
                }
                let rec = match parse_record(v) {
                    Ok(r) => r,
                    Err((id, code)) => {
                        confirm(tx, &id, meta.epoch)?;
                        skipped.push((id, code));
                        continue;
                    }
                };
                let local = tx.item(&rec.id)?;
                // Never go back to an older version
                if local.as_ref().is_some_and(|l| l.seq > rec.seq) {
                    confirm(tx, &rec.id, meta.epoch)?;
                    continue;
                }
                // An item with queued changes is merged when its upload answers `stale`, not here
                if queued.contains(&rec.id) {
                    confirm(tx, &rec.id, meta.epoch)?;
                    continue;
                }
                let env_json = match &rec.envelope {
                    Some(e) => Some(e.to_json()?),
                    None => None,
                };
                tx.put_item(&ItemRow { id: rec.id, seq: rec.seq, deleted: rec.deleted, envelope_json: env_json, mark: meta.epoch })?;
                applied += 1;
            }
            meta.max_seen_seq = max_seen;
            meta.cursor = meta.cursor.max(page.next_after);
            if !page.has_more && meta.refetching {
                // The full pull is complete: forget what the server no longer has, unless a change is still queued for it
                for row in tx.items()? {
                    if row.mark < meta.epoch && !queued.contains(&row.id) {
                        tx.remove_item(&row.id)?;
                    }
                }
                meta.refetching = false;
            }
            tx.set_meta(&meta)?;
            Ok(Applied { applied, skipped, rolled_back: false })
        })
    }
}

/// Records that the server still has this item in the current pull.
fn confirm(tx: &mut dyn crate::store::Tx, id: &str, epoch: u64) -> Result<(), Error> {
    if id.is_empty() {
        return Ok(());
    }
    if let Some(mut row) = tx.item(id)? {
        if row.mark != epoch {
            row.mark = epoch;
            tx.put_item(&row)?;
        }
    }
    Ok(())
}
