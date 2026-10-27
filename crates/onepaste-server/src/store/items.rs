//! Items: writes with optimistic concurrency on `seq`, and pulls by cursor.

use super::{Reply, Store};
use crate::error::ApiError;
use axum::http::StatusCode;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};

pub enum ItemWrite {
    /// The canonical envelope JSON.
    Upsert(String),
    Delete,
}

struct Row {
    seq: i64,
    deleted: bool,
    envelope: Option<String>,
}

fn record_json(id: &str, row: &Row) -> Value {
    let mut v = json!({ "id": id, "seq": row.seq, "deleted": row.deleted });
    if let (Some(env), Value::Object(m)) = (&row.envelope, &mut v) {
        if let Ok(e) = serde_json::from_str::<Value>(env) {
            m.insert("envelope".to_string(), e);
        }
    }
    v
}

impl Store {
    pub fn put_item_tx(tx: &Transaction, id: &str, base_seq: i64, write: &ItemWrite, now: i64) -> Result<Reply, ApiError> {
        let cur: Option<Row> = tx
            .query_row("SELECT seq, deleted, envelope FROM items WHERE id = ?1", [id], |r| {
                Ok(Row { seq: r.get(0)?, deleted: r.get::<_, i64>(1)? != 0, envelope: r.get(2)? })
            })
            .optional()?;
        let cur_seq = cur.as_ref().map(|r| r.seq).unwrap_or(0);
        let stale = |cur: &Option<Row>| {
            let mut e = ApiError::new(StatusCode::CONFLICT, "stale");
            if let Some(r) = cur {
                e = e.with("current_item", record_json(id, r));
            }
            e
        };
        if base_seq != cur_seq {
            return Err(stale(&cur));
        }
        let reply = |seq: i64, notify: bool| Reply {
            status: 200,
            body: json!({ "seq": seq }).to_string().into_bytes(),
            stored: None,
            notify_seq: notify.then_some(seq),
            replayed: false,
        };
        match write {
            ItemWrite::Upsert(envelope) => {
                if cur.as_ref().is_some_and(|r| r.deleted) {
                    // Deleting wins: a tombstone is never resurrected
                    return Err(stale(&cur));
                }
                let seq = next_seq(tx)?;
                tx.execute(
                    "INSERT INTO items (id, seq, deleted, envelope, deleted_at) VALUES (?1, ?2, 0, ?3, NULL)
                     ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, envelope = excluded.envelope",
                    params![id, seq, envelope],
                )?;
                Ok(reply(seq, true))
            }
            ItemWrite::Delete => {
                match &cur {
                    None => return Err(ApiError::not_found()),
                    Some(r) if r.deleted => return Ok(reply(r.seq, false)),
                    Some(_) => {}
                }
                let seq = next_seq(tx)?;
                tx.execute(
                    "UPDATE items SET seq = ?1, deleted = 1, envelope = NULL, deleted_at = ?2 WHERE id = ?3",
                    params![seq, now, id],
                )?;
                Ok(reply(seq, true))
            }
        }
    }

    /// One page of records with `seq > after`, plus `head_seq`, all read from one snapshot.
    ///
    /// A cursor below the purge point is expired, because tombstones the client never saw are gone. The exception is a
    /// cursor this server handed to the same device after the last purge: that device is in the middle of a full pull
    /// from 0 and is rebuilding its copy, so it does not depend on the purged tombstones.
    pub fn list_items(&self, device_id: &str, after: i64, limit: i64) -> Result<Value, ApiError> {
        let (page, floor, generation, next_after) = self.read(|tx| {
            let (head, floor): (i64, i64) =
                tx.query_row("SELECT seq, cursor_floor FROM vault WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let generation: i64 = tx
                .query_row("SELECT value FROM kv WHERE key = 'purge_generation'", [], |r| r.get(0))
                .optional()?
                .unwrap_or(0);
            if after > 0 && after < floor {
                let issued: Option<(i64, i64)> = tx
                    .query_row("SELECT next_after, generation FROM pull_state WHERE device_id = ?1", [device_id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?;
                if issued != Some((after, generation)) {
                    return Err(ApiError::new(StatusCode::GONE, "cursor_expired"));
                }
            }
            let mut st = tx.prepare(
                "SELECT id, seq, deleted, envelope FROM items WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
            )?;
            let rows = st
                .query_map(params![after, limit + 1], |r| {
                    Ok((r.get::<_, String>(0)?, Row { seq: r.get(1)?, deleted: r.get::<_, i64>(2)? != 0, envelope: r.get(3)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let has_more = rows.len() as i64 > limit;
            let page: Vec<&(String, Row)> = rows.iter().take(limit as usize).collect();
            let next_after = page.last().map(|(_, r)| r.seq).unwrap_or(after);
            let items: Vec<Value> = page.iter().map(|(id, r)| record_json(id, r)).collect();
            Ok((json!({ "items": items, "next_after": next_after, "has_more": has_more, "head_seq": head }), floor, generation, next_after))
        })?;
        if floor > 0 {
            // Remember the cursor that was just handed out (only needed once something has been purged)
            self.write(|tx| {
                tx.execute(
                    "INSERT INTO pull_state (device_id, next_after, generation) VALUES (?1, ?2, ?3)
                     ON CONFLICT(device_id) DO UPDATE SET next_after = excluded.next_after, generation = excluded.generation",
                    params![device_id, next_after, generation],
                )?;
                Ok(())
            })?;
        }
        Ok(page)
    }
}

fn next_seq(tx: &Transaction) -> Result<i64, ApiError> {
    tx.execute("UPDATE vault SET seq = seq + 1 WHERE id = 1", [])?;
    Ok(tx.query_row("SELECT seq FROM vault WHERE id = 1", [], |r| r.get(0))?)
}
