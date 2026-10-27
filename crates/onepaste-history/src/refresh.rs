//! Building and updating the history from the local items.

use crate::store::{HistoryError, HistoryStore};
use onepaste_core::crypto::Key;
use onepaste_core::envelope::{open_item, Envelope};
use onepaste_sync::{ItemRow, Store, StoreError};
use rusqlite::params;
use std::collections::HashSet;

/// What a source reports for one refresh.
pub struct SourceItems {
    /// Items whose `seq` is above the one given, items not sent yet (`seq` 0) and items with a queued change.
    pub rows: Vec<ItemRow>,
    /// The ids of all items the source holds, so items that disappeared can be dropped from the history.
    pub all_ids: Vec<String>,
}

/// Where the history reads its items from. Any sync [`Store`] is one.
pub trait ItemSource {
    fn changed_since(&mut self, seq: i64) -> Result<SourceItems, HistoryError>;
}

impl<S: Store> ItemSource for S {
    fn changed_since(&mut self, seq: i64) -> Result<SourceItems, HistoryError> {
        let mut out = SourceItems { rows: Vec::new(), all_ids: Vec::new() };
        self.transact(&mut |tx| {
            let queued: HashSet<String> = tx.outbox()?.into_iter().map(|o| o.item_id).collect();
            out.rows.clear();
            out.all_ids.clear();
            for row in tx.items()? {
                out.all_ids.push(row.id.clone());
                if row.seq > seq || row.seq == 0 || queued.contains(&row.id) {
                    out.rows.push(row);
                }
            }
            Ok(())
        })
        .map_err(|e: StoreError| HistoryError::Storage(e.to_string()))?;
        Ok(out)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RefreshReport {
    pub upserted: usize,
    pub removed: usize,
    /// Items that could not be read, with a stable error code (never any content).
    pub skipped: Vec<(String, &'static str)>,
    pub indexed_seq: i64,
}

struct Entry {
    item_id: String,
    seq: i64,
    kind: String,
    text: String,
    mime: String,
    size: i64,
    favorite: bool,
    pinned: bool,
    note: String,
    tags: String,
    source_device: String,
    created_at: i64,
}

enum Change {
    Upsert(Entry),
    Delete(String),
}

/// Decrypts one item. Images keep only their metadata: the bytes are never copied into the history.
fn read_row(row: &ItemRow, mk: &Key) -> Result<Change, &'static str> {
    if row.deleted {
        return Ok(Change::Delete(row.id.clone()));
    }
    let json = row.envelope_json.as_deref().ok_or("malformed_envelope")?;
    let env: Envelope = serde_json::from_str(json).map_err(|_| "malformed_envelope")?;
    let payload = open_item(mk, &env).map_err(|e| e.code())?;
    let is_image = payload.kind == "image";
    let mime = payload.extra.get("mime").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let size = payload.extra.get("size").and_then(|v| v.as_i64()).unwrap_or(env.size as i64);
    Ok(Change::Upsert(Entry {
        item_id: row.id.clone(),
        seq: row.seq,
        kind: if is_image { "image".to_string() } else { "text".to_string() },
        text: if is_image { String::new() } else { payload.content.unwrap_or_default() },
        mime,
        size,
        favorite: payload.favorite,
        pinned: payload.pinned,
        note: payload.note.unwrap_or_default(),
        tags: payload.tags.join("\n"),
        source_device: payload.device.unwrap_or_default(),
        created_at: env.created_at,
    }))
}

impl HistoryStore {
    /// Brings the history up to date with the source: decrypts what changed, then updates rows, index and the
    /// processed sequence number in one transaction, so a failure leaves everything as it was.
    pub fn refresh<S: ItemSource + ?Sized>(&mut self, source: &mut S, mk: &Key) -> Result<RefreshReport, HistoryError> {
        let indexed = self.indexed_seq()?;
        let items = source.changed_since(indexed)?;
        let mut changes = Vec::new();
        let mut skipped = Vec::new();
        let mut top = indexed;
        for row in &items.rows {
            top = top.max(row.seq);
            match read_row(row, mk) {
                Ok(c) => changes.push(c),
                Err(code) => skipped.push((row.id.clone(), code)),
            }
        }
        let present: HashSet<&str> = items.all_ids.iter().map(String::as_str).collect();

        let tx = self.conn.transaction()?;
        let mut removed = 0;
        let mut upserted = 0;
        {
            let known: Vec<String> = tx.prepare("SELECT item_id FROM history")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for id in known.iter().filter(|id| !present.contains(id.as_str())) {
                removed += tx.execute("DELETE FROM history WHERE item_id = ?1", [id])?;
            }
            let mut upsert = tx.prepare(
                "INSERT INTO history (item_id, seq, kind, text, mime, size, favorite, pinned, note, tags, source_device, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(item_id) DO UPDATE SET seq = excluded.seq, kind = excluded.kind, text = excluded.text, mime = excluded.mime,
                   size = excluded.size, favorite = excluded.favorite, pinned = excluded.pinned, note = excluded.note, tags = excluded.tags,
                   source_device = excluded.source_device, created_at = excluded.created_at",
            )?;
            for change in &changes {
                match change {
                    Change::Delete(id) => removed += tx.execute("DELETE FROM history WHERE item_id = ?1", [id])?,
                    Change::Upsert(e) => {
                        upsert.execute(params![
                            e.item_id, e.seq, e.kind, e.text, e.mime, e.size, e.favorite, e.pinned, e.note, e.tags, e.source_device, e.created_at
                        ])?;
                        upserted += 1;
                    }
                }
            }
        }
        tx.execute(
            "INSERT INTO state (key, value) VALUES ('indexed_seq', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [top],
        )?;
        tx.commit()?;
        Ok(RefreshReport { upserted, removed, skipped, indexed_seq: top })
    }

    /// Empties the history and builds it again from the source. The result equals what an unbroken series of
    /// refreshes would have produced.
    pub fn rebuild<S: ItemSource + ?Sized>(&mut self, source: &mut S, mk: &Key) -> Result<RefreshReport, HistoryError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM history", [])?;
        tx.execute("INSERT INTO state (key, value) VALUES ('indexed_seq', 0) ON CONFLICT(key) DO UPDATE SET value = 0", [])?;
        tx.commit()?;
        self.refresh(source, mk)
    }
}
