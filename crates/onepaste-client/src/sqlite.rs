//! SQLite implementation of the engine's `Store`. Nothing is stored in the clear: items are server envelopes, queued
//! operations are sealed, and the device token is sealed with the master key.

use crate::error::ClientError;
use onepaste_sync::{ItemRow, OpState, OutboxRow, PauseReason, Store, StoreError, SyncMeta, Tx};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY, seq INTEGER NOT NULL, deleted INTEGER NOT NULL, envelope_json TEXT, mark INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS outbox (
  op_id INTEGER PRIMARY KEY, item_id TEXT NOT NULL, sealed_intent BLOB NOT NULL, idem_key TEXT NOT NULL,
  attempts INTEGER NOT NULL, state TEXT NOT NULL, last_error TEXT
);
CREATE TABLE IF NOT EXISTS secrets (name TEXT PRIMARY KEY, value BLOB NOT NULL);
";

pub struct SqliteStore {
    conn: Connection,
}

fn fail(e: rusqlite::Error) -> StoreError {
    StoreError::Failed(e.to_string())
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<SqliteStore, ClientError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(SqliteStore { conn })
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>, ClientError> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn secret_get(&self, name: &str) -> Result<Option<Vec<u8>>, ClientError> {
        Ok(self.conn.query_row("SELECT value FROM secrets WHERE name = ?1", [name], |r| r.get(0)).optional()?)
    }

    /// Writes several meta values and secrets in one transaction.
    pub fn save(&mut self, meta: &[(&str, &str)], secrets: &[(&str, &[u8])]) -> Result<(), ClientError> {
        let tx = self.conn.transaction()?;
        for (k, v) in meta {
            tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![k, v])?;
        }
        for (k, v) in secrets {
            tx.execute("INSERT INTO secrets (name, value) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET value = excluded.value", params![k, v])?;
        }
        tx.commit()?;
        Ok(())
    }
}

struct SqliteTx<'a> {
    tx: &'a Transaction<'a>,
}

impl SqliteTx<'_> {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.tx.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(fail)
    }

    fn put(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.tx
            .execute("INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])
            .map(|_| ())
            .map_err(fail)
    }
}

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<ItemRow> {
    Ok(ItemRow { id: r.get(0)?, seq: r.get(1)?, deleted: r.get::<_, i64>(2)? != 0, envelope_json: r.get(3)?, mark: r.get::<_, i64>(4)? as u64 })
}

fn row_to_op(r: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxRow> {
    let state: String = r.get(5)?;
    Ok(OutboxRow {
        op_id: r.get::<_, i64>(0)? as u64,
        item_id: r.get(1)?,
        sealed_intent: r.get(2)?,
        idem_key: r.get(3)?,
        attempts: r.get::<_, i64>(4)? as u32,
        state: OpState::parse(&state).unwrap_or(OpState::Failed),
        last_error: r.get(6)?,
    })
}

impl Tx for SqliteTx<'_> {
    fn meta(&self) -> Result<SyncMeta, StoreError> {
        let num = |k: &str| -> Result<i64, StoreError> { Ok(self.get(k)?.and_then(|v| v.parse().ok()).unwrap_or(0)) };
        Ok(SyncMeta {
            cursor: num("cursor")?,
            max_seen_seq: num("max_seen_seq")?,
            paused: self.get("paused")?.as_deref().and_then(PauseReason::parse),
            refetching: num("refetching")? != 0,
            epoch: num("epoch")? as u64,
            next_op_id: num("next_op_id")? as u64,
        })
    }

    fn set_meta(&mut self, m: &SyncMeta) -> Result<(), StoreError> {
        self.put("cursor", &m.cursor.to_string())?;
        self.put("max_seen_seq", &m.max_seen_seq.to_string())?;
        self.put("paused", m.paused.map(|p| p.as_str()).unwrap_or(""))?;
        self.put("refetching", if m.refetching { "1" } else { "0" })?;
        self.put("epoch", &m.epoch.to_string())?;
        self.put("next_op_id", &m.next_op_id.to_string())
    }

    fn item(&self, id: &str) -> Result<Option<ItemRow>, StoreError> {
        self.tx.query_row("SELECT id, seq, deleted, envelope_json, mark FROM items WHERE id = ?1", [id], row_to_item).optional().map_err(fail)
    }

    fn put_item(&mut self, row: &ItemRow) -> Result<(), StoreError> {
        self.tx
            .execute(
                "INSERT INTO items (id, seq, deleted, envelope_json, mark) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET seq = excluded.seq, deleted = excluded.deleted, envelope_json = excluded.envelope_json, mark = excluded.mark",
                params![row.id, row.seq, row.deleted as i64, row.envelope_json, row.mark as i64],
            )
            .map(|_| ())
            .map_err(fail)
    }

    fn remove_item(&mut self, id: &str) -> Result<(), StoreError> {
        self.tx.execute("DELETE FROM items WHERE id = ?1", [id]).map(|_| ()).map_err(fail)
    }

    fn items(&self) -> Result<Vec<ItemRow>, StoreError> {
        let mut st = self.tx.prepare("SELECT id, seq, deleted, envelope_json, mark FROM items ORDER BY id").map_err(fail)?;
        let rows = st.query_map([], row_to_item).map_err(fail)?.collect::<Result<Vec<_>, _>>().map_err(fail)?;
        Ok(rows)
    }

    fn clear_items(&mut self) -> Result<(), StoreError> {
        self.tx.execute("DELETE FROM items", []).map(|_| ()).map_err(fail)
    }

    fn outbox_row(&self, op_id: u64) -> Result<Option<OutboxRow>, StoreError> {
        self.tx
            .query_row("SELECT op_id, item_id, sealed_intent, idem_key, attempts, state, last_error FROM outbox WHERE op_id = ?1", [op_id as i64], row_to_op)
            .optional()
            .map_err(fail)
    }

    fn put_outbox(&mut self, row: &OutboxRow) -> Result<(), StoreError> {
        self.tx
            .execute(
                "INSERT INTO outbox (op_id, item_id, sealed_intent, idem_key, attempts, state, last_error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(op_id) DO UPDATE SET item_id = excluded.item_id, sealed_intent = excluded.sealed_intent, idem_key = excluded.idem_key,
                   attempts = excluded.attempts, state = excluded.state, last_error = excluded.last_error",
                params![row.op_id as i64, row.item_id, row.sealed_intent, row.idem_key, row.attempts as i64, row.state.as_str(), row.last_error],
            )
            .map(|_| ())
            .map_err(fail)
    }

    fn remove_outbox(&mut self, op_id: u64) -> Result<(), StoreError> {
        self.tx.execute("DELETE FROM outbox WHERE op_id = ?1", [op_id as i64]).map(|_| ()).map_err(fail)
    }

    fn outbox(&self) -> Result<Vec<OutboxRow>, StoreError> {
        let mut st = self.tx.prepare("SELECT op_id, item_id, sealed_intent, idem_key, attempts, state, last_error FROM outbox ORDER BY op_id").map_err(fail)?;
        let rows = st.query_map([], row_to_op).map_err(fail)?.collect::<Result<Vec<_>, _>>().map_err(fail)?;
        Ok(rows)
    }

    fn checkpoint(&mut self, name: &'static str) {
        crate::crash::crash_point(name);
    }
}

impl Store for SqliteStore {
    fn transact(&mut self, f: &mut dyn FnMut(&mut dyn Tx) -> Result<(), StoreError>) -> Result<(), StoreError> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(fail)?;
        {
            let mut t = SqliteTx { tx: &tx };
            // On error the transaction is dropped here and SQLite rolls everything back
            f(&mut t)?;
        }
        tx.commit().map_err(fail)
    }
}
