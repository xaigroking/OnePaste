//! SQLite storage. Only code under this module executes SQL that writes to the database.

mod items;
mod vault;

pub use items::ItemWrite;
pub use vault::RegisterIn;

use crate::error::ApiError;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS vault (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  seq INTEGER NOT NULL,
  cursor_floor INTEGER NOT NULL,
  keyparams TEXT,
  keyparams_version INTEGER NOT NULL,
  proof_hash BLOB
);
INSERT OR IGNORE INTO vault (id, seq, cursor_floor, keyparams, keyparams_version, proof_hash) VALUES (1, 0, 0, NULL, 0, NULL);
CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY,
  token_hash BLOB NOT NULL UNIQUE,
  sealed_name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS reg_tokens (
  hash BLOB PRIMARY KEY,
  expires_at INTEGER NOT NULL,
  used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY,
  seq INTEGER NOT NULL UNIQUE,
  deleted INTEGER NOT NULL,
  envelope TEXT,
  deleted_at INTEGER
);
CREATE TABLE IF NOT EXISTS idempotency (
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  req_hash BLOB NOT NULL,
  status INTEGER NOT NULL,
  body BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (scope, key)
);
";

pub struct Store {
    conn: Mutex<Connection>,
}

/// The outcome of a write: what to send, what to keep for replays, and the new global sequence number.
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// When set, this is stored for replays instead of `body` (used when the body holds a secret).
    pub stored: Option<Vec<u8>>,
    pub notify_seq: Option<i64>,
    pub replayed: bool,
}

pub struct DeviceRow {
    pub id: String,
    pub sealed_name: String,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked: bool,
}

impl Store {
    pub fn open(data_dir: &Path) -> Result<Store, ApiError> {
        std::fs::create_dir_all(data_dir).map_err(|_| ApiError::internal())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700));
        }
        let db_path = data_dir.join("onepaste.db");
        let conn = Connection::open(&db_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `f` in one write transaction; an error rolls everything back.
    fn write<R>(&self, f: impl FnOnce(&Transaction) -> Result<R, ApiError>) -> Result<R, ApiError> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Runs `f` in one read transaction (a consistent snapshot).
    fn read<R>(&self, f: impl FnOnce(&Transaction) -> Result<R, ApiError>) -> Result<R, ApiError> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        f(&tx)
    }

    pub fn device_count(&self) -> Result<i64, ApiError> {
        self.read(|tx| Ok(tx.query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?))
    }

    /// Looks up a device token hash. A revoked or unknown device is `unauthorized`; success records `last_seen_at`.
    pub fn auth_device(&self, token_hash: &[u8; 32], now: i64) -> Result<String, ApiError> {
        self.write(|tx| {
            let row: Option<(String, i64)> = tx
                .query_row("SELECT id, revoked FROM devices WHERE token_hash = ?1", [&token_hash[..]], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            match row {
                Some((id, 0)) => {
                    tx.execute("UPDATE devices SET last_seen_at = ?1 WHERE id = ?2", params![now, id])?;
                    Ok(id)
                }
                _ => Err(ApiError::unauthorized()),
            }
        })
    }

    /// Whether a registration credential is valid (known, unused and not expired). It is not consumed.
    pub fn reg_token_valid(&self, hash: &[u8; 32], now: i64) -> Result<bool, ApiError> {
        self.read(|tx| reg_token_valid_tx(tx, hash, now))
    }

    /// Creates a registration credential. With `invalidate_others`, every unused one is invalidated first.
    pub fn create_reg_token(
        &self,
        hash: &[u8; 32],
        now: i64,
        ttl: i64,
        invalidate_others: bool,
    ) -> Result<i64, ApiError> {
        self.write(|tx| create_reg_token_tx(tx, hash, now, ttl, invalidate_others))
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRow>, ApiError> {
        self.read(|tx| {
            let mut st =
                tx.prepare("SELECT id, sealed_name, created_at, last_seen_at, revoked FROM devices ORDER BY created_at, id")?;
            let rows = st
                .query_map([], |r| {
                    Ok(DeviceRow {
                        id: r.get(0)?,
                        sealed_name: r.get(1)?,
                        created_at: r.get(2)?,
                        last_seen_at: r.get(3)?,
                        revoked: r.get::<_, i64>(4)? != 0,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Revokes a device. It takes effect for the very next request because every request looks the token up here.
    pub fn revoke_device_tx(tx: &Transaction, id: &str) -> Result<(), ApiError> {
        let n = tx.execute("UPDATE devices SET revoked = 1 WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(ApiError::not_found());
        }
        Ok(())
    }

    /// Runs a write under the idempotency rules. A repeated key with the same request replays the stored response;
    /// a repeated key with a different request is `idempotency_conflict`. Only successful responses are stored.
    pub fn idempotent(
        &self,
        scope: &str,
        key: &str,
        req_hash: &[u8; 32],
        now: i64,
        ttl: i64,
        f: impl FnOnce(&Transaction) -> Result<Reply, ApiError>,
    ) -> Result<Reply, ApiError> {
        self.write(|tx| {
            let prev: Option<(Vec<u8>, i64, Vec<u8>, i64)> = tx
                .query_row(
                    "SELECT req_hash, status, body, created_at FROM idempotency WHERE scope = ?1 AND key = ?2",
                    params![scope, key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            if let Some((h, status, body, created)) = prev {
                if created.saturating_add(ttl) > now {
                    if crate::util::ct_eq(&h, req_hash) {
                        return Ok(Reply { status: status as u16, body, stored: None, notify_seq: None, replayed: true });
                    }
                    return Err(ApiError::new(
                        axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                        "idempotency_conflict",
                    ));
                }
                tx.execute("DELETE FROM idempotency WHERE scope = ?1 AND key = ?2", params![scope, key])?;
            }
            let reply = f(tx)?;
            let kept: &[u8] = reply.stored.as_deref().unwrap_or(&reply.body);
            tx.execute(
                "INSERT INTO idempotency (scope, key, req_hash, status, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![scope, key, &req_hash[..], i64::from(reply.status), kept, now],
            )?;
            Ok(reply)
        })
    }

    /// Daily maintenance: purge old tombstones, raise the cursor floor, drop expired idempotency records and credentials.
    pub fn cleanup(&self, now: i64, tombstone_retention: i64, idempotency_ttl: i64) -> Result<(), ApiError> {
        self.write(|tx| {
            let cutoff = now.saturating_sub(tombstone_retention);
            let max_purged: Option<i64> = tx.query_row(
                "SELECT MAX(seq) FROM items WHERE deleted = 1 AND deleted_at < ?1",
                [cutoff],
                |r| r.get(0),
            )?;
            if let Some(m) = max_purged {
                tx.execute("DELETE FROM items WHERE deleted = 1 AND deleted_at < ?1", [cutoff])?;
                tx.execute("UPDATE vault SET cursor_floor = MAX(cursor_floor, ?1) WHERE id = 1", [m])?;
            }
            tx.execute("DELETE FROM idempotency WHERE created_at < ?1", [now.saturating_sub(idempotency_ttl)])?;
            tx.execute("DELETE FROM reg_tokens WHERE used = 1 OR expires_at < ?1", [now.saturating_sub(idempotency_ttl)])?;
            Ok(())
        })
    }

    /// Revokes a device inside an idempotent write.
    pub fn revoke_device(&self, id: &str, scope: &str, key: &str, req_hash: &[u8; 32], now: i64, ttl: i64) -> Result<Reply, ApiError> {
        self.idempotent(scope, key, req_hash, now, ttl, |tx| {
            Store::revoke_device_tx(tx, id)?;
            Ok(Reply { status: 204, body: Vec::new(), stored: None, notify_seq: None, replayed: false })
        })
    }
}

fn reg_token_valid_tx(tx: &Transaction, hash: &[u8; 32], now: i64) -> Result<bool, ApiError> {
    let row: Option<(i64, i64)> = tx
        .query_row("SELECT expires_at, used FROM reg_tokens WHERE hash = ?1", [&hash[..]], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(matches!(row, Some((exp, 0)) if exp > now))
}

fn create_reg_token_tx(tx: &Transaction, hash: &[u8; 32], now: i64, ttl: i64, invalidate_others: bool) -> Result<i64, ApiError> {
    if invalidate_others {
        tx.execute("UPDATE reg_tokens SET used = 1 WHERE used = 0", [])?;
    }
    let expires = now.saturating_add(ttl);
    tx.execute("INSERT INTO reg_tokens (hash, expires_at, used) VALUES (?1, ?2, 0)", params![&hash[..], expires])?;
    Ok(expires)
}
